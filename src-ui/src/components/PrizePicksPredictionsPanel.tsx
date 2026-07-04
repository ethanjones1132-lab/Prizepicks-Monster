import { useCallback, useEffect, useMemo, useState } from 'react';
import { prizepicksApi } from '../services/prizepicks';
import { paperSideLabel } from '../types/prizepicks';
import { prizepicksBetWon } from '../services/prizepicks';
import type {
  CalibrationPoint,
  PaperAnalytics,
  PaperCategoryStats,
  PaperConfidenceTierStats,
  PaperDisagreementStats,
  PaperEntryPriceStats,
  PaperEquitySnapshot,
  PaperHoldTimeStats,
  PaperLot,
  PaperPlayerStats,
  PaperSideStats,
  PaperSourceStats,
  PaperTagStats,
  PrizePicksPrediction,
  SessionDelta,
} from '../types/prizepicks';

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

/**
 * Render a single session PnL delta (today / 7d) as a tinted dollar+percent
 * chip. A `null` delta renders the muted `—` placeholder so the summary
 * layout doesn't shift when an account is too new to have a baseline.
 * The tooltip includes the absolute baseline equity + its timestamp so a
 * user can see exactly which snapshot the delta was computed from.
 */
function SessionDeltaChip({ delta }: { delta: SessionDelta | null }) {
  if (!delta) {
    return (
      <span
        className="sessionDeltaChip muted"
        aria-label="No baseline snapshot yet"
        title="No baseline snapshot yet — place a paper trade to seed the equity history."
      >
        —
      </span>
    );
  }
  const sign = delta.pnl_dollars >= 0 ? '+' : '−';
  const tint = delta.pnl_dollars >= 0 ? 'pos' : 'neg';
  const dollars = `${sign}$${Math.abs(delta.pnl_dollars).toFixed(2)}`;
  const pct = `${delta.pnl_pct >= 0 ? '+' : ''}${delta.pnl_pct.toFixed(1)}%`;
  const tooltip =
    `Baseline: $${delta.baseline_equity.toFixed(2)} at ${delta.baseline_ts}\n` +
    `Δ = $${delta.pnl_dollars.toFixed(2)} (${delta.pnl_pct.toFixed(2)}%)`;
  return (
    <span className={`sessionDeltaChip ${tint}`} title={tooltip} aria-label={tooltip}>
      {dollars} <span className="sessionDeltaPct">({pct})</span>
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
 * Format a hold duration in seconds into a short human-readable string
 * for the avg/median cells. Hours roll into days for legibility. Sub-second
 * values render as "0s" rather than "0.0s" to keep the row compact.
 */
function formatHoldSeconds(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return '—';
  if (secs < 60) return `${secs.toFixed(0)}s`;
  if (secs < 3600) return `${(secs / 60).toFixed(0)}m`;
  if (secs < 86_400) return `${(secs / 3600).toFixed(1)}h`;
  return `${(secs / 86_400).toFixed(1)}d`;
}

/**
 * Per-hold-time-bucket performance table. Mirrors the layout of
 * `CategoryBreakdown` and `SideBreakdown` so all three read as siblings.
 * Shows the same five metrics (Hold time / Trades / Win % / PnL / ROI) plus
 * an avg+median hold duration cell. The backend emits buckets in
 * chronological order (Intraday → SameDay → MultiDay → Long → unknown),
 * so the table renders as a stable "fastest to slowest" ladder.
 */
function HoldTimeBreakdown({ stats }: { stats: PaperHoldTimeStats[] }) {
  if (!stats || stats.length === 0) {
    return (
      <div className="holdTimeBreakdown empty">
        <span className="muted small">
          No hold-time data yet — place or settle paper trades to populate per-duration performance.
        </span>
      </div>
    );
  }
  return (
    <div className="holdTimeBreakdown">
      <div className="holdTimeBreakdownHeader">
        <span className="muted small">Per-hold-time performance (Intraday → Long)</span>
        <span className="muted small">{stats.length} {stats.length === 1 ? 'bucket' : 'buckets'}</span>
      </div>
      <table className="holdTimeTable">
        <thead>
          <tr>
            <th scope="col">Hold time</th>
            <th scope="col">Trades</th>
            <th scope="col">Win %</th>
            <th scope="col">PnL</th>
            <th scope="col">ROI</th>
            <th scope="col">Avg / Median hold</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((s) => {
            const pnlPositive = s.realized_pnl >= 0;
            const decided = s.wins + s.losses;
            const showHold = s.avg_hold_seconds > 0 || s.median_hold_seconds > 0;
            return (
              <tr key={s.bucket}>
                <td>
                  <strong>{s.bucket_label}</strong>
                  {s.open_trades > 0 && (
                    <span className="muted small holdTimeOpenTag" title={`${s.open_trades} open lot(s)`}>
                      {' '}+{s.open_trades} open
                    </span>
                  )}
                </td>
                <td>{s.total_trades}</td>
                <td>{decided > 0 ? `${s.win_rate.toFixed(0)}%` : '—'}</td>
                <td className={pnlPositive ? 'pos' : 'neg'}>
                  {pnlPositive ? '+' : ''}${s.realized_pnl.toFixed(2)}
                </td>
                <td className={pnlPositive ? 'pos' : 'neg'}>
                  {pnlPositive ? '+' : ''}{s.roi_pct.toFixed(1)}%
                </td>
                <td className="muted small">
                  {showHold
                    ? `${formatHoldSeconds(s.avg_hold_seconds)} / ${formatHoldSeconds(s.median_hold_seconds)}`
                    : '—'}
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
 * Per-player performance table. Mirrors the layout of
 * `CategoryBreakdown` / `SideBreakdown` so all four read as siblings.
 * The `player` field is the name extracted from the lot's `title`
 * (`"<name> Over|Under <line> <stat>"` pattern) on the backend. Lots
 * with unparseable titles are bucketed under "Unknown" so they still
 * appear in the table.
 */
function PlayerBreakdown({ stats }: { stats: PaperPlayerStats[] }) {
  if (!stats || stats.length === 0) {
    return (
      <div className="playerBreakdown empty">
        <span className="muted small">
          No player data yet — place or settle paper trades to populate per-player performance.
        </span>
      </div>
    );
  }
  return (
    <div className="playerBreakdown">
      <div className="playerBreakdownHeader">
        <span className="muted small">Per-player performance</span>
        <span className="muted small">{stats.length} {stats.length === 1 ? 'player' : 'players'}</span>
      </div>
      <table className="playerTable">
        <thead>
          <tr>
            <th scope="col">Player</th>
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
              <tr key={s.player}>
                <td>
                  <strong>{s.player}</strong>
                  {s.open_trades > 0 && (
                    <span className="muted small playerOpenTag" title={`${s.open_trades} open lot(s)`}>
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
   * Per-entry-price-bucket performance table. Mirrors the layout of
   * `CategoryBreakdown` / `SideBreakdown` / `PlayerBreakdown` so all five
   * read as siblings. Shows the same five metrics (Bucket / Trades / Win % /
   * PnL / ROI) with the same pos/neg PnL tint. The backend emits 20-cent-wide
   * buckets (0-20¢, 20-40¢, 40-60¢, 60-80¢, 80-100¢) in ascending order so the
   * table renders as a stable "long-shot to favorite" ladder.
   */
  function EntryPriceBreakdown({ stats }: { stats: PaperEntryPriceStats[] }) {
    if (!stats || stats.length === 0) {
      return (
        <div className="entryPriceBreakdown empty">
          <span className="muted small">
            No entry-price data yet — place or settle paper trades to populate per-price performance.
          </span>
        </div>
      );
    }
    return (
      <div className="entryPriceBreakdown">
        <div className="entryPriceBreakdownHeader">
          <span className="muted small">Per-entry-price performance (long-shot → favorite)</span>
          <span className="muted small">{stats.length} {stats.length === 1 ? 'bucket' : 'buckets'}</span>
        </div>
        <table className="entryPriceTable">
          <thead>
            <tr>
              <th scope="col">Entry price</th>
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
                <tr key={s.bucket}>
                  <td>
                    <strong>{s.bucket}</strong>
                    {s.open_trades > 0 && (
                      <span className="muted small entryPriceOpenTag" title={`${s.open_trades} open lot(s)`}>
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
   * Per-disagreement-bucket performance table. Mirrors the layout of
   * `CategoryBreakdown` / `SideBreakdown` / `PlayerBreakdown` /
   * `EntryPriceBreakdown` so all six read as siblings. Shows the same
   * five metrics (Bucket / Trades / Win % / PnL / ROI) with the same
   * pos/neg PnL tint.
   *
   * The backend always emits the three canonical buckets in a fixed
   * order (Disagreement → Consensus → Unknown) so the table renders as
   * a stable "disagree → agree → unknown" ladder without resorting. An
   * inline `<small>` note explains the 12pp threshold for users who
   * don't know what "disagreement" means in this context. Empty
   * buckets render with muted zeros so the table layout doesn't
   * shift as the user's history grows.
   */
  function DisagreementBreakdown({ stats }: { stats: PaperDisagreementStats[] }) {
    if (!stats || stats.length === 0) {
      return (
        <div className="disagreementBreakdown empty">
          <span className="muted small">
            No decision data yet — place paper trades through the Analyst chat to populate disagreement-bucket performance.
          </span>
        </div>
      );
    }
    return (
      <div className="disagreementBreakdown">
        <div className="disagreementBreakdownHeader">
          <span className="muted small">
            Per-disagreement-bucket performance
            {' '}
            <span className="muted small" title="Disagreement = |model fair % − market price %| > 12pp at entry">
              (disagree → agree → unknown)
            </span>
          </span>
          <span className="muted small">{stats.length} buckets</span>
        </div>
        <table className="disagreementTable">
          <thead>
            <tr>
              <th scope="col">Disagreement</th>
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
                <tr key={s.bucket}>
                  <td>
                    <strong>{s.bucket_label}</strong>
                    {s.open_trades > 0 && (
                      <span className="muted small disagreementOpenTag" title={`${s.open_trades} open lot(s)`}>
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
   * Per-confidence-tier performance table. Mirrors the layout of
   * `DisagreementBreakdown` and the other breakdowns so the views
   * read as siblings. Shows the same five metrics (Tier / Trades /
   * Win % / PnL / ROI) with the same pos/neg PnL tint.
   *
   * The backend always emits the four canonical tiers in a fixed
   * order (High → Medium → Low → None, highest conviction to lowest)
   * so the table renders as a stable "conviction ladder" without
   * resorting. The `bucket_label` (e.g. `"High"`, `"None"`) is what
   * the table actually displays — the raw `bucket` enum is for
   * machine-readable comparison. An inline `<small>` note explains
   * the four-tier meaning for users who don't know what confidence
   * tier means in this context. Empty tiers render with muted
   * zeros so the table layout doesn't shift as the user's history
   * grows.
   *
   * The companion to `DisagreementBreakdown` — together they answer
   * the question "is the model self-aware?" (i.e. are the
   * high-confidence picks actually the profitable ones?).
   */
  function ConfidenceTierBreakdown({ stats }: { stats: PaperConfidenceTierStats[] }) {
    if (!stats || stats.length === 0) {
      return (
        <div className="confidenceTierBreakdown empty">
          <span className="muted small">
            No decision data yet — place paper trades through the Analyst chat to populate confidence-tier performance.
          </span>
        </div>
      );
    }
    return (
      <div className="confidenceTierBreakdown">
        <div className="confidenceTierBreakdownHeader">
          <span className="muted small">
            Per-confidence-tier performance
            {' '}
            <span className="muted small" title="Confidence tier is the model's stated conviction at entry (from decision_json.confidence_tier).">
              (high → medium → low → none)
            </span>
          </span>
          <span className="muted small">{stats.length} tiers</span>
        </div>
        <table className="confidenceTierTable">
          <thead>
            <tr>
              <th scope="col">Confidence</th>
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
                <tr key={s.bucket}>
                  <td>
                    <strong>{s.bucket_label}</strong>
                    {s.open_trades > 0 && (
                      <span className="muted small confidenceTierOpenTag" title={`${s.open_trades} open lot(s)`}>
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
   * Per-source (AI vs Manual) performance table. Mirrors the layout
   * of `DisagreementBreakdown` and the other breakdowns so the views
   * read as siblings. Shows the same five metrics (Source / Trades /
   * Win % / PnL / ROI) with the same pos/neg PnL tint.
   *
   * The backend always emits the two canonical sources in a fixed
   * order (`ai_decision` → `manual`) so the table renders as a stable
   * "AI vs human" comparison without resorting. The `source_label`
   * (e.g. `"AI decision"`, `"Manual"`) is what the table actually
   * displays — the raw `source` enum is for machine-readable
   * comparison. An inline `<small>` note explains the headline
   * question for users who don't yet know what the table answers.
   *
   * The headline question this breakdown answers: **"is the AI model
   * actually profitable vs. my manual picks?"** — the central
   * evaluation question for the entire app.
   */
  function SourceBreakdown({ stats }: { stats: PaperSourceStats[] }) {
    if (!stats || stats.length === 0) {
      return (
        <div className="sourceBreakdown empty">
          <span className="muted small">
            No paper trades yet — place a few (some via Analyst chat, some manually) to populate the AI-vs-manual comparison.
          </span>
        </div>
      );
    }
    return (
      <div className="sourceBreakdown">
        <div className="sourceBreakdownHeader">
          <span className="muted small">
            Per-source performance (AI vs Manual)
            {' '}
            <span className="muted small" title="Is the AI model actually profitable vs. your manual picks?">
              (ai decision → manual)
            </span>
          </span>
          <span className="muted small">{stats.length} sources</span>
        </div>
        <table className="sourceTable">
          <thead>
            <tr>
              <th scope="col">Source</th>
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
                <tr key={s.source}>
                  <td>
                    <strong>{s.source_label}</strong>
                    {s.open_trades > 0 && (
                      <span className="muted small sourceOpenTag" title={`${s.open_trades} open lot(s)`}>
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
   * Per-tag performance breakdown. Renders a five-column table
   * (Tag / Trades / Win % / PnL / ROI) for the tags parsed out of the
   * `paper_lots.tags` field. The Rust side splits each lot's tags on
   * commas, lowercases + trims, and a lot with multiple tags
   * contributes to *each* tag bucket. Lots with no tags are silently
   * skipped — no "Untagged" bucket — so the empty state copy
   * ("tag your trades to see per-tag performance") guides the user
   * toward the journal editor. Sorted by `realized_pnl` DESC with
   * alphabetical tiebreak, so the strongest tag surfaces first.
   *
   * Mirrors the `CategoryBreakdown` / `SideBreakdown` / `DisagreementBreakdown`
   * table style so all of the per-axis performance views read as siblings.
   * Open lot count surfaces as a `+N open` muted tag next to the tag name.
   */
  function TagBreakdown({ stats }: { stats: PaperTagStats[] }) {
    if (!stats || stats.length === 0) {
      return (
        <div className="tagBreakdown empty">
          <span className="muted small">
            No tagged trades yet — use the paper-journal editor (📝 Journal below) to add tags like
            <code> injury, regression, value, sharp</code> and the breakdown will populate.
          </span>
        </div>
      );
    }
    return (
      <div className="tagBreakdown">
        <div className="tagBreakdownHeader">
          <span className="muted small">
            Per-tag performance
            {' '}
            <span
              className="muted small"
              title="Tags come from paper_lots.tags (comma-separated, lowercased). Lots with multiple tags contribute to each."
            >
              (a lot with N tags counts toward N buckets)
            </span>
          </span>
          <span className="muted small">{stats.length} tags</span>
        </div>
        <table className="tagTable">
          <thead>
            <tr>
              <th scope="col">Tag</th>
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
                <tr key={s.tag}>
                  <td>
                    <span className="tagChip">#{s.tag}</span>
                    {s.open_trades > 0 && (
                      <span className="muted small tagOpenTag" title={`${s.open_trades} open lot(s)`}>
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
   * Pure-SVG scatter of model `fair_probability_pct` (X axis, 0-100) vs
   * realized PnL in dollars (Y axis). One circle per closed paper lot;
   * bubble radius is scaled by `stake_dollars` (clamped to a sensible
   * range so a $5 lot and a $5,000 lot don't render at extremes). Color
   * is green for wins, red for losses, muted gray for pushes. The zero-PnL
   * baseline is drawn as a horizontal dashed line. A vertical reference
   * line at 50% (the break-even implied probability for a 50¢ line) makes
   * over/under-confident regions easy to spot.
   *
   * Hovering a point surfaces the lot title + fair_probability_pct + PnL +
   * stake. Hovering is implemented with a single absolutely-positioned
   * `<div>` that gets re-rendered with the active point's text — no chart
   * lib required.
   */
  function CalibrationScatter({ points }: { points: CalibrationPoint[] }) {
    const [hoverIdx, setHoverIdx] = useState<number | null>(null);

    if (!points || points.length === 0) {
      return (
        <div className="calibrationScatter empty">
          <span className="muted small">
            No calibration data yet — settle paper trades (with a recorded
            decision) to populate the model-vs-result scatter.
          </span>
        </div>
      );
    }

    // Layout — same proportions as the equity curve so the panel stays
    // visually consistent. 360x180 gives a 2:1 aspect ratio that's wide
    // enough to read horizontal scatter without dominating the panel.
    const w = 360;
    const h = 180;
    const padL = 44; // room for Y-axis labels
    const padR = 12;
    const padT = 12;
    const padB = 28; // room for X-axis labels
    const innerW = w - padL - padR;
    const innerH = h - padT - padB;

    // Y range: symmetric around the largest-magnitude PnL so the zero
    // baseline is always in the middle. Clamp to ±$20 minimum so a single
    // tiny lot doesn't compress the plot to a flat line.
    const pnlExtent = Math.max(
      20,
      ...points.map((p) => Math.abs(p.realized_pnl_dollars)),
    );
    const minPnl = -pnlExtent;
    const maxPnl = pnlExtent;

    // X range is always 0-100 (% probability). The fair_probability_pct
    // is the model's "true" probability for the selected side, so it
    // lives in [0, 100] (clamped on the backend).

    const xFor = (pct: number) => padL + (pct / 100) * innerW;
    const yFor = (pnl: number) =>
      padT + innerH - ((pnl - minPnl) / (maxPnl - minPnl)) * innerH;

    // Bubble radius: clamped between 3 and 12 px so the smallest and
    // largest stakes are still readable. We use a sqrt scale so the
    // bubble AREA (not radius) scales linearly with stake — this is
    // the standard "perceptually accurate" bubble chart.
    const stakes = points.map((p) => p.stake_dollars);
    const minStake = Math.min(...stakes);
    const maxStake = Math.max(...stakes);
    const stakeRange = Math.max(maxStake - minStake, 0.01);
    const rFor = (stake: number) => {
      const t = (stake - minStake) / stakeRange; // 0..1
      return 3 + Math.sqrt(t) * 9; // 3..12 px
    };

    // Y axis tick lines at -pnlExtent, 0, +pnlExtent. X axis tick lines
    // at 0%, 25%, 50%, 75%, 100% (so the 50% reference line is one of
    // them). Render in a single pass.
    const yTicks = [minPnl, 0, maxPnl];
    const xTicks = [0, 25, 50, 75, 100];

    // Counts for the header — useful at a glance.
    const wins = points.filter((p) => p.won === true).length;
    const losses = points.filter((p) => p.won === false).length;
    const pushes = points.filter((p) => p.won === null).length;
    const noDecision = points.filter((p) => p.market_price_cents == null && p.fair_probability_pct === 0).length;

    const hover = hoverIdx != null ? points[hoverIdx] : null;

    return (
      <div className="calibrationScatter">
        <div className="calibrationScatterHeader">
          <span className="muted small">
            Calibration scatter (model fair % → realized PnL)
          </span>
          <span className="muted small">
            {points.length} {points.length === 1 ? 'lot' : 'lots'}
            {wins > 0 && ` · ${wins}W`}
            {losses > 0 && ` · ${losses}L`}
            {pushes > 0 && ` · ${pushes} push`}
            {noDecision > 0 && ` · ${noDecision} no-decision`}
          </span>
        </div>
        <div className="calibrationScatterCanvas">
          <svg
            className="calibrationScatterSvg"
            viewBox={`0 0 ${w} ${h}`}
            preserveAspectRatio="xMidYMid meet"
            role="img"
            aria-label="Calibration scatter: model fair probability vs realized PnL"
          >
            {/* Y grid + axis labels. */}
            {yTicks.map((p) => (
              <g key={`y-${p}`}>
                <line
                  x1={padL}
                  x2={w - padR}
                  y1={yFor(p)}
                  y2={yFor(p)}
                  stroke="rgba(255,255,255,0.08)"
                  strokeWidth={1}
                />
                <text
                  x={padL - 6}
                  y={yFor(p) + 3}
                  textAnchor="end"
                  fontSize={9}
                  fill="rgba(255,255,255,0.55)"
                >
                  {p >= 0 ? `+$${p.toFixed(0)}` : `-$${Math.abs(p).toFixed(0)}`}
                </text>
              </g>
            ))}
            {/* X grid + axis labels. */}
            {xTicks.map((p) => (
              <g key={`x-${p}`}>
                <line
                  x1={xFor(p)}
                  x2={xFor(p)}
                  y1={padT}
                  y2={h - padB}
                  stroke={p === 50 ? 'rgba(255,255,255,0.18)' : 'rgba(255,255,255,0.05)'}
                  strokeWidth={p === 50 ? 1 : 0.5}
                  strokeDasharray={p === 50 ? '3 3' : undefined}
                />
                <text
                  x={xFor(p)}
                  y={h - padB + 14}
                  textAnchor="middle"
                  fontSize={9}
                  fill="rgba(255,255,255,0.55)"
                >
                  {p}%
                </text>
              </g>
            ))}
            {/* Zero baseline (more visible than the gridlines). */}
            <line
              x1={padL}
              x2={w - padR}
              y1={yFor(0)}
              y2={yFor(0)}
              stroke="rgba(255,255,255,0.35)"
              strokeWidth={1}
            />
            {/* Points. */}
            {points.map((p, i) => {
              const fill =
                p.won === true
                  ? 'var(--pos, #3fbf7f)'
                  : p.won === false
                  ? 'var(--neg, #d04848)'
                  : 'rgba(255,255,255,0.45)';
              return (
                <circle
                  key={p.lot_id || i}
                  cx={xFor(p.fair_probability_pct)}
                  cy={yFor(p.realized_pnl_dollars)}
                  r={rFor(p.stake_dollars)}
                  fill={fill}
                  fillOpacity={0.55}
                  stroke={fill}
                  strokeWidth={hoverIdx === i ? 1.5 : 0.5}
                  onMouseEnter={() => setHoverIdx(i)}
                  onMouseLeave={() => setHoverIdx(null)}
                  style={{ cursor: 'pointer' }}
                />
              );
            })}
          </svg>
          {hover && (
            <div
              className="calibrationScatterTooltip"
              style={{
                left: `${(xFor(hover.fair_probability_pct) / w) * 100}%`,
                top: `${(yFor(hover.realized_pnl_dollars) / h) * 100}%`,
              }}
            >
              <div className="calibrationScatterTooltipTitle">
                {hover.title || hover.ticker}
              </div>
              <div className="muted small">
                Fair {hover.fair_probability_pct.toFixed(1)}%
                {hover.market_price_cents != null
                  ? ` · Market ${hover.market_price_cents.toFixed(0)}¢`
                  : ' · no market price'}
              </div>
              <div
                className={
                  hover.realized_pnl_dollars > 0
                    ? 'pos'
                    : hover.realized_pnl_dollars < 0
                    ? 'neg'
                    : 'muted'
                }
              >
                {hover.realized_pnl_dollars >= 0 ? '+' : ''}${hover.realized_pnl_dollars.toFixed(2)}
                {' · '}
                ${hover.stake_dollars.toFixed(0)} stake
                {hover.won === true && ' · Win'}
                {hover.won === false && ' · Loss'}
                {hover.won === null && ' · Push'}
              </div>
            </div>
          )}
        </div>
        <div className="calibrationLegend muted small">
          <span className="calibrationLegendDot pos" /> Win
          <span className="calibrationLegendDot neg" /> Loss
          <span className="calibrationLegendDot muted" /> Push
          <span className="calibrationLegendSep">·</span>
          Bubble size ∝ stake
        </div>
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

/**
 * Paper Journal — inline notes/tags editor for recent paper lots.
 * Each row exposes a one-line summary (title, side, stake, status, PnL) plus
 * a notes textarea and a tags input. The Save button calls
 * `prizepicksApi.updatePaperLotNotes` with both fields and reflects the
 * server-confirmed values back into local state. A status filter (All /
 * Open / Closed) sits in the header so the user can focus on actionable
 * positions vs. settled history. Empty state guides the user toward placing
 * paper trades.
 */
function PaperJournal({ lots, onUpdated }: { lots: PaperLot[]; onUpdated: (lot: PaperLot) => void }) {
  const [filter, setFilter] = useState<'All' | 'Open' | 'Closed'>('All');
  // Free-text search across title, notes, and tags. Case-insensitive
  // substring match — the goal is "find this lot again" not "fuzzy
  // match the closest title". Empty string means no search active.
  const [search, setSearch] = useState('');
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editNotes, setEditNotes] = useState('');
  const [editTags, setEditTags] = useState('');
  const [savingId, setSavingId] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  // Filter pipeline: status first (cheap, often trims to a small subset),
  // then free-text search. Search is normalized to lowercase once per
  // render so each lot's check stays O(title + notes + tags). Whitespace
  // around the search term is trimmed; an all-whitespace search is
  // treated as empty (no filter) so the placeholder remains accurate.
  const filtered = useMemo(() => {
    const searchTerm = search.trim().toLowerCase();
    return lots.filter((l) => {
      if (filter !== 'All' && l.status !== filter) return false;
      if (!searchTerm) return true;
      const haystack = [
        l.title ?? '',
        l.notes ?? '',
        l.tags ?? '',
        // Ticker + category help when the title is "(untitled)" but the
        // user remembers "I traded the QQQ-style Over on points".
        l.ticker ?? '',
        l.category ?? '',
      ]
        .join(' ')
        .toLowerCase();
      return haystack.includes(searchTerm);
    });
  }, [lots, filter, search]);

  const beginEdit = (lot: PaperLot) => {
    setEditingId(lot.id);
    setEditNotes(lot.notes ?? '');
    setEditTags(lot.tags ?? '');
    setSaveError(null);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditNotes('');
    setEditTags('');
    setSaveError(null);
  };

  const saveEdit = async (lot: PaperLot) => {
    setSavingId(lot.id);
    setSaveError(null);
    try {
      const updated = await prizepicksApi.updatePaperLotNotes(
        lot.id,
        editNotes,
        editTags,
      );
      onUpdated(updated);
      setEditingId(null);
      setEditNotes('');
      setEditTags('');
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingId(null);
    }
  };

  if (lots.length === 0) {
    return (
      <div className="paperJournal empty">
        <span className="muted small">
          No paper lots yet — place or settle paper trades to start journaling your reasoning.
        </span>
      </div>
    );
  }

  return (
    <div className="paperJournal">
      <div className="paperJournalHeader">
        <span className="muted small">📝 Paper journal</span>
        <div className="paperJournalFilters">
          {(['All', 'Open', 'Closed'] as const).map((f) => (
            <button
              key={f}
              type="button"
              className={`ghostBtn small ${filter === f ? 'active' : ''}`}
              onClick={() => setFilter(f)}
            >
              {f}
            </button>
          ))}
        </div>
        <div className="paperJournalSearch">
          <input
            type="search"
            className="paperJournalSearchInput"
            placeholder="🔍 title / notes / tags / ticker"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Filter paper lots by title, notes, tags, or ticker"
            title="Filter paper lots by title, notes, tags, ticker, or category"
          />
          {search && (
            <button
              type="button"
              className="ghostBtn small paperJournalSearchClear"
              onClick={() => setSearch('')}
              title="Clear search"
              aria-label="Clear search"
            >
              ✕
            </button>
          )}
        </div>
        <span className="muted small">
          {filtered.length} of {lots.length}
        </span>
      </div>
      <div className="paperJournalList">
        {filtered.map((lot) => {
          const isOpen = lot.status === 'Open';
          const pnlPositive = (lot.realized_pnl ?? 0) >= 0;
          const isEditing = editingId === lot.id;
          const isSaving = savingId === lot.id;
          const hasNotes = !!(lot.notes || lot.tags);
          return (
            <article key={lot.id} className={`paperJournalRow ${isOpen ? 'open' : 'closed'}`}>
              <div className="paperJournalRowHeader">
                <div className="paperJournalRowTitle">
                  <strong>{lot.title || '(untitled)'}</strong>
                  <span className="muted small">
                    {paperSideLabel(lot.side)} · ${lot.stake_dollars.toFixed(2)} @ {lot.entry_price_cents.toFixed(0)}¢
                  </span>
                </div>
                <div className="paperJournalRowMeta">
                  <span className={`paperJournalStatus ${isOpen ? 'open' : 'closed'}`}>
                    {lot.status}
                  </span>
                  {lot.realized_pnl != null && (
                    <span className={pnlPositive ? 'pos' : 'neg'}>
                      {pnlPositive ? '+' : ''}${lot.realized_pnl.toFixed(2)}
                    </span>
                  )}
                  {!isEditing && (
                    <button
                      type="button"
                      className="ghostBtn small"
                      onClick={() => beginEdit(lot)}
                      title={hasNotes ? 'Edit notes/tags' : 'Add notes/tags'}
                    >
                      {hasNotes ? '✏️ Edit' : '＋ Note'}
                    </button>
                  )}
                </div>
              </div>
              {hasNotes && !isEditing && (
                <div className="paperJournalReadonly">
                  {lot.notes && <div className="paperJournalNotes">{lot.notes}</div>}
                  {lot.tags && (
                    <div className="paperJournalTags">
                      {lot.tags.split(',').map((t) => t.trim()).filter(Boolean).map((t) => (
                        <span key={t} className="paperJournalTag">{t}</span>
                      ))}
                    </div>
                  )}
                </div>
              )}
              {isEditing && (
                <div className="paperJournalEditor">
                  <textarea
                    className="paperJournalTextarea"
                    rows={2}
                    placeholder="Notes — e.g. 'injury-watch, line moved 1.5pts'"
                    value={editNotes}
                    onChange={(e) => setEditNotes(e.target.value)}
                  />
                  <input
                    className="paperJournalTagInput"
                    type="text"
                    placeholder="Tags — comma-separated, e.g. injury,regression,underdog"
                    value={editTags}
                    onChange={(e) => setEditTags(e.target.value)}
                  />
                  <div className="paperJournalEditorActions">
                    <button
                      type="button"
                      className="primaryBtn small"
                      onClick={() => void saveEdit(lot)}
                      disabled={isSaving}
                    >
                      {isSaving ? 'Saving…' : 'Save'}
                    </button>
                    <button
                      type="button"
                      className="ghostBtn small"
                      onClick={cancelEdit}
                      disabled={isSaving}
                    >
                      Cancel
                    </button>
                    {saveError && (
                      <span className="neg small">{saveError}</span>
                    )}
                  </div>
                </div>
              )}
            </article>
          );
        })}
        {filtered.length === 0 && lots.length > 0 && (
          <div className="paperJournal empty-filter">
            <span className="muted small">
              No paper lots match{' '}
              {search && filter !== 'All'
                ? `“${search.trim()}” in ${filter.toLowerCase()} lots`
                : search
                ? `“${search.trim()}”`
                : `the ${filter.toLowerCase()} filter`}
              . Try a shorter term or{' '}
              <button
                type="button"
                className="ghostBtn small paperJournalClearLink"
                onClick={() => {
                  setSearch('');
                  setFilter('All');
                }}
              >
                clear filters
              </button>
              .
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

export function PrizePicksPredictionsPanel() {
  const [predictions, setPredictions] = useState<PrizePicksPrediction[]>([]);
  const [analytics, setAnalytics] = useState<PaperAnalytics | null>(null);
  const [equityHistory, setEquityHistory] = useState<PaperEquitySnapshot[]>([]);
  const [paperLots, setPaperLots] = useState<PaperLot[]>([]);
  const [range, setRange] = useState<EquityRange>('30d');
  const [loading, setLoading] = useState(true);
  const [grading, setGrading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [data, paper, equity, lots] = await Promise.all([
        prizepicksApi.getPredictions(),
        prizepicksApi.getPaperAnalytics().catch(() => null),
        prizepicksApi.getPaperEquityHistory(500).catch(() => []),
        prizepicksApi.getPaperLots(undefined, 200).catch(() => []),
      ]);
      setPredictions(data);
      setAnalytics(paper);
      setEquityHistory(equity);
      setPaperLots(lots);
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

  // Reflect an updated paper lot (notes/tags save) back into the journal
  // state without a full reload — keeps the editor responsive and avoids
  // a network round-trip on every Save.
  const handleLotUpdated = useCallback((updated: PaperLot) => {
    setPaperLots((prev) => prev.map((l) => (l.id === updated.id ? updated : l)));
  }, []);

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
          <div>
            <span className="muted">Today PnL</span>
            <SessionDeltaChip delta={analytics.session_pnl?.today ?? null} />
          </div>
          <div>
            <span className="muted">7d PnL</span>
            <SessionDeltaChip delta={analytics.session_pnl?.this_week ?? null} />
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
      {analytics && <HoldTimeBreakdown stats={analytics.hold_time_stats} />}
      {analytics && <PlayerBreakdown stats={analytics.player_stats} />}
      {analytics && <EntryPriceBreakdown stats={analytics.entry_price_stats} />}
      {analytics && <DisagreementBreakdown stats={analytics.paper_disagreement_stats} />}
      {analytics && <ConfidenceTierBreakdown stats={analytics.confidence_tier_stats} />}
      {analytics && <TagBreakdown stats={analytics.tag_stats} />}
      {analytics && <SourceBreakdown stats={analytics.source_stats} />}
      {analytics && <CalibrationScatter points={analytics.calibration_points} />}
      <PaperJournal lots={paperLots} onUpdated={handleLotUpdated} />
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
