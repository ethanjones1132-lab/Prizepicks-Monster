import { useState, useEffect, useCallback, useRef } from 'react';
import { prizepicksApi } from '../services/prizepicks';
import type { PropPick, ScoredProp } from '../types';

const INITIAL_PROP_LIMIT = 50;

function formatEdge(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '—';
}

function formatProb(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '—';
}

export function PrizePicksView() {
  const [props, setProps] = useState<PropPick[]>([]);
  const [scoredProps, setScoredProps] = useState<ScoredProp[]>([]);
  const [selectedLeague, setSelectedLeague] = useState('All');
  const [searchQuery, setSearchQuery] = useState('');
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const leagues = ['All', 'NFL', 'NBA', 'MLB', 'NHL'];

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

  useEffect(() => {
    void loadProps({ league: selectedLeague });
    void loadScored();
  }, [selectedLeague, loadProps, loadScored]);

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

  return (
    <div className="prizepicksPage">
      <header className="prizepicksHeader">
        <div>
          <h2>Player Props</h2>
          <p className="muted">DFS prop board with edge analysis and projections</p>
        </div>
        <button type="button" className="primaryBtn" onClick={() => void refreshAll()} disabled={refreshing || loading}>
          {refreshing ? 'Refreshing…' : 'Refresh props'}
        </button>
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

      {/* All props */}
      {!loading && (
        <>
          <h3 className="sectionHeader">All Props</h3>
          <div className="marketGrid">
            {props.map((prop) => (
              <div key={prop.id} className="marketCard">
                <div className="marketCardTop">
                  <code>{prop.player}</code>
                  <span className="chip small">{prop.league}</span>
                </div>
                <h3>{prop.player} — {prop.prop_type}</h3>
                <div className="marketStats">
                  <span>Line: {prop.line}</span>
                  <span>Proj: {prop.projection.toFixed(1)}</span>
                  <span>Edge: {formatEdge(prop.edge_pct)}</span>
                  <span>Conf: {prop.confidence}%</span>
                </div>
                <p className="muted small">{prop.recommendation}</p>
              </div>
            ))}
          </div>
        </>
      )}

      {!loading && props.length === 0 && !error && (
        <p className="muted pad">No props found.</p>
      )}
    </div>
  );
}
