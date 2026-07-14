import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
const COLLAPSED_STORAGE_KEY = 'prizepicks_collapsed_games';

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
import { prizepicksApi } from '../services/prizepicks';
import type { PrizePicksCacheStatus } from '../types/prizepicks';
import type { PropPick, ScoredProp } from '../types';

const INITIAL_PROP_LIMIT = 50;

function formatEdge(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '—';
}

function formatProb(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '—';
}

function edgeLevelClass(edge: number | undefined | null): string {
  if (edge == null || !Number.isFinite(edge)) return '';
  if (edge >= 10) return 'edge-high';
  if (edge >= 5) return 'edge-good';
  if (edge >= 2) return 'edge-modest';
  if (edge <= -2) return 'edge-poor';
  return '';
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

export function PrizePicksView() {
  const [props, setProps] = useState<PropPick[]>([]);
  const [scoredProps, setScoredProps] = useState<ScoredProp[]>([]);
  const [selectedLeague, setSelectedLeague] = useState('All');
  const [selectedCategory, setSelectedCategory] = useState('All');
  const [selectedTeam, setSelectedTeam] = useState('All');
  const [searchQuery, setSearchQuery] = useState('');
  type PropsSortKey = 'name' | 'edge' | 'confidence' | 'projection';
  const [sortKey, setSortKey] = useState<PropsSortKey>('edge');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('desc');
  const [minEdge, setMinEdge] = useState(0);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cacheStatus, setCacheStatus] = useState<PrizePicksCacheStatus | null>(null);
  const [collapsedGames, setCollapsedGames] = useState<Record<string, boolean>>(loadCollapsed);
  const requestId = useRef(0);

  const toggleGameGroup = (key: string) => {
    setCollapsedGames((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      saveCollapsed(next);
      return next;
    });
  };

  const leagues = ['All', 'NFL', 'NBA', 'MLB', 'NHL'];

  // Derive the active data source label from current props
  // (all props from a single fetch share the same source)
  const dataSource = useMemo(() => {
    const sources = new Set(props.map((p) => p.source).filter(Boolean));
    if (sources.size === 0) return null;
    if (sources.size === 1) {
      const s = sources.values().next().value!;
      const labels: Record<string, string> = {
        opticodds: '🔮 OpticOdds',
        'the-odds-api': '📊 The Odds API',
        espn: '📺 ESPN',
        sleeper: '😴 Sleeper',
        mock: '🧪 Mock',
      };
      return labels[s] ?? s;
    }
    return '🔄 Multi-source';
  }, [props]);

  // Reset category and team filters when props are reloaded (e.g. league change)
  useEffect(() => {
    setSelectedCategory('All');
    setSelectedTeam('All');
  }, [props]);

  // Compute unique stat categories from the loaded props
  const categories = useMemo(() => {
    const cats = new Set(props.map((p) => p.prop_type).filter(Boolean));
    return ['All', ...Array.from(cats).sort()];
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
   * Initial-mount loader — uses the single-call `getDashboardBootstrap`
   * endpoint so the top-props, scored-props, and cache-status slices
   * arrive in one IPC round-trip instead of three. Subsequent filter
   * changes (league / search) and refreshes still use the granular
   * commands because they touch only one slice.
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
    // Initial mount: single-call bootstrap. The granular commands
    // remain available for league / search / refresh.
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

  // Client-side filter by stat category, team, and minimum edge
  const displayProps = useMemo(() => {
    let filtered = selectedCategory === 'All'
      ? props
      : props.filter((p) => p.prop_type === selectedCategory);
    if (selectedTeam !== 'All') {
      filtered = filtered.filter((p) => p.team === selectedTeam);
    }
    if (minEdge > 0) {
      filtered = filtered.filter((p) => (p.edge_pct ?? 0) >= minEdge);
    }
    return filtered;
  }, [props, selectedCategory, selectedTeam, minEdge]);

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
  // Props without a game label fall into a trailing "Other" group
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
      // "Other" (no game label) sorts to the very end
      if (a[0] === 'Other') return 1;
      if (b[0] === 'Other') return -1;
      // Games with a known game_time sort chronologically
      if (ta && tb) return ta.localeCompare(tb);
      if (ta) return -1;
      if (tb) return 1;
      // Fallback: alphabetical by game label
      return a[0].localeCompare(b[0]);
    });
    return entries;
  }, [sortedProps]);

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
                  ? `${cacheStatus.markets_count} markets · ${cacheStatus.full_catalog ? 'Full catalog' : 'Partial cache (quick load)'}${cacheStatus.is_stale ? ' · stale' : ''}`
                  : 'Cache empty — awaiting first load'
              }
            >
              {cacheStatus.full_catalog
                ? `📦 ${cacheStatus.markets_count}`
                : cacheStatus.has_cache
                  ? `📦 ${cacheStatus.markets_count}*`
                  : '📦 empty'}
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
          {refreshing ? 'Refreshing…' : 'Refresh props'}
        </button>
        </div>   {/* prizepicksHeaderActions */}
      </header>

      <div className="prizepicksToolbar">
        <input
          className="searchInput"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search player or prop…"
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
          </button>
        ))}
      </div>

      {/* Stat category filter chips (appear once props are loaded) */}
      {!loading && categories.length > 1 && (
        <div className="categoryRow categoryRowCategories">
          {categories.map((cat) => (
            <button
              key={cat}
              type="button"
              className={`chip small ${selectedCategory === cat ? 'active' : ''}`}
              onClick={() => setSelectedCategory(cat)}
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

      {loading && <p className="muted pad">Loading props…</p>}
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
                <h3>{sp.player_name} — {sp.stat_category}</h3>
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

      {/* All props — filtered by stat category, grouped by game */}
      {!loading && (
        <>
          <h3 className="sectionHeader">
            {selectedCategory === 'All' ? 'All Props' : `${selectedCategory} Props`}
            {selectedCategory !== 'All' && props.length > 0 && (
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
                {sortDir === 'desc' ? '↓' : '↑'}
              </button>
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
              📥 CSV
            </button>
          </h3>
          {groupedGames.length === 0 ? (
            <p className="muted pad">
              {props.length === 0
                ? 'No props found.'
                : minEdge > 0
                  ? `No props meet the minimum edge requirement (≥${minEdge}%). Try lowering the threshold.`
                  : selectedTeam !== 'All'
                    ? `No ${selectedCategory} props for ${selectedTeam} match the current filters.`
                    : `No ${selectedCategory} props match the current filters.`}
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
                    ▶
                  </span>
                  <span className="gameGroupTitle">{game}</span>
                  <span className="chip small gameGroupCount">{gameProps.length}</span>
                  {gameProps[0]?.game_time && (
                    <span className="gameGroupTime muted small">
                      {new Date(gameProps[0].game_time).toLocaleString(undefined, {
                        weekday: 'short', month: 'short', day: 'numeric',
                        hour: 'numeric', minute: '2-digit',
                      })}
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
                        <code>{prop.player}</code>
                        <span className="chip small">{prop.league}</span>
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
            : minEdge > 0
              ? `No props meet the minimum edge requirement (≥${minEdge}%). Try lowering the threshold.`
              : selectedTeam !== 'All'
                ? `No ${selectedCategory} props for ${selectedTeam} match the current filters.`
                : `No ${selectedCategory} props match the current filters.`}
        </p>
      )}
    </div>
  );
}
