import { useCallback, useEffect, useMemo, useState } from 'react';
import { prizepicksApi } from '../services/prizepicks';
import type {
  PaperAnalytics,
  PaperCategoryStats,
  PaperEquitySnapshot,
  PaperSideStats,
  PrizePicksPrediction,
} from '../types/prizepicks';
import { paperSideLabel } from '../types/prizepicks';
import { prizepicksBetWon } from '../services/prizepicks';

/**
 * Render the current streak as a short chip. Wins use the `pos` tint, losses
 * use the `neg` tint, and an empty streak (no closed lots) renders muted `—`.
 * `length === 0` is treated as "no streak" regardless of `kind`, so a stale
 * `kind: "None"` with `length: 0` and a never-been-used account both look the
 * same to the user.
 */
function StreakChip({ analytics }: { analytics: PaperAnalytics }) {
  const { kind, length } = analytics.current_streak;
  if (length === 0 || kind === 'None') {
    return (
      <span className="streakChip muted" aria-label="No current streak">
        —
      </span>
    );
  }
  const tint = kind === 'W' ? 'pos' : 'neg';
  const label = kind === 'W' ? 'Win streak' : 'Loss streak';
  return (
    <span className={`streakChip ${tint}`} aria-label={`${label}: ${length}`} title={label}>
      {kind}
      {length}
    </span>
  );
}

type EquityRange = '7d' | '30d' | '90d' | 'all';

const EQUITY_RANGE_DAYS: Record<EquityRange, number | null> = {
  '7d': 7,
  '30d': 30,
  '90d': 90,
  all: null,
};

/**
 * Per-category performance table. The backend returns categories sorted by
 * `realized_pnl` DESC, so the strongest categories surface first. We render a
 * compact five-column table (category, trades, win rate, PnL, ROI) with a
 * green / red PnL tint that mirrors the equity curve's positive/negative
 * coloring. A small empty-state copy explains the data is computed from
 * closed paper-trade lots.
 */
