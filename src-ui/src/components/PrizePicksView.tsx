import { useState, useEffect, useCallback, useRef, useMemo } from 'react';

const COLLAPSED_STORAGE_KEY = 'prizepicks_collapsed_games';
const PREFERENCES_STORAGE_KEY = 'prizepicks_dashboard_preferences';
const WATCHLIST_STORAGE_KEY = 'prizepicks_watchlist';

interface DashboardPreferences {
  sortKey: 'name' | 'edge' | 'confidence' | 'projection';
  sortDir: 'asc' | 'desc';
  minEdge: number;
  selectedCategories: string[];
  selectedTeam: string;
  playerFilter: string;
}

const DEFAULT_PREFERENCES: DashboardPreferences = {
  sortKey: 'edge',
  sortDir: 'desc',
  minEdge: 0,
  selectedCategories: [],
  selectedTeam: 'All',
  playerFilter: '',
};

function loadCollapsed(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(COLLAPSED_STORAGE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function saveCollapsed(state: Record<string, boolean>) {
  try {
    localStorage.setItem(COLLAPSED_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // localStorage may be full or unavailable — silently ignore
  }
}

function loadPreferences(): DashboardPreferences {
  try {
    const raw = localStorage.getItem(PREFERENCES_STORAGE_KEY);
    if (!raw) return DEFAULT_PREFERENCES;
    const parsed = JSON.parse(raw);
    // Handle legacy single-category format: if selectedCategory (string) exists,
    // migrate to selectedCategories array. Ignore old key if new key already set.
    if (typeof parsed.selectedCategory === 'string' && !Array.isArray(parsed.selectedCategories)) {
      parsed.selectedCategories = parsed.selectedCategory === 'All' ? [] : [parsed.selectedCategory];
    }
    delete parsed.selectedCategory; // clean up legacy key
    // Merge with defaults to handle missing fields gracefully
    return { ...DEFAULT_PREFERENCES, ...parsed };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

function savePreferences(prefs: DashboardPreferences) {
  try {
    localStorage.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // localStorage may be full or unavailable — silently ignore
  }
}

function loadWatchlist(): string[] {
  try {
    const raw = localStorage.getItem(WATCHLIST_STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveWatchlist(ids: string[]) {
  try {
    localStorage.setItem(WATCHLIST_STORAGE_KEY, JSON.stringify(ids));
  } catch {
    // silently ignore
  }
}
import { prizepicksApi } from '../services/prizepicks';
import type { PrizePicksCacheStatus } from '../types/prizepicks';
import type { PropPick, ScoredProp } from '../types';

const INITIAL_PROP_LIMIT = 50;

function formatEdge(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '\u2014';
}

function formatProb(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '\u2014';
}

function edgeLevelClass(edge: number | undefined | null): string {
  if (edge == null || !Number.isFinite(edge)) return '';
  if (edge >= 10) return 'edge-high';
  if (edge >= 5) return 'edge-good';
  if (edge >= 2) return 'edge-modest';
  if (edge <= -2) return 'edge-poor';
  return '';
}

/**
 * Human-readable relative game time label (e.g. "in 3h", "tomorrow", "today").
 * Returns empty string when the date is invalid or missing.
 */
function gameTimeRelative(gameTime: string | undefined | null): string {
  if (!gameTime) return '';
  const now = Date.now();
  const gameDate = new Date(gameTime).getTime();
  if (!Number.isFinite(gameDate)) return '';
  const diffMs = gameDate - now;
  const diffSec = Math.round(diffMs / 1000);
  const absSec = Math.abs(diffSec);

  if (diffSec < 0) {
    // Past
    if (absSec < 60) return 'just now';
    if (absSec < 3600) return `${Math.floor(absSec / 60)}m ago`;
    if (absSec < 86400) return `${Math.floor(absSec / 3600)}h ago`;
    if (absSec < 172800) return 'yesterday';
    return `${Math.floor(absSec / 86400)}d ago`;
  }
  // Future
  if (absSec < 60) return 'soon';
  if (absSec < 3600) return `in ${Math.floor(absSec / 60)}m`;
  if (absSec < 86400) return `in ${Math.floor(absSec / 3600)}h`;
  if (absSec < 172800) return 'tomorrow';
  if (absSec < 604800) return `in ${Math.floor(absSec / 86400)}d`;
  // Further out \u2014 show short date
  const d = new Date(gameTime);
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function formatTimeAgo(ts: number): string {
  if (!ts) return 'never';
  const seconds = Math.floor(Date.now() / 1000 - ts);
  if (seconds < 0) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

function copyPropToClipboard(prop: PropPick) {
  const text = [
    `${prop.player} \u2014 ${prop.prop_type}`,
    `Line: ${prop.line} | Projection: ${prop.projection.toFixed(1)}`,
    `Edge: ${formatEdge(prop.edge_pct)} | Confidence: ${prop.confidence}%`,
    `Team: ${prop.team || 'N/A'} | Game: ${prop.game || 'N/A'} | League: ${prop.league}`,
    `Recommendation: ${prop.recommendation}`,
  ].join('\n');

  navigator.clipboard.writeText(text).catch((err) => {
    console.error('[PrizePicks] Failed to copy prop:', err);
  });
}

export function PrizePicksView() {
  // Load preferences from localStorage on mount
  const savedPreferences = loadPreferences();

  const [props, setProps] = useState<PropPick[]>([]);
  const [scoredProps, setScoredProps] = useState<ScoredProp[]>([]);
  const [selectedLeague, setSelectedLeague] = useState('All');
  const [selectedCategories, setSelectedCategories] = useState<string[]>(savedPreferences.selectedCategories);
  const [selectedTeam, setSelectedTeam] = useState(savedPreferences.selectedTeam);
  const [searchQuery, setSearchQuery] = useState('');
  const [playerFilter, setPlayerFilter] = useState(savedPreferences.playerFilter);
  type PropsSortKey = 'name' | 'edge' | 'confidence' | 'projection';
  const [sortKey, setSortKey] = useState<PropsSortKey>(savedPreferences.sortKey);
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>(savedPreferences.sortDir);
  const [minEdge, setMinEdge] = useState(savedPreferences.minEdge);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cacheStatus, setCacheStatus] = useState<PrizePicksCacheStatus | null>(null);
  const [collapsedGames, setCollapsedGames] = useState<Record<string, boolean>>(loadCollapsed);
  const [watchlist, setWatchlist] = useState<string[]>(loadWatchlist);
  const [showWatchlist, setShowWatchlist] = useState(false);
  // True when any filter control is set to a non-default value
  const hasActiveFilters = sortKey !== DEFAULT_PREFERENCES.sortKey || sortDir !== DEFAULT_PREFERENCES.sortDir || minEdge > 0 || selectedCategories.length > 0 || selectedTeam !== 'All' || playerFilter !== '' || showWatchlist;

  const resetFilters = () => {
    setSortKey(DEFAULT_PREFERENCES.sortKey);
    setSortDir(DEFAULT_PREFERENCES.sortDir);
    setMinEdge(0);
    setSelectedCategories([]);
    setSelectedTeam('All');
    setPlayerFilter('');
    setShowWatchlist(false);
  };

  const toggleWatchlistProp = (propId: string) => {
    setWatchlist((prev) => {
      const next = prev.includes(propId)
        ? prev.filter((id) => id !== propId)
        : [...prev, propId];
      saveWatchlist(next);
      return next;
    });
  };

  const requestId = useRef(0);

  // Persist preferences to localStorage when they change
  useEffect(() => {
    savePreferences({
      sortKey,
      sortDir,
      minEdge,
      selectedCategories,
      selectedTeam,
      playerFilter,
    });
  }, [sortKey, sortDir, minEdge, selectedCategories, selectedTeam, playerFilter]);

  const toggleGameGroup = (key: string) => {
    setCollapsedGames((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      saveCollapsed(next);
      return next;
    });
  };

  const leagues = ['All', 'NFL', 'NBA', 'MLB', 'NHL'];

  // Compute prop count per league for tab badges
  const leagueCounts = useMemo(() => {
    const counts: Record<string, number> = { All: props.length };
    for (const lg of leagues) {
      if (lg === 'All') continue;
      counts[lg] = props.filter((p) => p.league === lg).length;
    }
    return counts;
  }, [props]);

  // Derive the active data source label from current props
  // (all props from a single fetch share the same source)
  const dataSource = useMemo(() => {
    const sources = new Set(props.map((p) => p.source).filter(Boolean));
    if (sources.size === 0) return null;
    if (sources.size === 1) {
      const s = sources.values().next().value!;
      const labels: Record<string, string> = {
        opticodds: '\uD83D\uDD2E OpticOdds',
        'the-odds-api': '\uD83D\uDCCA The Odds API',
        espn: '\uD83D\uDCFA ESPN',
        sleeper: '\uD83D\uDE34 Sleeper',
        mock: '\uD83E\uDDEA Mock',
      };
      return labels[s] ?? s;
    }
    return '\uD83D\uDD04 Multi-source';
  }, [props]);

  // Reset filters when props are reloaded (e.g. league change)
  useEffect(() => {
    setSelectedCategories([]);
    setSelectedTeam('All');
    setPlayerFilter('');
    setShowWatchlist(false);
  }, [props]);

  // Compute unique stat categories from the loaded props
  const categories = useMemo(() => {
    const cats = new Set(props.map((p) => p.prop_type).filter(Boolean));
    return Array.from(cats).sort();
  }, [props]);

  // Compute unique team abbreviations from the loaded props
  const teams = useMemo(() => {
    const tm = new Set(props.map((p) => p.team).filter(Boolean));
    return ['All', ...Array.from(tm).sort()];
  }, [props]);

  const loadProps = useCallback(async (opts?: { query?: string; league?: string }) => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);
    const league = opts?.league ?? selectedLeague;
    const query = (opts?.query ?? '').trim();

    try {
      const data = query
        ? await prizepicksApi.searchProps(query)
        : league === 'All'
          ? await prizepicksApi.getTopProps(INITIAL_PROP_LIMIT)
          : await prizepicksApi.getProps(league);

      if (id !== requestId.current) return;
      setProps(data);
    } catch (e) {
      if (id !== requestId.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, [selectedLeague]);

  const loadScored = useCallback(async () => {
    try {
      const data = await prizepicksApi.getScoredProps();
      setScoredProps(data);
    } catch {
      // non-fatal
    }
  }, []);

  /**
   * Initial-mount loader \u2014 uses the single-call `getDashboardBootstrap`
   * endpoint so the top-props, scored-props, and cache-status slices
   * arrive in one IPC round-trip instead of three.
   */
  const loadDashboardBootstrap = useCallback(async () => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);
    try {
      const data = await prizepicksApi.getDashboardBootstrap(INITIAL_PROP_LIMIT);
      if (id !== requestId.current) return;
      setProps(data.props);
      setScoredProps(data.scored_props as ScoredProp[]);
      setCacheStatus(data.cache_status);
    } catch (e) {
      if (id !== requestId.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadDashboardBootstrap();
  }, [loadDashboardBootstrap]);

  const runSearch = () => {
    void loadProps({ query: searchQuery, league: selectedLeague });
  };

  const refreshAll = async () => {
    setRefreshing(true);
    setError(null);
    try {
      await prizepicksApi.refresh();
      await loadProps({ league: selectedLeague });
      await loadScored();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRefreshing(false);
    }
  };

  // Client-side filter by stat category (multi-select), team, player name, and minimum edge
  const displayProps = useMemo(() => {
    let filtered = selectedCategories.length === 0
      ? props
      : props.filter((p) => p.prop_type && selectedCategories.includes(p.prop_type));
    if (selectedTeam !== 'All') {
      filtered = filtered.filter((p) => p.team === selectedTeam);
    }
    if (playerFilter) {
      const q = playerFilter.toLowerCase();
      filtered = filtered.filter((p) => p.player.toLowerCase().includes(q));
    }
    if (minEdge > 0) {
      filtered = filtered.filter((p) => (p.edge_pct ?? 0) >= minEdge);
    }
    if (showWatchlist) {
      filtered = filtered.filter((p) => watchlist.includes(p.id));
    }
    return filtered;
  }, [props, selectedCategories, selectedTeam, playerFilter, minEdge, showWatchlist, watchlist]);

  // Client-side sort by edge/confidence/name/projection
  const sortedProps = useMemo(() => {
    const copy = [...displayProps];
    copy.sort((a, b) => {
      let cmp = 0;
      switch (sortKey) {
        case 'name':
          cmp = a.player.localeCompare(b.player);
          break;
        case 'edge':
          cmp = a.edge_pct - b.edge_pct;
          break;
        case 'confidence':
          cmp = a.confidence - b.confidence;
          break;
        case 'projection':
          cmp = a.projection - b.projection;
          break;
      }
      return sortDir === 'desc' ? -cmp : cmp;
    });
    return copy;
  }, [displayProps, sortKey, sortDir]);

  // Group sorted props by game, sorted chronologically by game_time
  const groupedGames = useMemo(() => {
    const groups = new Map<string, PropPick[]>();
    for (const prop of sortedProps) {
      const key = prop.game || 'Other';
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key)!.push(prop);
    }
    const entries = Array.from(groups.entries());
    entries.sort((a, b) => {
      const propsA = a[1];
      const propsB = b[1];
      const ta = propsA[0]?.game_time;
      const tb = propsB[0]?.game_time;
      if (a[0] === 'Other') return 1;
      if (b[0] === 'Other') return -1;
      if (ta && tb) return ta.localeCompare(tb);
      if (ta) return -1;
      if (tb) return 1;
      return a[0].localeCompare(b[0]);
    });
    return entries;
  }, [sortedProps]);

  // Dashboard summary stats computed from the filtered (displayed) props
  const dashboardSummary = useMemo(() => {
    const total = displayProps.length;
    if (total === 0) return null;
    let totalEdge = 0;
    let edgeCount = 0;
    let totalConf = 0;
    let confCount = 0;
    let highEdgeCount = 0;
    let modestEdgeCount = 0;
    let bestEdge = -Infinity;
    let bestPlayer = '';
    for (const p of displayProps) {
      if (p.edge_pct != null && Number.isFinite(p.edge_pct)) {
        totalEdge += p.edge_pct;
        edgeCount++;
        if (p.edge_pct >= 5) highEdgeCount++;
        else if (p.edge_pct >= 2) modestEdgeCount++;
        if (p.edge_pct > bestEdge) {
          bestEdge = p.edge_pct;
          bestPlayer = p.player;
        }
      }
      if (p.confidence != null) {
        totalConf += p.confidence;
        confCount++;
      }
    }
    return {
      total,
      avgEdge: edgeCount > 0 ? totalEdge / edgeCount : 0,
      avgConf: confCount > 0 ? totalConf / confCount : 0,
      highEdgeCount,
      modestEdgeCount,
      bestEdge: bestEdge > -Infinity ? bestEdge : 0,
      bestPlayer,
    };
  }, [displayProps]);

  // Helper to toggle a category in the multi-select set
  const toggleCategory = (cat: string) => {
    setSelectedCategories((prev) => {
      if (prev.includes(cat)) {
        return prev.filter((c) => c !== cat);
      }
      return [...prev, cat];
    });
  };

  return (
    <div className="prizepicksPage">
      <header className="prizepicksHeader">
        <div>
          <h2>Player Props</h2>
          <p className="muted">DFS prop board with edge analysis and projections</p>
        </div>
        <div className="prizepicksHeaderActions">
          {cacheStatus && (
            <span
              className={`chip small ${
                cacheStatus.full_catalog ? 'info' : cacheStatus.has_cache ? 'warn' : ''
              }`}
              title={
                cacheStatus.has_cache
                  ? `${cacheStatus.markets_count} markets \u00B7 ${cacheStatus.full_catalog ? 'Full catalog' : 'Partial cache (quick load)'}${cacheStatus.is_stale ? ' \u00B7 stale' : ''}`
                  : 'Cache empty \u2014 awaiting first load'
              }
            >
              {cacheStatus.full_catalog
                ? `\uD83D\uDCE6 ${cacheStatus.markets_count}`
                : cacheStatus.has_cache
                  ? `\uD83D\uDCE6 ${cacheStatus.markets_count}*`
                  : '\uD83D\uDCE6 empty'}
            </span>
          )}
          {cacheStatus?.has_cache && (
            <span
              className={`chip small lastUpdated${cacheStatus.is_stale ? ' stale' : ''}`}
              title={`Last fetched at ${new Date(cacheStatus.fetched_at * 1000).toLocaleString()}`}
            >
              Updated {formatTimeAgo(cacheStatus.fetched_at)}
            </span>
          )}
          {dataSource && (
            <span className="chip small sourceChip" title={`Data provided by ${dataSource}`}>
              {dataSource}
            </span>
          )}
          <button type="button" className="primaryBtn" onClick={() => void refreshAll()} disabled={refreshing || loading}>
          {refreshing ? 'Refreshing\u2026' : 'Refresh props'}
        </button>
        </div>
      </header>

      <div className="prizepicksToolbar">
        <input
          className="searchInput"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search player or prop\u2026"
          onKeyDown={(e) => e.key === 'Enter' && runSearch()}
        />
        <button type="button" className="ghostBtn" onClick={runSearch} disabled={loading}>
          Search
        </button>
      </div>

      {/* League filter chips */}
      <div className="categoryRow">
        {leagues.map((lg) => (
          <button
            key={lg}
            type="button"
            className={`chip ${selectedLeague === lg ? 'active' : ''}`}
            onClick={() => setSelectedLeague(lg)}
            disabled={loading}
          >
            {lg}
            {leagueCounts[lg] !== undefined && !loading && (
              <span className="leagueCountBadge">{leagueCounts[lg]}</span>
            )}
          </button>
        ))}
        <button
          type="button"
          className={`chip ${showWatchlist ? 'active' : ''}`}
          onClick={() => setShowWatchlist((v) => !v)}
          disabled={loading}
          title={showWatchlist ? 'Show all props' : 'Show only bookmarked props'}
        >
          {showWatchlist ? '\u2B50 Watchlist' : '\u2606 Watchlist'}
          {watchlist.length > 0 && !loading && (
            <span className="leagueCountBadge">{watchlist.length}</span>
          )}
        </button>
        {/* Bulk watchlist actions — appear when filtered props are available */}
        {displayProps.length > 0 && !loading && (
          <span className="bulkWatchActions">
            {displayProps.some((p) => !watchlist.includes(p.id)) && (
              <button
                type="button"
                className="ghostBtn small"
                onClick={() => {
                  setWatchlist((prev) => {
                    const ids = new Set(prev);
                    for (const p of displayProps) {
                      ids.add(p.id);
                    }
                    const next = Array.from(ids);
                    saveWatchlist(next);
                    return next;
                  });
                }}
                title="Add all visible props to watchlist"
                aria-label="Add all visible props to watchlist"
              >
                {'\u2B50'} All {displayProps.length}
              </button>
            )}
            {displayProps.some((p) => watchlist.includes(p.id)) && (
              <button
                type="button"
                className="ghostBtn small"
                onClick={() => {
                  setWatchlist((prev) => {
                    const visibleIds = new Set(displayProps.map((p) => p.id));
                    const next = prev.filter((id) => !visibleIds.has(id));
                    saveWatchlist(next);
                    return next;
                  });
                }}
                title="Remove visible props from watchlist"
                aria-label="Remove visible props from watchlist"
              >
                {'\u2606'} Unwatch
              </button>
            )}
          </span>
        )}
      </div>

      {/* Stat category filter chips -- multi-select toggle */}
      {!loading && categories.length > 0 && (
        <div className="categoryRow categoryRowCategories">
          <button
            type="button"
            className={`chip small ${selectedCategories.length === 0 ? 'active' : ''}`}
            onClick={() => setSelectedCategories([])}
            disabled={loading || props.length === 0}
            title="Show all categories"
          >
            All
          </button>
          {categories.map((cat) => (
            <button
              key={cat}
              type="button"
              className={`chip small ${selectedCategories.includes(cat) ? 'active' : ''}`}
              onClick={() => toggleCategory(cat)}
              disabled={loading || props.length === 0}
            >
              {cat}
            </button>
          ))}
        </div>
      )}

      {/* Team filter chips (appear once props are loaded) */}
      {!loading && teams.length > 1 && (
        <div className="categoryRow categoryRowTeams">
          {teams.map((tm) => (
            <button
              key={tm}
              type="button"
              className={`chip small ${selectedTeam === tm ? 'active' : ''}`}
              onClick={() => setSelectedTeam(tm)}
              disabled={loading || props.length === 0}
            >
              {tm}
            </button>
          ))}
        </div>
      )}

      {/* Dashboard summary stats */}
      {!loading && dashboardSummary && (
        <div className="dashboardSummary">
          <span className="dashboardSummaryStat">
            <strong>{dashboardSummary.total}</strong> props
          </span>
          <span className="dashboardSummaryDivider" />
          <span className="dashboardSummaryStat">
            <span className="dashboardSummaryLabel">Avg edge</span>
            <strong className={dashboardSummary.avgEdge >= 2 ? 'pos' : ''}>
              {dashboardSummary.avgEdge.toFixed(1)}%
            </strong>
          </span>
          <span className="dashboardSummaryDivider" />
          <span className="dashboardSummaryStat">
            <span className="dashboardSummaryLabel">High &ge;5%</span>
            <strong className="pos">{dashboardSummary.highEdgeCount}</strong>
          </span>
          {dashboardSummary.modestEdgeCount > 0 && (
            <>
              <span className="dashboardSummaryDivider" />
              <span className="dashboardSummaryStat">
                <span className="dashboardSummaryLabel">Modest &ge;2%</span>
                <strong>{dashboardSummary.modestEdgeCount}</strong>
              </span>
            </>
          )}
          {dashboardSummary.bestEdge > 0 && dashboardSummary.bestPlayer && (
            <>
              <span className="dashboardSummaryDivider" />
              <span className="dashboardSummaryStat">
                <span className="dashboardSummaryLabel">Best</span>
                <strong className="pos">{dashboardSummary.bestEdge.toFixed(1)}%</strong>
                <span className="dashboardSummarySub">{dashboardSummary.bestPlayer}</span>
              </span>
            </>
          )}
          <span className="dashboardSummaryDivider" />
          <span className="dashboardSummaryStat">
            <span className="dashboardSummaryLabel">Avg conf</span>
            <strong>{dashboardSummary.avgConf.toFixed(0)}%</strong>
          </span>
        </div>
      )}

      {loading && <p className="muted pad">Loading props\u2026</p>}
      {error && <p className="error pad">{error}</p>}

      {/* Scored props section */}
      {!loading && scoredProps.length > 0 && (
        <>
          <h3 className="sectionHeader">Top Scored Props</h3>
          <div className="marketGrid">
            {scoredProps.slice(0, 10).map((sp, i) => (
              <div key={`scored-${i}`} className="marketCard">
                <div className="marketCardTop">
                  <span className="chip small">{sp.tier}</span>
                  <span className="chip small">{sp.confidence}</span>
                </div>
                <h3>{sp.player_name} \u2014 {sp.stat_category}</h3>
                <div className="marketStats">
                  <span>Line: {sp.line}</span>
                  <span>EV: {sp.expected_value.toFixed(1)}</span>
                  <span>Edge: {formatEdge(sp.edge_score)}</span>
                  <span>Win: {formatProb(sp.win_probability)}</span>
                </div>
                <p className="muted small">{sp.recommendation}</p>
              </div>
            ))}
          </div>
        </>
      )}

      {/* All props -- filtered by stat category, grouped by game */}
      {!loading && (
        <>
          <h3 className="sectionHeader">
            {selectedCategories.length === 0 ? 'All Props' : `${selectedCategories.join(', ')} Props`}
            {selectedCategories.length > 0 && props.length > 0 && (
              <span className="muted small">
                {' '}({displayProps.length} of {props.length})
              </span>
            )}
            <span className="propsSort">
              <select
                className="propsSortSelect"
                value={sortKey}
                onChange={(e) => setSortKey(e.target.value as PropsSortKey)}
                aria-label="Sort props by"
              >
                <option value="edge">Edge</option>
                <option value="confidence">Confidence</option>
                <option value="projection">Projection</option>
                <option value="name">Name</option>
              </select>
              <button
                type="button"
                className="sortDirBtn"
                onClick={() => setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'))}
                title={sortDir === 'desc' ? 'Sort descending' : 'Sort ascending'}
                aria-label={`Sort ${sortDir === 'desc' ? 'descending' : 'ascending'}`}
              >
                {sortDir === 'desc' ? '\u2193' : '\u2191'}
              </button>
            </span>
            <span className="playerFilter">
              <input
                type="text"
                className="playerFilterInput"
                value={playerFilter}
                onChange={(e) => setPlayerFilter(e.target.value)}
                placeholder="Player\u2026"
                aria-label="Filter by player name"
              />
            </span>
            <span className="minEdgeFilter">
              <label>Min edge:</label>
              <input
                type="number"
                className="minEdgeInput"
                min="0" max="100" step="1"
                value={minEdge}
                onChange={(e) => setMinEdge(Math.max(0, Number(e.target.value) || 0))}
                aria-label="Minimum edge percentage"
              />
              <span className="muted small">%</span>
            </span>
            <button
              type="button"
              className="ghostBtn small"
              onClick={async () => {
                try {
                  const csv = await prizepicksApi.exportPropsCsv(selectedLeague === 'All' ? undefined : selectedLeague);
                  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
                  const link = document.createElement('a');
                  const url = URL.createObjectURL(blob);
                  link.setAttribute('href', url);
                  const now = new Date();
                  const dateStr = now.toISOString().split('T')[0];
                  link.setAttribute('download', `props-${dateStr}.csv`);
                  link.style.visibility = 'hidden';
                  document.body.appendChild(link);
                  link.click();
                  document.body.removeChild(link);
                  URL.revokeObjectURL(url);
                } catch (e) {
                  console.error('[PrizePicks] Failed to export props CSV:', e);
                }
              }}
              title="Export visible props to CSV"
              aria-label="Export visible props to CSV"
            >
              \uD83D\uDCE5 CSV
            </button>
            {hasActiveFilters && (
              <button
                type="button"
                className="resetFiltersBtn"
                onClick={resetFilters}
                title="Reset all filters to defaults"
                aria-label="Reset all filters"
              >
                \u21BA Reset
              </button>
            )}
          </h3>
          {groupedGames.length === 0 ? (
            <p className="muted pad">
              {props.length === 0
                ? 'No props found.'
                : showWatchlist
                  ? 'No bookmarked props found. Click the \u2606 star on a prop card to add it to your watchlist.'
                  : playerFilter
                    ? `No props match "${playerFilter}". Try a different name.`
                    : minEdge > 0
                      ? `No props meet the minimum edge requirement (\u2265${minEdge}%). Try lowering the threshold.`
                    : selectedTeam !== 'All'
                      ? `No ${selectedCategories.length > 0 ? selectedCategories.join(', ') + ' ' : ''}props for ${selectedTeam} match the current filters.`
                      : selectedCategories.length > 0
                        ? `No ${selectedCategories.join(', ')} props match the current filters.`
                        : 'No props match the current filters.'}
            </p>
          ) : (
            groupedGames.map(([game, gameProps]) => (
              <div key={game} className="gameGroup">
                <div
                  className="gameGroupHeader"
                  onClick={() => toggleGameGroup(game)}
                  aria-expanded={!collapsedGames[game]}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleGameGroup(game); } }}
                >
                  <span className={`gameGroupCollapseArrow${collapsedGames[game] ? ' collapsed' : ''}`}>
                    \u25B6
                  </span>
                  <span className="gameGroupTitle">{game}</span>
                  <span className="chip small gameGroupCount">{gameProps.length}</span>
                  {gameProps.length > 0 && (() => {
                    const e = gameProps.map(p => p.edge_pct).filter(x => x != null);
                    if (e.length === 0) return null;
                    const avg = e.reduce((a, b) => a + b, 0) / e.length;
                    const hc = e.filter(x => x >= 5).length;
                    return (
                      <span className="gameGroupEdge small muted" title={`Avg edge: ${avg >= 0 ? '+' : ''}${avg.toFixed(1)}% \u00B7 ${hc} prop${hc === 1 ? '' : 's'} with edge \u22655%`}>
                        avg <span className={avg >= 2 ? 'pos' : ''}>{avg >= 0 ? '+' : ''}{avg.toFixed(1)}%</span>
                        {hc > 0 && <> \u00B7 {hc}\u22655%</>}
                      </span>
                    );
                  })()}
                  {gameProps[0]?.game_time && (
                    <span className="gameGroupTime muted small">
                      {(() => {
                        const gt = gameProps[0].game_time!;
                        const abs = new Date(gt).toLocaleString(undefined, {
                          weekday: 'short', month: 'short', day: 'numeric',
                          hour: 'numeric', minute: '2-digit',
                        });
                        const rel = gameTimeRelative(gt);
                        return <>{abs} <span className="gameTimeRel muted">{rel}</span></>;
                      })()}
                    </span>
                  )}
                  {collapsedGames[game] && (
                    <span className="gameGroupHidden muted small">{gameProps.length} props hidden</span>
                  )}
                </div>
                {!collapsedGames[game] && (
                  <div className="marketGrid">
                    {gameProps.map((prop) => (
                    <div key={prop.id} className={`marketCard ${edgeLevelClass(prop.edge_pct)}`}>
                      <div className="marketCardTop">
                        <button
                          type="button"
                          className={`watchlistStar ${watchlist.includes(prop.id) ? 'active' : ''}`}
                          onClick={() => toggleWatchlistProp(prop.id)}
                          title={watchlist.includes(prop.id) ? 'Remove from watchlist' : 'Add to watchlist'}
                          aria-label={watchlist.includes(prop.id) ? 'Remove from watchlist' : 'Add to watchlist'}
                        >
                          {watchlist.includes(prop.id) ? '\u2B50' : '\u2606'}
                        </button>
                        <code>{prop.player}</code>
                        <span className={`riskBadge risk${prop.risk.charAt(0).toUpperCase() + prop.risk.slice(1)}`}>
                          {prop.risk}
                        </span>
                        <span className="chip small">{prop.league}</span>
                        <button
                          type="button"
                          className="copyPropBtn"
                          onClick={() => copyPropToClipboard(prop)}
                          title="Copy prop details"
                          aria-label="Copy prop details"
                        >
                          \uD83D\uDCCB
                        </button>
                      </div>
                      <h3>{prop.prop_type}</h3>
                      <div className="marketCardMeta">
                        {prop.team && <span className="teamTag">{prop.team}</span>}
                        {prop.game && <span className="muted">{prop.game}</span>}
                      </div>
                      <div className="marketStats">
                        <span>Line: {prop.line}</span>
                        <span>Proj: {prop.projection.toFixed(1)}</span>
                        <span>Edge: {formatEdge(prop.edge_pct)}</span>
                        <span>Conf: {prop.confidence}%</span>
                      </div>
                      <p className="small">{prop.recommendation}</p>
                    </div>
                  ))}
                </div>
              )}
            </div>
            ))
          )}
        </>
      )}

      {!loading && groupedGames.length === 0 && displayProps.length === 0 && !error && (
        <p className="muted pad">
          {props.length === 0
            ? 'No props found.'
            : showWatchlist
              ? 'No bookmarked props found. Click the \u2606 star on a prop card to add it to your watchlist.'
              : playerFilter
                ? `No props match "${playerFilter}". Try a different name.`
                : minEdge > 0
                  ? `No props meet the minimum edge requirement (\u2265${minEdge}%). Try lowering the threshold.`
                : selectedTeam !== 'All'
                  ? `No ${selectedCategories.length > 0 ? selectedCategories.join(', ') + ' ' : ''}props for ${selectedTeam} match the current filters.`
                  : selectedCategories.length > 0
                    ? `No ${selectedCategories.join(', ')} props match the current filters.`
                    : 'No props match the current filters.'}
        </p>
      )}
    </div>
  );
}
