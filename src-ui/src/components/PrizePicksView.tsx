import { useState, useEffect, useCallback, useRef, useMemo } from 'react';

const COLLAPSED_STORAGE_KEY = 'prizepicks_collapsed_games';
const PREFERENCES_STORAGE_KEY = 'prizepicks_dashboard_preferences';
const WATCHLIST_STORAGE_KEY = 'prizepicks_watchlist';

interface DashboardPreferences {
  sortKey: 'name' | 'edge' | 'confidence' | 'projection';
  sortDir: 'asc' | 'desc';
  minEdge: number;
  minConfidence: number;
  selectedCategories: string[];
  selectedTeams: string[];
  selectedRisk: string;
  playerFilter: string;
}

const DEFAULT_PREFERENCES: DashboardPreferences = {
  sortKey: 'edge',
  sortDir: 'desc',
  minEdge: 0,
  minConfidence: 0,
  selectedCategories: [],
  selectedTeams: [],
  selectedRisk: 'All',
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
    // Handle legacy single-team format: if selectedTeam (string) exists,
    // migrate to selectedTeams array.
    if (typeof parsed.selectedTeam === 'string' && !Array.isArray(parsed.selectedTeams)) {
      parsed.selectedTeams = parsed.selectedTeam === 'All' ? [] : [parsed.selectedTeam];
    }
    delete parsed.selectedTeam; // clean up legacy key
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
type PropsSortKey = 'name' | 'edge' | 'confidence' | 'projection';

/** A saved dashboard filter preset — captures the user's filter configuration. */
interface FilterPreset {
  name: string;
  selectedCategories: string[];
  selectedTeams: string[];
  selectedRisk: string;
  sortKey: PropsSortKey;
  sortDir: 'asc' | 'desc';
  minEdge: number;
  minConfidence: number;
  playerFilter: string;
  showWatchlist: boolean;
}

const FILTER_PRESETS_KEY = 'prizepicks_filter_presets';

function loadFilterPresets(): FilterPreset[] {
  try {
    const raw = localStorage.getItem(FILTER_PRESETS_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveFilterPresets(presets: FilterPreset[]) {
  try {
    localStorage.setItem(FILTER_PRESETS_KEY, JSON.stringify(presets));
  } catch {
    // silently ignore
  }
}

/** Generate a short human-readable summary of a preset's filter configuration. */
function describePreset(preset: FilterPreset): string {
  const parts: string[] = [];
  if (preset.selectedCategories.length > 0) {
    parts.push(preset.selectedCategories.join(',+'));
  }
  if (preset.selectedTeams.length > 0) {
    parts.push(preset.selectedTeams.join(',+'));
  }
  if (preset.minEdge > 0) {
    parts.push(`≥${preset.minEdge}% edge`);
  }
  if (preset.minConfidence > 0) {
    parts.push(`≥${preset.minConfidence}% conf`);
  }
  if (preset.selectedRisk !== 'All') {
    parts.push(preset.selectedRisk);
  }
  if (preset.showWatchlist) {
    parts.push('watchlist');
  }
  if (preset.sortKey !== 'edge' || preset.sortDir !== 'desc') {
    parts.push(`sort:${preset.sortKey} ${preset.sortDir}`);
  }
  return parts.join(' · ') || 'default sort';
}

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

/** Generate CSV string from an array of visible (filtered/sorted) props. */
function generatePropsCsv(props: PropPick[]): string {
  const esc = (v: string | number | null | undefined): string => {
    const s = v == null ? '' : String(v);
    if (s.includes(',') || s.includes('"') || s.includes('\n')) {
      return `"${s.replace(/"/g, '""')}"`;
    }
    return s;
  };
  const header = 'Player,Team,League,Category,Line,Projection,Edge %,Confidence %,Risk,Game,Recommendation';
  const rows = props.map((p) =>
    [p.player, esc(p.team), p.league, p.prop_type, esc(p.line), esc(p.projection?.toFixed(1) ?? ''),
     esc(p.edge_pct != null ? p.edge_pct.toFixed(1) : ''), esc(p.confidence ?? ''), esc(p.risk ?? ''),
     esc(p.game ?? ''), esc(p.recommendation ?? '')].join(',')
  );
  return [header, ...rows].join('\n');
}

export function PrizePicksView() {
  // Load preferences from localStorage on mount
  const savedPreferences = loadPreferences();

  const [props, setProps] = useState<PropPick[]>([]);
  const [scoredProps, setScoredProps] = useState<ScoredProp[]>([]);
  const [selectedLeague, setSelectedLeague] = useState('All');
  const [selectedCategories, setSelectedCategories] = useState<string[]>(savedPreferences.selectedCategories);
  const [selectedTeams, setSelectedTeams] = useState<string[]>(savedPreferences.selectedTeams ?? []);
  const [searchQuery, setSearchQuery] = useState('');
  const [playerFilter, setPlayerFilter] = useState(savedPreferences.playerFilter);
  const [sortKey, setSortKey] = useState<PropsSortKey>(savedPreferences.sortKey);
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>(savedPreferences.sortDir);
  const [minEdge, setMinEdge] = useState(savedPreferences.minEdge);
  const [minConfidence, setMinConfidence] = useState(savedPreferences.minConfidence);
  const [selectedRisk, setSelectedRisk] = useState(savedPreferences.selectedRisk);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cacheStatus, setCacheStatus] = useState<PrizePicksCacheStatus | null>(null);
  const [collapsedGames, setCollapsedGames] = useState<Record<string, boolean>>(loadCollapsed);
  const [watchlist, setWatchlist] = useState<string[]>(loadWatchlist);
  const [showWatchlist, setShowWatchlist] = useState(false);
  // Filter presets state
  const [presets, setPresets] = useState<FilterPreset[]>(loadFilterPresets);
  const [savingPreset, setSavingPreset] = useState(false);
  const [editingPresetName, setEditingPresetName] = useState('');
  const [expandedPropId, setExpandedPropId] = useState<string | null>(null);
  // True when any filter control is set to a non-default value
  const hasActiveFilters = sortKey !== DEFAULT_PREFERENCES.sortKey || sortDir !== DEFAULT_PREFERENCES.sortDir || minEdge > 0 || minConfidence > 0 || selectedCategories.length > 0 || selectedTeams.length > 0 || selectedRisk !== 'All' || playerFilter !== '' || showWatchlist;

  const resetFilters = () => {
    setSortKey(DEFAULT_PREFERENCES.sortKey);
    setSortDir(DEFAULT_PREFERENCES.sortDir);
    setMinEdge(0);
    setMinConfidence(0);
    setSelectedCategories([]);
    setSelectedTeams([]);
    setSelectedRisk('All');
    setPlayerFilter('');
    setShowWatchlist(false);
  };

  // ── Filter preset handlers ──

  /** Capture current filter state and save as a new preset. */
  const saveCurrentAsPreset = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    setPresets((prev) => {
      // Replace existing preset with same name, or append
      const filtered = prev.filter((p) => p.name !== trimmed);
      const next: FilterPreset[] = [
        ...filtered,
        {
          name: trimmed,
          selectedCategories,
          selectedTeams,
          selectedRisk,
          sortKey,
          sortDir,
          minEdge,
          minConfidence,
          playerFilter,
          showWatchlist,
        },
      ];
      saveFilterPresets(next);
      return next;
    });
    setEditingPresetName('');
    setSavingPreset(false);
  };

  /** Restore all filter state from a saved preset. */
  const applyPreset = (preset: FilterPreset) => {
    setSelectedCategories(preset.selectedCategories);
    setSelectedTeams(preset.selectedTeams ?? []);
    setSelectedRisk(preset.selectedRisk);
    setSortKey(preset.sortKey);
    setSortDir(preset.sortDir);
    setMinEdge(preset.minEdge);
    setMinConfidence(preset.minConfidence ?? 0);
    setPlayerFilter(preset.playerFilter);
    setShowWatchlist(preset.showWatchlist);
    setEditingPresetName('');
    setSavingPreset(false);
  };

  /** Delete a preset by name. */
  const deletePreset = (name: string) => {
    setPresets((prev) => {
      const next = prev.filter((p) => p.name !== name);
      saveFilterPresets(next);
      return next;
    });
  };

  /** Determine which preset (if any) matches the current filter state. */
  const activePresetName = useMemo(() => {
    for (const p of presets) {
      if (
        p.sortKey === sortKey &&
        p.sortDir === sortDir &&
        p.minEdge === minEdge &&
        p.minConfidence === minConfidence &&
        p.showWatchlist === showWatchlist &&
        p.playerFilter === playerFilter &&
        p.selectedTeams.length === selectedTeams.length &&
        p.selectedTeams.every((t) => selectedTeams.includes(t)) &&
        p.selectedRisk === selectedRisk &&
        p.selectedCategories.length === selectedCategories.length &&
        p.selectedCategories.every((c) => selectedCategories.includes(c))
      ) {
        return p.name;
      }
    }
    return null;
  }, [presets, sortKey, sortDir, minEdge, minConfidence, showWatchlist, playerFilter, selectedTeams, selectedCategories, selectedRisk]);

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
      minConfidence,
      selectedCategories,
      selectedTeams,
      selectedRisk,
      playerFilter,
    });
  }, [sortKey, sortDir, minEdge, minConfidence, selectedCategories, selectedTeams, selectedRisk, playerFilter]);

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
    setSelectedTeams([]);
    setSelectedRisk('All');
    setPlayerFilter('');
    setShowWatchlist(false);
  }, [props]);

  // Compute unique stat categories from the loaded props
  const categories = useMemo(() => {
    const cats = new Set(props.map((p) => p.prop_type).filter(Boolean));
    return Array.from(cats).sort();
  }, [props]);

  // Compute per-category prop counts for filter chip badges
  const categoryCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const p of props) {
      if (p.prop_type) {
        counts[p.prop_type] = (counts[p.prop_type] || 0) + 1;
      }
    }
    return counts;
  }, [props]);

  // Compute unique team abbreviations from the loaded props
  const teams = useMemo(() => {
    const tm = new Set(props.map((p) => p.team).filter(Boolean));
    return ['All', ...Array.from(tm).sort()];
  }, [props]);

  // Compute per-team prop counts for filter chip badges
  const teamCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const p of props) {
      if (p.team) {
        counts[p.team] = (counts[p.team] || 0) + 1;
      }
    }
    return counts;
  }, [props]);

  // Compute unique risk levels from the loaded props
  const riskLevels = useMemo(() => {
    const rl = new Set(props.map((p) => p.risk).filter(Boolean));
    // Always show standard risk levels in order
    const standard = ['All', 'low', 'medium', 'high'];
    const present = new Set(rl);
    return standard.filter((x) => x === 'All' || present.has(x));
  }, [props]);

  // Compute per-risk-level prop counts for filter chip badges
  const riskCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const p of props) {
      if (p.risk) {
        counts[p.risk] = (counts[p.risk] || 0) + 1;
      }
    }
    return counts;
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
    if (selectedTeams.length > 0) {
      filtered = filtered.filter((p) => p.team && selectedTeams.includes(p.team));
    }
    if (playerFilter) {
      const q = playerFilter.toLowerCase();
      filtered = filtered.filter((p) => p.player.toLowerCase().includes(q));
    }
    if (minEdge > 0) {
      filtered = filtered.filter((p) => (p.edge_pct ?? 0) >= minEdge);
    }
    if (minConfidence > 0) {
      filtered = filtered.filter((p) => (p.confidence ?? 0) >= minConfidence);
    }
    if (selectedRisk !== 'All') {
      filtered = filtered.filter((p) => p.risk === selectedRisk);
    }
    if (showWatchlist) {
      filtered = filtered.filter((p) => watchlist.includes(p.id));
    }
    return filtered;
  }, [props, selectedCategories, selectedTeams, playerFilter, minEdge, minConfidence, selectedRisk, showWatchlist, watchlist]);

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

  // Edge distribution — count props by edge tier for the distribution bar
  const edgeDistribution = useMemo(() => {
    let high = 0, good = 0, modest = 0, neutral = 0, poor = 0;
    for (const p of displayProps) {
      const e = p.edge_pct;
      if (e == null || !Number.isFinite(e)) { neutral++; continue; }
      if (e >= 10) high++;
      else if (e >= 5) good++;
      else if (e >= 2) modest++;
      else if (e <= -2) poor++;
      else neutral++;
    }
    const total = high + good + modest + neutral + poor;
    return { high, good, modest, neutral, poor, total };
  }, [displayProps]);

  // Top picks — auto-select the highest-edge props that meet minimum quality
  const topPicks = useMemo(() => {
    const candidates = displayProps.filter(
      (p) => (p.edge_pct ?? 0) >= 2 && (p.confidence ?? 0) >= 50
    );
    if (candidates.length === 0) return null;
    const sorted = [...candidates].sort((a, b) => {
      const edgeCmp = (b.edge_pct ?? 0) - (a.edge_pct ?? 0);
      if (edgeCmp !== 0) return edgeCmp;
      return (b.confidence ?? 0) - (a.confidence ?? 0);
    });
    return sorted.slice(0, 5);
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
              {categoryCounts[cat] !== undefined && !loading && (
                <span className="categoryCountBadge">{categoryCounts[cat]}</span>
              )}
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
              className={`chip small ${tm === 'All' ? (selectedTeams.length === 0 ? 'active' : '') : selectedTeams.includes(tm) ? 'active' : ''}`}
              onClick={() => {
                if (tm === 'All') {
                  setSelectedTeams([]);
                } else {
                  setSelectedTeams((prev) =>
                    prev.includes(tm) ? prev.filter((t) => t !== tm) : [...prev, tm]
                  );
                }
              }}
              disabled={loading || props.length === 0}
            >
              {tm}
              {tm !== 'All' && teamCounts[tm] !== undefined && !loading && (
                <span className="teamCountBadge">{teamCounts[tm]}</span>
              )}
            </button>
          ))}
        </div>
      )}

      {/* Risk level filter chips */}
      {!loading && riskLevels.length > 1 && (
        <div className="categoryRow categoryRowRisk">
          {riskLevels.map((rl) => (
            <button
              key={rl}
              type="button"
              className={`chip small riskChip ${selectedRisk === rl ? 'active' : ''}`}
              onClick={() => setSelectedRisk(rl)}
              disabled={loading || props.length === 0}
              title={rl === 'All' ? 'Show all risk levels' : `Show only ${rl}-risk props`}
              aria-label={rl === 'All' ? 'Show all risk levels' : `Filter to ${rl}-risk props`}
            >
              {rl === 'All' ? 'All' : rl.charAt(0).toUpperCase() + rl.slice(1)}
              {rl !== 'All' && riskCounts[rl] !== undefined && !loading && (
                <span className="riskCountBadge">{riskCounts[rl]}</span>
              )}
            </button>
          ))}
        </div>
      )}

      {/* Filter presets row — saved views that restore filter state */}
      {!loading && props.length > 0 && presets.length > 0 && (
        <div className="presetsRow">
          {presets.map((p) => (
            <span key={p.name} className="presetChipGroup">
              <button
                type="button"
                className={`chip small presetChip ${activePresetName === p.name ? 'active' : ''}`}
                onClick={() => applyPreset(p)}
                title={`Restore filters: ${describePreset(p)}`}
                aria-label={`Apply preset "${p.name}"`}
              >
                📍 {p.name}
              </button>
              <button
                type="button"
                className="presetDeleteBtn"
                onClick={() => deletePreset(p.name)}
                title={`Delete preset "${p.name}"`}
                aria-label={`Delete preset "${p.name}"`}
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}

      {/* Save new preset inline UI */}
      {!loading && props.length > 0 && savingPreset && (
        <div className="presetsRow">
          <input
            type="text"
            className="presetNameInput"
            value={editingPresetName}
            onChange={(e) => setEditingPresetName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') saveCurrentAsPreset(editingPresetName);
              if (e.key === 'Escape') { setSavingPreset(false); setEditingPresetName(''); }
            }}
            placeholder="View name…"
            autoFocus
            aria-label="Enter a name for this filter preset"
          />
          <button
            type="button"
            className="ghostBtn small"
            onClick={() => saveCurrentAsPreset(editingPresetName)}
            disabled={!editingPresetName.trim()}
            title="Save current filters as a preset"
          >
            Save
          </button>
          <button
            type="button"
            className="ghostBtn small"
            onClick={() => { setSavingPreset(false); setEditingPresetName(''); }}
            title="Cancel"
          >
            Cancel
          </button>
        </div>
      )}

      {/* Save button (only shown when there are active filters to save) */}
      {!loading && props.length > 0 && !savingPreset && (
        <div className="presetsRow">
          <button
            type="button"
            className="ghostBtn small"
            onClick={() => setSavingPreset(true)}
            disabled={!hasActiveFilters}
            title="Save current filter configuration as a named preset"
            aria-label="Save current filters"
          >
            💾 Save view
          </button>
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

      {/* Edge distribution bar — visual breakdown of props by edge tier */}
      {!loading && edgeDistribution.total > 0 && (
        <div className="edgeDistribution">
          <span className="edgeDistLabel">Edge distribution</span>
          <div className="edgeDistBar" title="Proportional breakdown of edge tiers across visible props">
            {edgeDistribution.high > 0 && (
              <div className="edgeDistSeg edgeDistHigh" style={{width: `${(edgeDistribution.high / edgeDistribution.total) * 100}%`}} title={`High (≥10%): ${edgeDistribution.high} props`} />
            )}
            {edgeDistribution.good > 0 && (
              <div className="edgeDistSeg edgeDistGood" style={{width: `${(edgeDistribution.good / edgeDistribution.total) * 100}%`}} title={`Good (≥5%): ${edgeDistribution.good} props`} />
            )}
            {edgeDistribution.modest > 0 && (
              <div className="edgeDistSeg edgeDistModest" style={{width: `${(edgeDistribution.modest / edgeDistribution.total) * 100}%`}} title={`Modest (≥2%): ${edgeDistribution.modest} props`} />
            )}
            {edgeDistribution.neutral > 0 && (
              <div className="edgeDistSeg edgeDistNeutral" style={{width: `${(edgeDistribution.neutral / edgeDistribution.total) * 100}%`}} title={`Neutral: ${edgeDistribution.neutral} props`} />
            )}
            {edgeDistribution.poor > 0 && (
              <div className="edgeDistSeg edgeDistPoor" style={{width: `${(edgeDistribution.poor / edgeDistribution.total) * 100}%`}} title={`Poor (≤-2%): ${edgeDistribution.poor} props`} />
            )}
          </div>
          <div className="edgeDistLegend">
            {edgeDistribution.high > 0 && <span className="edgeDistLegendItem"><i className="edgeDistDot edgeDistHigh" />{edgeDistribution.high} high</span>}
            {edgeDistribution.good > 0 && <span className="edgeDistLegendItem"><i className="edgeDistDot edgeDistGood" />{edgeDistribution.good} good</span>}
            {edgeDistribution.modest > 0 && <span className="edgeDistLegendItem"><i className="edgeDistDot edgeDistModest" />{edgeDistribution.modest} mod</span>}
            {edgeDistribution.neutral > 0 && <span className="edgeDistLegendItem"><i className="edgeDistDot edgeDistNeutral" />{edgeDistribution.neutral} neut</span>}
            {edgeDistribution.poor > 0 && <span className="edgeDistLegendItem"><i className="edgeDistDot edgeDistPoor" />{edgeDistribution.poor} poor</span>}
          </div>
        </div>
      )}

      {/* Top picks — auto-select highest-edge props */}{!loading && topPicks && topPicks.length > 0 && (
        <div className="topPicksSection">
          <div className="topPicksHeader">
            <span className="topPicksIcon">🏆</span>
            <strong>Top Picks</strong>
            <span className="muted small topPicksSub"> — Best edge props with ≥2% edge and ≥50% confidence</span>
          </div>
          <div className="topPicksGrid">
            {topPicks.map((prop) => (
              <div key={prop.id} className={`topPickCard ${edgeLevelClass(prop.edge_pct)}`}>
                <span className="topPickBadge">🏆</span>
                <div className="topPickInfo">
                  <strong className="topPickPlayer">{prop.player}</strong>
                  <span className="muted small">{prop.prop_type}</span>
                  <span className="chip small">{prop.league}</span>
                </div>
                <div className="topPickStats">
                  <span className="topPickStat">
                    <strong className={prop.edge_pct != null && prop.edge_pct >= 2 ? 'pos' : ''}>
                      {formatEdge(prop.edge_pct)}
                    </strong>
                    {' '}edge
                  </span>
                  <span className="topPickStat">{prop.confidence}% conf</span>
                </div>
              </div>
            ))}
          </div>
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
              {[2, 5, 10].map((v) => (
                <button
                  key={v}
                  type="button"
                  className={`chip mini ${minEdge === v ? 'active' : ''}`}
                  onClick={() => setMinEdge(v)}
                  title={`Show only props with edge ≥${v}%`}
                  aria-label={`Filter to edge ≥${v}%`}
                >
                  ≥{v}%
                </button>
              ))}
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
            <span className="minConfidenceFilter">
              <label>Min conf:</label>
              {[60, 70, 80].map((v) => (
                <button
                  key={v}
                  type="button"
                  className={`chip mini ${minConfidence === v ? 'active' : ''}`}
                  onClick={() => setMinConfidence(v)}
                  title={`Show only props with confidence ≥${v}%`}
                  aria-label={`Filter to confidence ≥${v}%`}
                >
                  ≥{v}%
                </button>
              ))}
              <input
                type="number"
                className="minConfidenceInput"
                min="0" max="100" step="5"
                value={minConfidence}
                onChange={(e) => setMinConfidence(Math.max(0, Number(e.target.value) || 0))}
                aria-label="Minimum confidence percentage"
              />
              <span className="muted small">%</span>
            </span>
            <button
              type="button"
              className="ghostBtn small"
              onClick={() => {
                try {
                  const csv = generatePropsCsv(sortedProps);
                  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
                  const link = document.createElement('a');
                  const url = URL.createObjectURL(blob);
                  link.setAttribute('href', url);
                  const now = new Date();
                  const dateStr = now.toISOString().split('T')[0];
                  link.setAttribute('download', `props-filtered-${dateStr}.csv`);
                  link.style.visibility = 'hidden';
                  document.body.appendChild(link);
                  link.click();
                  document.body.removeChild(link);
                  URL.revokeObjectURL(url);
                } catch (e) {
                  console.error('[PrizePicks] Failed to export props CSV:', e);
                }
              }}
              title="Export currently visible (filtered/sorted) props to CSV"
              aria-label="Export currently visible props to CSV"
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
                    : minConfidence > 0
                      ? `No props meet the minimum confidence requirement (\u2265${minConfidence}%). Try lowering the threshold.`
                    : selectedRisk !== 'All'
                      ? `No ${selectedRisk}-risk props match the current filters. Try a different risk level.`
                    : minEdge > 0
                      ? `No props meet the minimum edge requirement (\u2265${minEdge}%). Try lowering the threshold.`
                    : selectedTeams.length > 0
                      ? `No ${selectedCategories.length > 0 ? selectedCategories.join(', ') + ' ' : ''}props for ${selectedTeams.join(', ')} match the current filters.`
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
                          📋
                        </button>
                        <button
                          type="button"
                          className={`insightBtn${expandedPropId === prop.id ? ' active' : ''}`}
                          onClick={() => setExpandedPropId(expandedPropId === prop.id ? null : prop.id)}
                          title="Show prop insight details"
                          aria-label="Show prop insight details"
                        >
                          🔍
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
                      {expandedPropId === prop.id && (
                        <div className="propInsight">
                          {prop.reasoning && (
                            <div className="propInsightSection">
                              <span className="propInsightLabel">Reasoning</span>
                              <p className="propInsightText">{prop.reasoning}</p>
                            </div>
                          )}
                          <div className="propInsightRow">
                            <div className="propInsightSection">
                              <span className="propInsightLabel">Model probability</span>
                              <strong className="pos">{prop.model_probability != null ? (prop.model_probability * 100).toFixed(1) + '%' : '—'}</strong>
                            </div>
                            <div className="propInsightSection">
                              <span className="propInsightLabel">Market probability</span>
                              <strong>{prop.implied_probability != null ? (prop.implied_probability * 100).toFixed(1) + '%' : '—'}</strong>
                            </div>
                          </div>
                          <div className="propInsightRow">
                            <div className="propInsightSection">
                              <span className="propInsightLabel">Source</span>
                              <span>{prop.source || '—'}</span>
                            </div>
                            {prop.updated_at && (
                              <div className="propInsightSection">
                                <span className="propInsightLabel">Updated</span>
                                <span>{new Date(prop.updated_at).toLocaleString()}</span>
                              </div>
                            )}
                          </div>
                        </div>
                      )}
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
              : minConfidence > 0
                ? `No props meet the minimum confidence requirement (\u2265${minConfidence}%). Try lowering the threshold.`
              : selectedRisk !== 'All'
                ? `No ${selectedRisk}-risk props match the current filters. Try a different risk level.`
              : minEdge > 0
                ? `No props meet the minimum edge requirement (\u2265${minEdge}%). Try lowering the threshold.`
              : selectedTeams.length > 0
                  ? `No ${selectedCategories.length > 0 ? selectedCategories.join(', ') + ' ' : ''}props for ${selectedTeams.join(', ')} match the current filters.`
                  : selectedCategories.length > 0
                    ? `No ${selectedCategories.join(', ')} props match the current filters.`
                    : 'No props match the current filters.'}
        </p>
      )}
    </div>
  );
}