function CategoryBreakdown({ stats }: { stats: PaperCategoryStats[] }) {
  if (!stats || stats.length === 0) {
    return (
      <div className="categoryBreakdown empty">
        <span className="muted small">
          No category data yet — place or settle paper trades to populate per-stat performance.
        </span>
      </div>
    );
  }
  return (
    <div className="categoryBreakdown">
      <div className="categoryBreakdownHeader">
        <span className="muted small">Per-category performance</span>
        <span className="muted small">{stats.length} {stats.length === 1 ? 'category' : 'categories'}</span>
      </div>
      <table className="categoryTable">
        <thead>
          <tr>
            <th scope="col">Category</th>
            <th scope="col">Trades</th>
            <th scope="col">Win %</th>
            <th scope="col">PnL</th>
            <th scope="col">ROI</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((s) => {
            const pnlPositive = s.realized_pnl >= 0;
            return (
              <tr key={s.category}>
                <td>
                  <strong>{s.category}</strong>
                  {s.open_trades > 0 && (
                    <span className="muted small categoryOpenTag" title={`${s.open_trades} open lot(s)`}>
                      {' '}+{s.open_trades} open
                    </span>
                  )}
                </td>
                <td>{s.total_trades}</td>
                <td>{s.wins + s.losses > 0 ? `${s.win_rate.toFixed(0)}%` : '—'}</td>
                <td className={pnlPositive ? 'pos' : 'neg'}>
                  {pnlPositive ? '+' : ''}${s.realized_pnl.toFixed(2)}
                </td>
                <td className={pnlPositive ? 'pos' : 'neg'}>
                  {pnlPositive ? '+' : ''}{s.roi_pct.toFixed(1)}%
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Per-side (Over/Under) performance table. Mirrors the layout of
 * `CategoryBreakdown` so the two read as siblings, but renders the side
 * label via `paperSideLabel()` so the user sees "Over" / "Under" instead of
 * the raw backend "YES" / "NO" tokens. The data layer keeps the raw side so
 * a future cross-platform fork can re-map without code changes here.
 */
function SideBreakdown({ stats }: { stats: PaperSideStats[] }) {
  if (!stats || stats.length === 0) {
    return (
      <div className="sideBreakdown empty">
        <span className="muted small">
          No side data yet — place or settle paper trades to populate Over/Under performance.
        </span>
      </div>
    );
  }
  return (
    <div className="sideBreakdown">
      <div className="sideBreakdownHeader">
        <span className="muted small">Per-side performance (Over / Under)</span>
        <span className="muted small">{stats.length} {stats.length === 1 ? 'side' : 'sides'}</span>
      </div>
      <table className="sideTable">
        <thead>
          <tr>
            <th scope="col">Side</th>
            <th scope="col">Trades</th>
            <th scope="col">Win %</th>
            <th scope="col">PnL</th>
            <th scope="col">ROI</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((s) => {
            const pnlPositive = s.realized_pnl >= 0;
            return (
              <tr key={s.side}>
                <td>
                  <strong>{paperSideLabel(s.side)}</strong>
                  {s.open_trades > 0 && (
                    <span className="muted small sideOpenTag" title={`${s.open_trades} open lot(s)`}>
                      {' '}+{s.open_trades} open
                    </span>
                  )}
                </td>
                <td>{s.total_trades}</td>
                <td>{s.wins + s.losses > 0 ? `${s.win_rate.toFixed(0)}%` : '—'}</td>
                <td className={pnlPositive ? 'pos' : 'neg'}>
                  {pnlPositive ? '+' : ''}${s.realized_pnl.toFixed(2)}
                </td>
                <td className={pnlPositive ? 'pos' : 'neg'}>
                  {pnlPositive ? '+' : ''}{s.roi_pct.toFixed(1)}%
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Compact SVG equity curve. No charting library — pure SVG so the bundle
 * stays lean. Plots equity_dollars over time, marks the starting balance
 * as a dashed baseline, and tints the area between curve and baseline
 * green (above) or red (below) to make the trajectory obvious at a glance.
 */
function EquityCurve({ snapshots }: { snapshots: PaperEquitySnapshot[] }) {
  const points = useMemo(() => {
    if (snapshots.length === 0) return null;
    // Snapshots arrive most-recent-first; flip to chronological.
    const ordered = [...snapshots].sort((a, b) => a.ts.localeCompare(b.ts));
    const equities = ordered.map((s) => s.equity_dollars);
    const minEq = Math.min(...equities);
    const maxEq = Math.max(...equities);
    const range = Math.max(maxEq - minEq, 1);
    // 8px padding so points don't sit on the edge.
    const w = 320;
    const h = 80;
    const padX = 4;
    const padY = 6;
    const innerW = w - 2 * padX;
    const innerH = h - 2 * padY;
    const xs = ordered.map((_, i) =>
      padX + (ordered.length === 1 ? innerW / 2 : (innerW * i) / (ordered.length - 1)),
    );
    const ys = equities.map((e) => padY + innerH - ((e - minEq) / range) * innerH);
    return { w, h, padX, padY, innerH, xs, ys, ordered, minEq, maxEq, startEq: equities[0] };
  }, [snapshots]);

  if (!points || points.ordered.length === 0) {
    return (
      <div className="equityChart empty">
        <p className="muted small">No equity history yet — snapshots appear after the first paper-trade settle.</p>
      </div>
    );
  }

  const { w, h, padX, xs, ys, ordered, minEq, maxEq, startEq } = points;
  const polyline = xs.map((x, i) => `${x.toFixed(2)},${ys[i].toFixed(2)}`).join(' ');
  const areaPathClean =
    `M ${xs[0].toFixed(2)} ${(h - padX).toFixed(2)} ` +
    xs.map((x, i) => `L ${x.toFixed(2)} ${ys[i].toFixed(2)}`).join(' ') +
    ` L ${xs[xs.length - 1].toFixed(2)} ${(h - padX).toFixed(2)} Z`;
  const positive = ordered[ordered.length - 1].equity_dollars >= startEq;
  const stroke = positive ? 'var(--pos, #3fbf7f)' : 'var(--neg, #d04848)';
  const fill = positive ? 'rgba(63, 191, 127, 0.18)' : 'rgba(208, 72, 72, 0.18)';
  const last = ordered[ordered.length - 1];
  const delta = last.equity_dollars - startEq;
  const deltaPct = startEq === 0 ? 0 : (delta / startEq) * 100;

  return (
    <div className="equityChart">
      <div className="equityChartHeader">
        <div>
          <span className="muted small">Equity curve</span>
          <strong>
            ${last.equity_dollars.toFixed(2)}{' '}
            <span style={{ color: positive ? stroke : stroke }}>
              ({delta >= 0 ? '+' : ''}${delta.toFixed(2)} / {deltaPct >= 0 ? '+' : ''}
              {deltaPct.toFixed(2)}%)
            </span>
          </strong>
        </div>
        <div className="muted small">
          {ordered.length} pts · min ${minEq.toFixed(0)} · max ${maxEq.toFixed(0)}
        </div>
      </div>
      <svg
        className="equityChartSvg"
        viewBox={`0 0 ${w} ${h}`}
        preserveAspectRatio="none"
        role="img"
        aria-label="Paper trading equity curve over time"
      >
        <path d={areaPathClean} fill={fill} stroke="none" />
        <polyline points={polyline} fill="none" stroke={stroke} strokeWidth={1.5} />
      </svg>
    </div>
  );
}

export function PrizePicksPredictionsPanel() {
  const [predictions, setPredictions] = useState<PrizePicksPrediction[]>([]);
  const [analytics, setAnalytics] = useState<PaperAnalytics | null>(null);
  const [equityHistory, setEquityHistory] = useState<PaperEquitySnapshot[]>([]);
  const [range, setRange] = useState<EquityRange>('30d');
  const [loading, setLoading] = useState(true);
  const [grading, setGrading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [data, paper, equity] = await Promise.all([
        prizepicksApi.getPredictions(),
        prizepicksApi.getPaperAnalytics().catch(() => null),
        prizepicksApi.getPaperEquityHistory(500).catch(() => []),
      ]);
      setPredictions(data);
      setAnalytics(paper);
      setEquityHistory(equity);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const filteredEquity = useMemo(() => {
    if (range === 'all') return equityHistory;
    const days = EQUITY_RANGE_DAYS[range] ?? 30;
    if (equityHistory.length === 0) return equityHistory;
    // Snapshots arrive most-recent-first.
    const newestTs = equityHistory[0].ts;
    const newestDate = new Date(newestTs);
    if (Number.isNaN(newestDate.getTime())) return equityHistory;
    const cutoff = newestDate.getTime() - days * 24 * 60 * 60 * 1000;
    return equityHistory.filter((s) => {
      const t = new Date(s.ts).getTime();
      return !Number.isNaN(t) && t >= cutoff;
    });
  }, [equityHistory, range]);

  const gradePending = async () => {
    setGrading(true);
    setMessage(null);
    try {
      const summary = await prizepicksApi.gradePending();
      setMessage(`Graded ${summary.graded} (${summary.wins}W/${summary.losses}L, $${summary.total_pnl.toFixed(2)})`);
      await load();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setGrading(false);
    }
  };

  return (
    <section className="predictionsPanel">
      <div className="panelToolbar">
        <h4>Player prop picks</h4>
        <button type="button" className="ghostBtn" onClick={() => void load()} disabled={loading}>
          Refresh
        </button>
        <button type="button" className="primaryBtn" onClick={() => void gradePending()} disabled={grading}>
          {grading ? 'Grading…' : 'Grade pending'}
        </button>
      </div>
      {analytics && (
        <div className="paperSummary">
          <div>
            <span className="muted">Paper equity</span>
            <strong>${analytics.equity.toFixed(2)}</strong>
          </div>
          <div>
            <span className="muted">Cash</span>
            <strong>${analytics.cash_balance.toFixed(2)}</strong>
          </div>
          <div>
            <span className="muted">Open</span>
            <strong>{analytics.open_positions}</strong>
          </div>
          <div>
            <span className="muted">Return</span>
            <strong>{analytics.total_return_pct.toFixed(1)}%</strong>
          </div>
          <div>
            <span className="muted">Win rate</span>
            <strong>{analytics.win_rate.toFixed(0)}%</strong>
          </div>
          <div>
            <span className="muted">Max DD</span>
            <strong>{analytics.max_drawdown_pct.toFixed(1)}%</strong>
          </div>
          <div>
            <span className="muted">Streak</span>
            <StreakChip analytics={analytics} />
          </div>
        </div>
      )}
      <div className="equityChartToolbar">
        <span className="muted small">Equity range:</span>
        {(['7d', '30d', '90d', 'all'] as EquityRange[]).map((r) => (
          <button
            key={r}
            type="button"
            className={`ghostBtn small ${range === r ? 'active' : ''}`}
            onClick={() => setRange(r)}
          >
            {r === 'all' ? 'All' : r.toUpperCase()}
          </button>
        ))}
      </div>
      <EquityCurve snapshots={filteredEquity} />
      {analytics && <CategoryBreakdown stats={analytics.category_stats} />}
      {analytics && <SideBreakdown stats={analytics.side_stats} />}
      {message && <p className="muted small">{message}</p>}
      {loading && <p className="muted">Loading predictions…</p>}
      <div className="predList">
        {predictions.map((pred) => {
          const won = prizepicksBetWon(pred);
          const pending = pred.actual_outcome == null;
          return (
            <article
              key={pred.id}
              className={`predCard ${pending ? 'pending' : won ? 'win' : 'loss'}`}
            >
              <header>
                <span>{pred.title}</span>
                <span>{pred.contract_side === 'YES' ? 'Over' : pred.contract_side === 'NO' ? 'Under' : pred.pick_type ?? '—'}</span>
              </header>
              <div className="predMeta">
                <span>Conf {pred.predicted_probability.toFixed(1)}%</span>
                <span>Stake ${pred.stake_amount.toFixed(2)}</span>
                {pred.pnl != null && <span>PnL ${pred.pnl.toFixed(2)}</span>}
              </div>
              {!pending && (
                <strong className={won ? 'pos' : 'neg'}>{won ? 'Win' : 'Loss'}</strong>
              )}
            </article>
          );
        })}
        {!loading && predictions.length === 0 && (
          <p className="muted">No prop picks yet — use the Analyst chat or Prop board to record picks.</p>
        )}
      </div>
    </section>
  );
}
