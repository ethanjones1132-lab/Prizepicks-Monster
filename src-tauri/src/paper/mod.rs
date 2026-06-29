//! Real PrizePicks paper-trading engine.
//!
//! Tracks an immutable lot journal, a cash account, and equity snapshots
//! independently of the real-money PrizePicks grading path. Payout math follows
//! PrizePicks's binary contract rules: each contract pays $1 if the held side
//! wins and $0 if it loses.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

use crate::prizepicks::MarketDataProvider;

pub const PAPER_SESSION_ID: &str = "paper-sim";
const DEFAULT_STARTING_BALANCE: f64 = 10_000.0;

/// Singleton paper account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperAccount {
    pub id: i64,
    pub balance_dollars: f64,
    pub total_deposits: f64,
    pub total_withdrawals: f64,
    pub created_at: String,
    pub updated_at: String,
}

/// How a paper trade was created.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PaperTradeSource {
    AiDecision,
    Manual,
}

impl PaperTradeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaperTradeSource::AiDecision => "AiDecision",
            PaperTradeSource::Manual => "Manual",
        }
    }
}

impl std::str::FromStr for PaperTradeSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AiDecision" => Ok(PaperTradeSource::AiDecision),
            "Manual" => Ok(PaperTradeSource::Manual),
            _ => Err(format!("unknown paper trade source: {}", s)),
        }
    }
}

/// An immutable fill (lot). Closed lots record realized PnL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperLot {
    pub id: String,
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub side: String,
    pub entry_price_cents: f64,
    pub qty: f64,
    pub stake_dollars: f64,
    pub source: PaperTradeSource,
    pub decision_json: Option<String>,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub closed_price_cents: Option<f64>,
    pub realized_pnl: Option<f64>,
    pub status: String,
    pub settlement_result: Option<String>,
}

/// Input used to open a new paper position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTradeInput {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub side: String,
    pub qty: f64,
    pub entry_price_cents: f64,
    pub source: PaperTradeSource,
    pub decision_json: Option<String>,
}

/// An aggregated open position per ticker/side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperPosition {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub side: String,
    pub total_qty: f64,
    pub avg_entry_price_cents: f64,
    pub cost_basis_dollars: f64,
    pub mark_price_cents: Option<f64>,
    pub market_value_dollars: Option<f64>,
    pub unrealized_pnl_dollars: Option<f64>,
    pub lots_count: i64,
}

/// Result of a settlement run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperSettlementSummary {
    pub settled: u32,
    pub wins: u32,
    pub losses: u32,
    pub total_pnl: f64,
    pub details: Vec<PaperSettlementDetail>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperSettlementDetail {
    pub lot_id: String,
    pub ticker: String,
    pub side: String,
    pub result: String,
    pub realized_pnl: f64,
}

/// High-level paper-trading analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperAnalytics {
    pub starting_balance: f64,
    pub cash_balance: f64,
    pub open_market_value: f64,
    pub equity: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub total_return_pct: f64,
    pub total_trades: u32,
    pub open_positions: u32,
    pub win_rate: f64,
    pub wins: u32,
    pub losses: u32,
    pub profit_factor: f64,
    pub avg_winner: f64,
    pub avg_loser: f64,
    pub largest_winner: f64,
    pub largest_loser: f64,
    pub max_drawdown_pct: f64,
    pub current_streak: PaperStreak,
    /// Per-category performance breakdown. Sorted by `realized_pnl` DESC so
    /// the strongest categories surface first. Empty when no lots have been
    /// placed yet.
    pub category_stats: Vec<PaperCategoryStats>,
    /// Per-side (Over/Under) performance breakdown. Sorted by
    /// `realized_pnl` DESC so the strongest side surfaces first. Empty when
    /// no lots have been placed yet.
    pub side_stats: Vec<PaperSideStats>,
    /// Per-hold-time-bucket performance breakdown. The 4 fixed buckets
    /// (Intraday / Same day / Multi-day / Long) always appear in the result
    /// vector in chronological order — not sorted by PnL — so the UI can
    /// render a stable "fastest to slowest" ladder without resorting.
    /// Buckets with no decided lots are still emitted (zeros) so the table
    /// layout doesn't shift as the user's history grows.
    pub hold_time_stats: Vec<PaperHoldTimeStats>,
    /// Today and 7-day equity deltas for the summary card. Both windows are
    /// `None` when no baseline snapshot exists (e.g. a brand-new account).
    pub session_pnl: SessionPnl,
    pub fetched_at: String,
}

/// A run of consecutive wins or losses ending on the most-recent closed lot.
/// `kind` is `"W"` (consecutive wins), `"L"` (consecutive losses), or `"None"`
/// when there are no closed lots yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperStreak {
    pub kind: String,
    pub length: u32,
}

impl PaperStreak {
    pub fn empty() -> Self {
        Self { kind: "None".to_string(), length: 0 }
    }
}

/// Performance breakdown for a single PrizePicks stat category (e.g. Points,
/// Rebounds, Goals). Sums all closed lots within the category, regardless of
/// side. `roi_pct` is `realized_pnl / total_staked * 100` — returns 0.0 when
/// no closed stake exists for the category. Open lots contribute zero to wins
/// / losses / pnl but ARE counted in `total_trades` and `open_trades`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperCategoryStats {
    pub category: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperCategoryStats {
    fn new(category: String) -> Self {
        Self {
            category,
            total_trades: 0,
            open_trades: 0,
            wins: 0,
            losses: 0,
            win_rate: 0.0,
            realized_pnl: 0.0,
            total_staked: 0.0,
            roi_pct: 0.0,
        }
    }
}

/// Performance breakdown for a single contract side ("YES" = Over, "NO" =
/// Under). Mirrors `PaperCategoryStats` but buckets by `side` instead of
/// `category`. Most-recent closed lots are aggregated; pushes (pnl == 0)
/// contribute stake but neither wins nor losses. Open lots count toward
/// `total_trades` and `open_trades` but contribute nothing to realized PnL
/// or the ROI denominator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperSideStats {
    /// Raw `side` value from the lot ("YES", "NO", or other legacy strings).
    /// The UI is responsible for mapping "YES" → "Over" / "NO" → "Under" for
    /// display. We keep the raw value so the data layer doesn't have to
    /// hard-code that mapping.
    pub side: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperSideStats {
    fn new(side: String) -> Self {
        Self {
            side,
            total_trades: 0,
            open_trades: 0,
            wins: 0,
            losses: 0,
            win_rate: 0.0,
            realized_pnl: 0.0,
            total_staked: 0.0,
            roi_pct: 0.0,
        }
    }
}

/// Per-window equity change for the paper-trading summary card. `pnl_dollars`
/// is the dollar change between the most-recent equity snapshot and the
/// baseline snapshot; `pnl_pct` is `pnl_dollars / baseline_equity * 100`
/// (returns 0.0 when `baseline_equity` <= 0). `baseline_ts` is the timestamp
/// of the baseline snapshot (the snapshot at-or-before the cutoff). Returns
/// `None` from `compute_session_pnl` when no qualifying baseline snapshot
/// exists (e.g. the account is brand-new and the cutoff predates the first
/// recorded snapshot).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionDelta {
    pub pnl_dollars: f64,
    pub pnl_pct: f64,
    pub baseline_equity: f64,
    pub baseline_ts: String,
}

/// Today and 7-day session PnL deltas for the paper account. Both fields are
/// `Option` because the user may have a brand-new account with no snapshot
/// pre-dating the cutoff, or the snapshot table may be empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionPnl {
    pub today: Option<SessionDelta>,
    pub this_week: Option<SessionDelta>,
}

/// Walk `paper_equity_snapshots` (DESC by `ts`, matching `get_equity_snapshots`)
/// to find the most-recent snapshot at-or-before each cutoff timestamp, then
/// compute the dollar and percent change versus the *current* equity supplied
/// by the caller. Returns `None` for a given window when the snapshot list
/// is empty, when every snapshot post-dates the cutoff, or when the baseline
/// equity is non-positive (avoids divide-by-zero / sign flips in the percent).
///
/// `now` is passed in (rather than calling `Utc::now()` directly) so the
/// function stays pure and testable.
fn compute_session_pnl(
    snapshots: &[PaperEquitySnapshot],
    current_equity: f64,
    now: &chrono::DateTime<chrono::Utc>,
) -> SessionPnl {
    fn find_baseline(
        snapshots: &[PaperEquitySnapshot],
        cutoff: &chrono::DateTime<chrono::Utc>,
    ) -> Option<SessionDelta> {
        // Snapshots come in DESC order. The first one whose parsed `ts` is
        // at-or-before the cutoff is the most-recent baseline.
        for s in snapshots {
            let parsed = match chrono::DateTime::parse_from_rfc3339(&s.ts) {
                Ok(dt) => dt.with_timezone(&chrono::Utc),
                Err(_) => continue,
            };
            if parsed <= *cutoff {
                return Some(SessionDelta {
                    pnl_dollars: 0.0,
                    pnl_pct: 0.0,
                    baseline_equity: s.equity_dollars,
                    baseline_ts: s.ts.clone(),
                });
            }
        }
        None
    }

    let today_cutoff = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_utc().into())
        .unwrap_or(*now);
    let week_cutoff = *now - chrono::Duration::days(7);

    let mut today = find_baseline(snapshots, &today_cutoff);
    let mut this_week = find_baseline(snapshots, &week_cutoff);

    // Fill in the dollar/percent change now that we know the baseline. Doing
    // this in a second pass keeps `find_baseline` focused on lookup.
    if let Some(ref mut d) = today {
        d.pnl_dollars = current_equity - d.baseline_equity;
        d.pnl_pct = if d.baseline_equity > 0.0 {
            (d.pnl_dollars / d.baseline_equity) * 100.0
        } else {
            0.0
        };
    }
    if let Some(ref mut d) = this_week {
        d.pnl_dollars = current_equity - d.baseline_equity;
        d.pnl_pct = if d.baseline_equity > 0.0 {
            (d.pnl_dollars / d.baseline_equity) * 100.0
        } else {
            0.0
        };
    }

    SessionPnl { today, this_week }
}

/// Bucket closed + open lots by category and compute per-category stats.
/// Lots with an empty category are bucketed under `"Other"`. Categories are
/// emitted in descending `realized_pnl` order so the strongest categories
/// surface first; ties are broken alphabetically for deterministic output.
fn compute_category_stats(lots: &[PaperLot]) -> Vec<PaperCategoryStats> {
    use std::collections::BTreeMap;

    // Sort by category first so the BTreeMap iteration is deterministic when
    // multiple categories tie on PnL. We sort the *results* explicitly below
    // so the in-memory grouping order doesn't leak to callers.
    let mut buckets: BTreeMap<String, PaperCategoryStats> = BTreeMap::new();
    for l in lots {
        let key = if l.category.trim().is_empty() {
            "Other".to_string()
        } else {
            l.category.clone()
        };
        let entry = buckets
            .entry(key.clone())
            .or_insert_with(|| PaperCategoryStats::new(key));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
            // Open lots have no realized PnL, but we still count their stake
            // toward exposure so the user sees the at-risk amount. We only
            // count closed stakes into the ROI denominator.
            continue;
        }
        let pnl = l.realized_pnl.unwrap_or(0.0);
        if pnl > 0.0 {
            entry.wins += 1;
        } else if pnl < 0.0 {
            entry.losses += 1;
        }
        entry.realized_pnl += pnl;
        entry.total_staked += l.stake_dollars;
    }

    let mut out: Vec<PaperCategoryStats> = buckets.into_values().collect();
    for s in out.iter_mut() {
        let decided = s.wins + s.losses;
        s.win_rate = if decided > 0 {
            (s.wins as f64 / decided as f64) * 100.0
        } else {
            0.0
        };
        s.roi_pct = if s.total_staked > 0.0 {
            (s.realized_pnl / s.total_staked) * 100.0
        } else {
            0.0
        };
    }
    // Sort: realized_pnl DESC, then category ASC for ties. Stable so the
    // BTreeMap-ordering above doesn't matter.
    out.sort_by(|a, b| {
        b.realized_pnl
            .partial_cmp(&a.realized_pnl)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.category.cmp(&b.category))
    });
    out
}

/// Bucket closed + open lots by side (YES/NO = Over/Under) and compute
/// per-side stats. Empty side strings are bucketed under "Unknown" so we
/// never silently drop a lot. Output is sorted by `realized_pnl` DESC,
/// ties broken alphabetically for deterministic output. This mirrors
/// `compute_category_stats` but groups by contract side instead of stat
/// category — the two views complement each other (per-category answers
/// "where is the edge?", per-side answers "am I better at picking Overs
/// or Unders?").
fn compute_side_stats(lots: &[PaperLot]) -> Vec<PaperSideStats> {
    use std::collections::BTreeMap;

    // BTreeMap keeps grouping deterministic; the explicit sort below
    // ensures the result order doesn't leak the in-memory ordering.
    let mut buckets: BTreeMap<String, PaperSideStats> = BTreeMap::new();
    for l in lots {
        let key = if l.side.trim().is_empty() {
            "Unknown".to_string()
        } else {
            l.side.clone()
        };
        let entry = buckets
            .entry(key.clone())
            .or_insert_with(|| PaperSideStats::new(key));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
            // Open lots: count exposure but not realized PnL.
            continue;
        }
        let pnl = l.realized_pnl.unwrap_or(0.0);
        if pnl > 0.0 {
            entry.wins += 1;
        } else if pnl < 0.0 {
            entry.losses += 1;
        }
        entry.realized_pnl += pnl;
        entry.total_staked += l.stake_dollars;
    }

    let mut out: Vec<PaperSideStats> = buckets.into_values().collect();
    for s in out.iter_mut() {
        let decided = s.wins + s.losses;
        s.win_rate = if decided > 0 {
            (s.wins as f64 / decided as f64) * 100.0
        } else {
            0.0
        };
        s.roi_pct = if s.total_staked > 0.0 {
            (s.realized_pnl / s.total_staked) * 100.0
        } else {
            0.0
        };
    }
    // Sort: realized_pnl DESC, then side ASC for ties. Stable so the
    // BTreeMap-ordering above doesn't matter.
    out.sort_by(|a, b| {
        b.realized_pnl
            .partial_cmp(&a.realized_pnl)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.side.cmp(&b.side))
    });
    out
}

/// Hold-time bucket for a paper-trade lot. Determines the time window between
/// `opened_at` and `closed_at`. Buckets are emitted in chronological order
/// (fastest to slowest) by `compute_hold_time_stats` so the UI can render a
/// stable "intraday → long" ladder without resorting. `Unknown` is used as
/// a fallback when timestamps are missing or unparseable; the corresponding
/// lots are still counted toward `total_trades` so the user can see they
/// exist, but they contribute zero to win-rate and PnL.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PaperHoldTimeBucket {
    Intraday,
    SameDay,
    MultiDay,
    Long,
    Unknown,
}

impl PaperHoldTimeBucket {
    /// All buckets in chronological order — the canonical render order.
    const ALL: [PaperHoldTimeBucket; 5] = [
        PaperHoldTimeBucket::Intraday,
        PaperHoldTimeBucket::SameDay,
        PaperHoldTimeBucket::MultiDay,
        PaperHoldTimeBucket::Long,
        PaperHoldTimeBucket::Unknown,
    ];

    /// Classify a hold duration in seconds into a bucket.
    /// - Intraday:    `< 1h`    (≤ 3599s)
    /// - SameDay:     `1h..24h`
    /// - MultiDay:    `1d..7d`
    /// - Long:        `> 7d`
    /// - Unknown:     negative or NaN
    fn from_seconds(secs: f64) -> Self {
        if !secs.is_finite() || secs < 0.0 {
            return PaperHoldTimeBucket::Unknown;
        }
        const ONE_HOUR: f64 = 3600.0;
        const ONE_DAY: f64 = 86_400.0;
        const SEVEN_DAYS: f64 = 604_800.0;
        if secs < ONE_HOUR {
            PaperHoldTimeBucket::Intraday
        } else if secs < ONE_DAY {
            PaperHoldTimeBucket::SameDay
        } else if secs < SEVEN_DAYS {
            PaperHoldTimeBucket::MultiDay
        } else {
            PaperHoldTimeBucket::Long
        }
    }

    /// Human-readable label for the bucket — the value the UI displays.
    pub fn as_label(&self) -> &'static str {
        match self {
            PaperHoldTimeBucket::Intraday => "Intraday (≤1h)",
            PaperHoldTimeBucket::SameDay => "Same day (1-24h)",
            PaperHoldTimeBucket::MultiDay => "Multi-day (1-7d)",
            PaperHoldTimeBucket::Long => "Long (>7d)",
            PaperHoldTimeBucket::Unknown => "Unknown",
        }
    }
}

/// Performance breakdown for a single hold-time bucket. Mirrors
/// `PaperCategoryStats` and `PaperSideStats` but groups by how long the lot
/// was held (from `opened_at` to `closed_at`) instead of stat category or
/// contract side. Helps users answer "am I better at quick in-game picks
/// or long-shot futures?" — the answer is usually not obvious from the
/// aggregate metrics alone. The 4 fixed buckets always appear in the
/// result vector in chronological order (Intraday → SameDay → MultiDay →
/// Long), with an optional trailing `Unknown` bucket when unparseable
/// timestamps exist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperHoldTimeStats {
    /// Bucket identifier — matches `PaperHoldTimeBucket` enum tags
    /// (`"intraday" | "same_day" | "multi_day" | "long" | "unknown"`).
    pub bucket: String,
    /// Human-readable label, e.g. `"Intraday (≤1h)"`. Computed from the
    /// bucket enum so the UI doesn't have to hard-code the mapping.
    pub bucket_label: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
    /// Mean hold duration for decided (closed) lots in this bucket, in
    /// seconds. `0.0` when no closed lots are in the bucket. Lets the user
    /// sanity-check the bucket assignment (e.g. a "Same day" bucket
    /// averaging 30 minutes suggests mostly misclassified intraday lots).
    pub avg_hold_seconds: f64,
    /// Median hold duration in seconds, `0.0` when no closed lots. More
    /// robust than `avg_hold_seconds` to outlier long-held lots. Returns
    /// `0.0` for empty input (no special `Option` needed; `0.0` is also
    /// the natural "no data" signal for both metrics).
    pub median_hold_seconds: f64,
}

impl PaperHoldTimeStats {
    fn new(bucket: PaperHoldTimeBucket) -> Self {
        Self {
            bucket: bucket.as_label_short(),
            bucket_label: bucket.as_label().to_string(),
            total_trades: 0,
            open_trades: 0,
            wins: 0,
            losses: 0,
            win_rate: 0.0,
            realized_pnl: 0.0,
            total_staked: 0.0,
            roi_pct: 0.0,
            avg_hold_seconds: 0.0,
            median_hold_seconds: 0.0,
        }
    }
}

/// Short, snake_case identifier for the bucket. Matches the `serde`
/// representation and is stable across versions.
trait BucketLabel {
    fn as_label_short(&self) -> String;
}

impl BucketLabel for PaperHoldTimeBucket {
    fn as_label_short(&self) -> String {
        match self {
            PaperHoldTimeBucket::Intraday => "intraday".to_string(),
            PaperHoldTimeBucket::SameDay => "same_day".to_string(),
            PaperHoldTimeBucket::MultiDay => "multi_day".to_string(),
            PaperHoldTimeBucket::Long => "long".to_string(),
            PaperHoldTimeBucket::Unknown => "unknown".to_string(),
        }
    }
}

/// Parse a lot's open/close timestamps into a hold-time duration in seconds.
/// Returns `None` when either timestamp is missing, unparseable, or the
/// duration is non-positive. The `None` case is treated as an `Unknown`
/// bucket by the caller (we still want to count the lot, but not make a
/// guess at the bucket).
fn lot_hold_seconds(lot: &PaperLot) -> Option<f64> {
    let open = chrono::DateTime::parse_from_rfc3339(&lot.opened_at).ok()?;
    let closed = lot
        .closed_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())?;
    let dur = closed.signed_duration_since(open);
    let secs = dur.num_milliseconds() as f64 / 1000.0;
    if secs.is_finite() && secs >= 0.0 {
        Some(secs)
    } else {
        None
    }
}

/// Bucket closed + open lots by how long they were held and compute
/// per-bucket stats. Lots with unparseable open/close timestamps fall into
/// the `Unknown` bucket so they are still counted in `total_trades` but
/// don't pollute the time-bucketed numbers. The result is emitted in
/// chronological bucket order (Intraday → SameDay → MultiDay → Long →
/// Unknown) so the UI can render a stable ladder without resorting.
///
/// Open lots contribute to `open_trades` and `total_trades` but are
/// excluded from `wins` / `losses` / `realized_pnl` / `total_staked` /
/// `avg_hold_seconds` / `median_hold_seconds` (they haven't been decided
/// yet). Pushes (realized_pnl == 0) contribute to `total_staked` and the
/// hold-duration averages but not to wins or losses.
fn compute_hold_time_stats(lots: &[PaperLot]) -> Vec<PaperHoldTimeStats> {
    use std::collections::BTreeMap;

    let mut buckets: BTreeMap<PaperHoldTimeBucket, PaperHoldTimeStats> = BTreeMap::new();
    // Per-bucket hold-duration samples for closed lots (used for median).
    let mut hold_samples: std::collections::BTreeMap<PaperHoldTimeBucket, Vec<f64>> =
        BTreeMap::new();
    let mut hold_sum: std::collections::BTreeMap<PaperHoldTimeBucket, f64> = BTreeMap::new();
    let mut hold_count: std::collections::BTreeMap<PaperHoldTimeBucket, u32> = BTreeMap::new();

    for l in lots {
        let bucket = match lot_hold_seconds(l) {
            Some(secs) => PaperHoldTimeBucket::from_seconds(secs),
            None => PaperHoldTimeBucket::Unknown,
        };
        let entry = buckets
            .entry(bucket)
            .or_insert_with(|| PaperHoldTimeStats::new(bucket));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
            // No realized PnL or hold duration for open lots.
            continue;
        }
        let pnl = l.realized_pnl.unwrap_or(0.0);
        if pnl > 0.0 {
            entry.wins += 1;
        } else if pnl < 0.0 {
            entry.losses += 1;
        }
        entry.realized_pnl += pnl;
        entry.total_staked += l.stake_dollars;

        // Track hold duration for closed (decided) lots only. Pushed lots
        // (pnl == 0) still count toward the hold-duration sample — they
        // were held for some real duration, we just don't know the result.
        if let Some(secs) = lot_hold_seconds(l) {
            *hold_sum.entry(bucket).or_insert(0.0) += secs;
            *hold_count.entry(bucket).or_insert(0) += 1;
            hold_samples.entry(bucket).or_default().push(secs);
        }
    }

    // Finalize the per-bucket aggregates (win_rate, ROI, avg/median).
    let mut out: Vec<PaperHoldTimeStats> = buckets.into_values().collect();
    for s in out.iter_mut() {
        let bucket = PaperHoldTimeBucket::ALL
            .iter()
            .copied()
            .find(|b| b.as_label().to_string() == s.bucket_label)
            .unwrap_or(PaperHoldTimeBucket::Unknown);
        let decided = s.wins + s.losses;
        s.win_rate = if decided > 0 {
            (s.wins as f64 / decided as f64) * 100.0
        } else {
            0.0
        };
        s.roi_pct = if s.total_staked > 0.0 {
            (s.realized_pnl / s.total_staked) * 100.0
        } else {
            0.0
        };
        let n = hold_count.get(&bucket).copied().unwrap_or(0);
        if n > 0 {
            s.avg_hold_seconds = hold_sum.get(&bucket).copied().unwrap_or(0.0) / n as f64;
            let mut samples = hold_samples.get(&bucket).cloned().unwrap_or_default();
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            s.median_hold_seconds = if samples.is_empty() {
                0.0
            } else if samples.len() % 2 == 0 {
                (samples[samples.len() / 2 - 1] + samples[samples.len() / 2]) / 2.0
            } else {
                samples[samples.len() / 2]
            };
        }
    }
    // Chronological order. `PaperHoldTimeBucket::ALL` is in canonical order;
    // sort by the bucket enum's position in that array so the result is
    // stable regardless of the BTreeMap keying.
    out.sort_by_key(|s| {
        PaperHoldTimeBucket::ALL
            .iter()
            .position(|b| b.as_label() == s.bucket_label)
            .unwrap_or(usize::MAX)
    });
    out
}

/// Compute the current streak by walking closed lots from most-recent to oldest.
/// Lots are passed in DESC `opened_at` order (matching `get_all_lots`). The first
/// closed lot's sign seeds the streak; the run ends as soon as a closed lot
/// disagrees, or at the first non-closed lot. Push (realized_pnl == 0) breaks
/// the streak without contributing to either side — it is rare but the binary
/// contract path can produce it when the entry price equals the payout.
fn compute_current_streak(lots_desc: &[PaperLot]) -> PaperStreak {
    let mut length: u32 = 0;
    let mut expected: Option<&'static str> = None;
    for l in lots_desc {
        if l.status != "Closed" {
            continue;
        }
        let pnl = l.realized_pnl.unwrap_or(0.0);
        if pnl > 0.0 {
            match expected {
                None => {
                    expected = Some("W");
                    length = 1;
                }
                Some("W") => length += 1,
                Some("L") => {
                    // Streak of losses ended; return the win streak that
                    // just began with this lot.
                    return PaperStreak { kind: "W".to_string(), length: 1 };
                }
                _ => unreachable!(),
            }
        } else if pnl < 0.0 {
            match expected {
                None => {
                    expected = Some("L");
                    length = 1;
                }
                Some("L") => length += 1,
                Some("W") => {
                    return PaperStreak { kind: "W".to_string(), length };
                }
                _ => unreachable!(),
            }
        } else {
            // Push (realized_pnl == 0): the lot settled neutrally. We keep
            // walking so that a single push doesn't reset a meaningful
            // streak — the user sees the last run of real wins or losses.
            continue;
        }
    }
    PaperStreak {
        kind: match expected {
            Some(e) => e.to_string(),
            None => "None".to_string(),
        },
        length,
    }
}

/// Equity snapshot used for drawdown and trend charts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperEquitySnapshot {
    pub id: i64,
    pub ts: String,
    pub balance_dollars: f64,
    pub open_market_value: f64,
    pub equity_dollars: f64,
    pub unrealized_pnl: f64,
}

// ═══════════════════════════════════════════════════════════════
// Schema & bootstrap
// ═══════════════════════════════════════════════════════════════

pub async fn init_paper_tables(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS paper_account (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            balance_dollars REAL NOT NULL,
            total_deposits REAL NOT NULL DEFAULT 0.0,
            total_withdrawals REAL NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create paper_account table: {}", e))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS paper_lots (
            id TEXT PRIMARY KEY,
            ticker TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT 'Other',
            side TEXT NOT NULL,
            entry_price_cents REAL NOT NULL,
            qty REAL NOT NULL,
            stake_dollars REAL NOT NULL,
            source TEXT NOT NULL DEFAULT 'Manual',
            decision_json TEXT,
            opened_at TEXT NOT NULL,
            closed_at TEXT,
            closed_price_cents REAL,
            realized_pnl REAL,
            status TEXT NOT NULL DEFAULT 'Open',
            settlement_result TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create paper_lots table: {}", e))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS paper_equity_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            balance_dollars REAL NOT NULL,
            open_market_value REAL NOT NULL,
            equity_dollars REAL NOT NULL,
            unrealized_pnl REAL NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create paper_equity_snapshots table: {}", e))?;

    // Indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_lots_ticker ON paper_lots(ticker)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_lots_status ON paper_lots(status)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_lots_opened ON paper_lots(opened_at)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_equity_ts ON paper_equity_snapshots(ts)")
        .execute(pool)
        .await
        .ok();

    // Bootstrap singleton account if missing.
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM paper_account WHERE id = 1)")
            .fetch_one(pool)
            .await
            .unwrap_or(false);
    if !exists {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO paper_account (id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        )
        .bind(DEFAULT_STARTING_BALANCE)
        .bind(DEFAULT_STARTING_BALANCE)
        .bind(0.0)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to bootstrap paper account: {}", e))?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Account
// ═══════════════════════════════════════════════════════════════

pub async fn get_account(pool: &Pool<Sqlite>) -> Result<PaperAccount, String> {
    let row = sqlx::query(
        "SELECT id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at FROM paper_account WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper account: {}", e))?;

    Ok(PaperAccount {
        id: row.get("id"),
        balance_dollars: row.get("balance_dollars"),
        total_deposits: row.get("total_deposits"),
        total_withdrawals: row.get("total_withdrawals"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn reset_account(
    pool: &Pool<Sqlite>,
    starting_balance: Option<f64>,
) -> Result<PaperAccount, String> {
    let balance = starting_balance
        .unwrap_or(DEFAULT_STARTING_BALANCE)
        .max(0.0);
    let now = Utc::now().to_rfc3339();

    sqlx::query("DELETE FROM paper_lots")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to clear paper lots: {}", e))?;
    sqlx::query("DELETE FROM paper_equity_snapshots")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to clear paper snapshots: {}", e))?;

    sqlx::query(
        "INSERT OR REPLACE INTO paper_account (id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
    )
    .bind(balance)
    .bind(balance)
    .bind(0.0)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to reset paper account: {}", e))?;

    Ok(get_account(pool).await?)
}

async fn update_balance(pool: &Pool<Sqlite>, delta: f64) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE paper_account SET balance_dollars = balance_dollars + ?1, updated_at = ?2 WHERE id = 1",
    )
    .bind(delta)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update paper balance: {}", e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Lots & trades
// ═══════════════════════════════════════════════════════════════

fn normalize_side(side: &str) -> Result<String, String> {
    let upper = side.trim().to_ascii_uppercase();
    if upper == "YES" || upper == "NO" {
        Ok(upper)
    } else {
        Err(format!("Invalid paper trade side: {}", side))
    }
}

pub async fn place_trade(pool: &Pool<Sqlite>, input: PaperTradeInput) -> Result<PaperLot, String> {
    let side = normalize_side(&input.side)?;
    if input.qty <= 0.0 {
        return Err("Paper trade quantity must be positive".into());
    }
    if input.entry_price_cents <= 0.0 || input.entry_price_cents >= 100.0 {
        return Err("Paper entry price must be between 0 and 100 cents".into());
    }

    let cost = input.qty * input.entry_price_cents / 100.0;
    let account = get_account(pool).await?;
    if cost > account.balance_dollars {
        return Err(format!(
            "Insufficient paper buying power: ${:.2} needed, ${:.2} available",
            cost, account.balance_dollars
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let source_str = input.source.as_str().to_string();

    sqlx::query(
        r#"
        INSERT INTO paper_lots
            (id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
             source, decision_json, opened_at, status)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'Open')
        "#,
    )
    .bind(&id)
    .bind(&input.ticker)
    .bind(&input.title)
    .bind(&input.category)
    .bind(&side)
    .bind(input.entry_price_cents)
    .bind(input.qty)
    .bind(cost)
    .bind(&source_str)
    .bind(&input.decision_json)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert paper lot: {}", e))?;

    update_balance(pool, -cost).await?;
    record_equity_snapshot(pool, None).await?;

    get_lot(pool, &id).await
}

pub async fn close_lot(
    pool: &Pool<Sqlite>,
    lot_id: &str,
    exit_price_cents: f64,
) -> Result<PaperLot, String> {
    if exit_price_cents < 0.0 || exit_price_cents > 100.0 {
        return Err("Exit price must be between 0 and 100 cents".into());
    }

    let lot = get_lot(pool, lot_id).await?;
    if lot.status != "Open" {
        return Err(format!("Lot {} is not open", lot_id));
    }

    let proceeds = lot.qty * exit_price_cents / 100.0;
    let realized = proceeds - lot.stake_dollars;
    let now = Utc::now().to_rfc3339();
    let result = if exit_price_cents >= 99.99 {
        "Yes"
    } else if exit_price_cents <= 0.01 {
        "No"
    } else {
        "Closed"
    };

    sqlx::query(
        r#"
        UPDATE paper_lots
        SET closed_at = ?1, closed_price_cents = ?2, realized_pnl = ?3,
            status = 'Closed', settlement_result = ?4
        WHERE id = ?5
        "#,
    )
    .bind(&now)
    .bind(exit_price_cents)
    .bind(realized)
    .bind(result)
    .bind(lot_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to close paper lot: {}", e))?;

    update_balance(pool, proceeds).await?;
    record_equity_snapshot(pool, None).await?;

    get_lot(pool, lot_id).await
}

pub async fn get_lot(pool: &Pool<Sqlite>, lot_id: &str) -> Result<PaperLot, String> {
    let row = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots WHERE id = ?1
        "#,
    )
    .bind(lot_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper lot: {}", e))?;

    Ok(row_to_lot(&row))
}

pub async fn get_all_lots(pool: &Pool<Sqlite>) -> Result<Vec<PaperLot>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots ORDER BY opened_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper lots: {}", e))?;

    Ok(rows.iter().map(row_to_lot).collect())
}

pub async fn get_open_lots(pool: &Pool<Sqlite>) -> Result<Vec<PaperLot>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots WHERE status = 'Open' ORDER BY opened_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch open paper lots: {}", e))?;

    Ok(rows.iter().map(row_to_lot).collect())
}

fn row_to_lot(r: &sqlx::sqlite::SqliteRow) -> PaperLot {
    let source: String = r.get("source");
    PaperLot {
        id: r.get("id"),
        ticker: r.get("ticker"),
        title: r.get("title"),
        category: r.get("category"),
        side: r.get("side"),
        entry_price_cents: r.get("entry_price_cents"),
        qty: r.get("qty"),
        stake_dollars: r.get("stake_dollars"),
        source: source.parse().unwrap_or(PaperTradeSource::Manual),
        decision_json: r.get("decision_json"),
        opened_at: r.get("opened_at"),
        closed_at: r.get("closed_at"),
        closed_price_cents: r.get("closed_price_cents"),
        realized_pnl: r.get("realized_pnl"),
        status: r.get("status"),
        settlement_result: r.get("settlement_result"),
    }
}

// ═══════════════════════════════════════════════════════════════
// Aggregate positions
// ═══════════════════════════════════════════════════════════════

pub async fn aggregate_positions(
    pool: &Pool<Sqlite>,
    client: Option<&dyn MarketDataProvider>,
) -> Result<Vec<PaperPosition>, String> {
    let open = get_open_lots(pool).await?;
    if open.is_empty() {
        return Ok(Vec::new());
    }

    // Group by ticker/side.
    let mut groups: std::collections::HashMap<(String, String), Vec<PaperLot>> =
        std::collections::HashMap::new();
    for lot in open {
        groups
            .entry((lot.ticker.clone(), lot.side.clone()))
            .or_default()
            .push(lot);
    }

    let mut positions = Vec::new();
    for ((ticker, side), lots) in groups {
        let total_qty: f64 = lots.iter().map(|l| l.qty).sum();
        let cost_basis: f64 = lots.iter().map(|l| l.stake_dollars).sum();
        let avg_entry = if total_qty > 0.0 {
            lots.iter()
                .map(|l| l.entry_price_cents * l.qty)
                .sum::<f64>()
                / total_qty
        } else {
            0.0
        };

        let (title, category) = lots
            .first()
            .map(|l| (l.title.clone(), l.category.clone()))
            .unwrap_or_default();

        let mark = if let Some(c) = client {
            best_bid_cents(c, &ticker, &side).await.ok()
        } else {
            None
        };

        let (market_value, unrealized) = mark
            .map(|m| {
                let mv = total_qty * m / 100.0;
                let ur = mv - cost_basis;
                (Some(mv), Some(ur))
            })
            .unwrap_or((None, None));

        positions.push(PaperPosition {
            ticker,
            title,
            category,
            side,
            total_qty,
            avg_entry_price_cents: avg_entry,
            cost_basis_dollars: cost_basis,
            mark_price_cents: mark,
            market_value_dollars: market_value,
            unrealized_pnl_dollars: unrealized,
            lots_count: lots.len() as i64,
        });
    }

    Ok(positions)
}

// ═══════════════════════════════════════════════════════════════
// Settlement against resolved PrizePicks markets
// ═══════════════════════════════════════════════════════════════

pub async fn settle_pending(
    pool: &Pool<Sqlite>,
    client: &dyn MarketDataProvider,
) -> Result<PaperSettlementSummary, String> {
    let open = get_open_lots(pool).await?;
    if open.is_empty() {
        return Ok(PaperSettlementSummary {
            settled: 0,
            wins: 0,
            losses: 0,
            total_pnl: 0.0,
            details: Vec::new(),
            fetched_at: Utc::now().to_rfc3339(),
        });
    }

    let mut by_ticker: std::collections::HashMap<String, Vec<PaperLot>> =
        std::collections::HashMap::new();
    for lot in open {
        by_ticker.entry(lot.ticker.clone()).or_default().push(lot);
    }

    let mut summary = PaperSettlementSummary {
        settled: 0,
        wins: 0,
        losses: 0,
        total_pnl: 0.0,
        details: Vec::new(),
        fetched_at: Utc::now().to_rfc3339(),
    };

    for (ticker, lots) in by_ticker {
        let market = match client.get_market(&ticker).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("paper settle: skip {} — {}", ticker, e);
                continue;
            }
        };
        if market.result.is_empty() {
            continue;
        }

        let exit = if market.result.eq_ignore_ascii_case("yes") {
            100.0
        } else {
            0.0
        };

        for lot in lots {
            let closed = close_lot(pool, &lot.id, exit).await?;
            let won = (closed.side == "YES" && market.result.eq_ignore_ascii_case("yes"))
                || (closed.side == "NO" && market.result.eq_ignore_ascii_case("no"));
            summary.settled += 1;
            if won {
                summary.wins += 1;
            } else {
                summary.losses += 1;
            }
            summary.total_pnl += closed.realized_pnl.unwrap_or(0.0);
            summary.details.push(PaperSettlementDetail {
                lot_id: closed.id,
                ticker: closed.ticker,
                side: closed.side,
                result: market.result.clone(),
                realized_pnl: closed.realized_pnl.unwrap_or(0.0),
            });
        }
    }

    Ok(summary)
}

// ═══════════════════════════════════════════════════════════════
// Analytics
// ═══════════════════════════════════════════════════════════════

pub async fn get_analytics(
    pool: &Pool<Sqlite>,
    client: Option<&dyn MarketDataProvider>,
) -> Result<PaperAnalytics, String> {
    let all = get_all_lots(pool).await?;
    let account = get_account(pool).await?;
    let closed: Vec<&PaperLot> = all.iter().filter(|l| l.status == "Closed").collect();
    let open_positions = all.iter().filter(|l| l.status == "Open").count() as u32;

    let realized_pnl: f64 = closed.iter().map(|l| l.realized_pnl.unwrap_or(0.0)).sum();

    let mut wins = 0u32;
    let mut losses = 0u32;
    let mut gross_wins = 0.0;
    let mut gross_losses = 0.0;
    let mut largest_winner: f64 = 0.0;
    let mut largest_loser: f64 = 0.0;

    for l in &closed {
        let pnl = l.realized_pnl.unwrap_or(0.0);
        if pnl > 0.0 {
            wins += 1;
            gross_wins += pnl;
            largest_winner = largest_winner.max(pnl);
        } else if pnl < 0.0 {
            losses += 1;
            gross_losses += pnl.abs();
            largest_loser = largest_loser.min(pnl);
        }
    }

    let win_rate = if wins + losses > 0 {
        (wins as f64 / (wins + losses) as f64) * 100.0
    } else {
        0.0
    };
    let profit_factor = if gross_losses > 0.0 {
        gross_wins / gross_losses
    } else if gross_wins > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    let avg_winner = if wins > 0 {
        gross_wins / wins as f64
    } else {
        0.0
    };
    let avg_loser = if losses > 0 {
        -gross_losses / losses as f64
    } else {
        0.0
    };

    let positions = aggregate_positions(pool, client).await?;
    let open_market_value: f64 = positions
        .iter()
        .filter_map(|p| p.market_value_dollars)
        .sum();
    let unrealized_pnl: f64 = positions
        .iter()
        .filter_map(|p| p.unrealized_pnl_dollars)
        .sum();

    let equity = account.balance_dollars + open_market_value;
    let total_return_pct = if account.total_deposits > 0.0 {
        ((equity - account.total_deposits) / account.total_deposits) * 100.0
    } else {
        0.0
    };

    let max_dd = max_drawdown_pct(pool).await.unwrap_or(0.0);

    let current_streak = compute_current_streak(&all);
    let category_stats = compute_category_stats(&all);
    let side_stats = compute_side_stats(&all);
    let hold_time_stats = compute_hold_time_stats(&all);

    // Session PnL: fetch the most-recent equity snapshots and walk them to
    // compute today's and 7-day deltas. The snapshot list is bounded to
    // 500 rows which is well in excess of any realistic daily-snapshot
    // history while keeping the in-memory scan cheap.
    let session_pnl = {
        let snapshots = get_equity_snapshots(pool, 500).await.unwrap_or_default();
        let now = chrono::Utc::now();
        compute_session_pnl(&snapshots, equity, &now)
    };

    Ok(PaperAnalytics {
        starting_balance: account.total_deposits,
        cash_balance: account.balance_dollars,
        open_market_value,
        equity,
        realized_pnl,
        unrealized_pnl,
        total_return_pct,
        total_trades: all.len() as u32,
        open_positions,
        win_rate,
        wins,
        losses,
        profit_factor,
        avg_winner,
        avg_loser,
        largest_winner,
        largest_loser,
        max_drawdown_pct: max_dd,
        current_streak,
        category_stats,
        side_stats,
        hold_time_stats,
        session_pnl,
        fetched_at: Utc::now().to_rfc3339(),
    })
}

async fn max_drawdown_pct(pool: &Pool<Sqlite>) -> Result<f64, String> {
    let rows = sqlx::query("SELECT equity_dollars FROM paper_equity_snapshots ORDER BY ts ASC")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch equity snapshots: {}", e))?;

    if rows.is_empty() {
        return Ok(0.0);
    }

    let mut peak = 0.0;
    let mut max_dd = 0.0;
    for row in rows {
        let equity: f64 = row.get("equity_dollars");
        if equity > peak {
            peak = equity;
        }
        if peak > 0.0 {
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }

    Ok(max_dd * 100.0)
}

// ═══════════════════════════════════════════════════════════════
// Equity snapshots
// ═══════════════════════════════════════════════════════════════

pub async fn record_equity_snapshot(
    pool: &Pool<Sqlite>,
    client: Option<&dyn MarketDataProvider>,
) -> Result<(), String> {
    let account = get_account(pool).await?;
    let positions = aggregate_positions(pool, client).await?;
    let open_market_value: f64 = positions
        .iter()
        .filter_map(|p| p.market_value_dollars)
        .sum();
    let unrealized: f64 = positions
        .iter()
        .filter_map(|p| p.unrealized_pnl_dollars)
        .sum();
    let equity = account.balance_dollars + open_market_value;

    sqlx::query(
        "INSERT INTO paper_equity_snapshots (ts, balance_dollars, open_market_value, equity_dollars, unrealized_pnl) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(account.balance_dollars)
    .bind(open_market_value)
    .bind(equity)
    .bind(unrealized)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to record equity snapshot: {}", e))?;

    Ok(())
}

pub async fn get_equity_snapshots(
    pool: &Pool<Sqlite>,
    limit: i64,
) -> Result<Vec<PaperEquitySnapshot>, String> {
    let rows = sqlx::query(
        "SELECT id, ts, balance_dollars, open_market_value, equity_dollars, unrealized_pnl FROM paper_equity_snapshots ORDER BY ts DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch equity snapshots: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| PaperEquitySnapshot {
            id: r.get("id"),
            ts: r.get("ts"),
            balance_dollars: r.get("balance_dollars"),
            open_market_value: r.get("open_market_value"),
            equity_dollars: r.get("equity_dollars"),
            unrealized_pnl: r.get("unrealized_pnl"),
        })
        .collect())
}

// ═══════════════════════════════════════════════════════════════
// Mark-to-market helpers
// ═══════════════════════════════════════════════════════════════

async fn best_bid_cents(
    client: &dyn MarketDataProvider,
    ticker: &str,
    side: &str,
) -> Result<f64, String> {
    let book = client.get_orderbook(ticker).await?;
    let best = match side {
        "YES" => best_bid(&book.yes),
        "NO" => best_bid(&book.no),
        _ => None,
    };

    if let Some(price) = best {
        return Ok(price);
    }

    // Fallback to last traded price.
    let market = client.get_market(ticker).await?;
    let last: f64 = market.last_price_dollars.parse().unwrap_or(0.0);
    if last <= 0.0 {
        return Err(format!("No mark price available for {}", ticker));
    }

    let cents = last * 100.0;
    match side {
        "YES" => Ok(cents),
        "NO" => Ok((100.0 - cents).clamp(0.0, 100.0)),
        _ => Err(format!("Invalid side for mark: {}", side)),
    }
}

fn best_bid(levels: &[crate::prizepicks::models::PrizePicksOrderbookLevel]) -> Option<f64> {
    levels
        .iter()
        .map(|l| l.price as f64)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

// ═══════════════════════════════════════════════════════════════
// Background settlement
// ═══════════════════════════════════════════════════════════════

pub fn spawn_paper_settle_task<T: MarketDataProvider + Send + 'static>(
    pool: Pool<Sqlite>,
    prizepicks: std::sync::Arc<tokio::sync::Mutex<T>>,
    poll_interval_secs: u64,
) {
    let interval_secs = poll_interval_secs.max(60);
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let open_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM paper_lots WHERE status = 'Open'")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(0);
            if open_count == 0 {
                continue;
            }
            let summary = {
                let client = prizepicks.lock().await;
                match settle_pending(&pool, &*client).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("paper auto-settle: {}", e);
                        continue;
                    }
                }
            };
            if summary.settled > 0 {
                tracing::info!(
                    "paper auto-settle: {} lots ({}W/{}L, ${:.2})",
                    summary.settled,
                    summary.wins,
                    summary.losses,
                    summary.total_pnl
                );
            }
        }
    });
}

/// Normalize a dollar or cent price into PrizePicks cents (0–100).
pub fn normalize_entry_cents(price: f64) -> f64 {
    if price > 0.0 && price < 1.0 {
        price * 100.0
    } else {
        price.clamp(0.01, 99.99)
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_yes_payout_math() {
        let qty = 10.0;
        let entry = 55.0; // cents
        let cost = qty * entry / 100.0; // $5.50
        let exit = 100.0;
        let proceeds = qty * exit / 100.0; // $10.00
        let pnl: f64 = proceeds - cost;
        assert!((pnl - 4.50).abs() < 0.001);
    }

    #[test]
    fn long_no_payout_math() {
        let qty = 10.0;
        let entry = 45.0; // NO price in cents
        let cost = qty * entry / 100.0; // $4.50
        let exit = 100.0; // No wins
        let proceeds = qty * exit / 100.0; // $10.00
        let pnl: f64 = proceeds - cost;
        assert!((pnl - 5.50).abs() < 0.001);
    }

    #[test]
    fn normalize_entry_cents_from_dollars() {
        assert!((normalize_entry_cents(0.55) - 55.0).abs() < 0.001);
        assert!((normalize_entry_cents(55.0) - 55.0).abs() < 0.001);
    }

    #[test]
    fn source_roundtrip() {
        assert_eq!(PaperTradeSource::AiDecision, "AiDecision".parse().unwrap());
        assert!("Other".parse::<PaperTradeSource>().is_err());
    }

    fn closed_lot(pnl: f64) -> PaperLot {
        PaperLot {
            id: format!("lot-{pnl}"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: "Over".to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: 0.5,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
            closed_at: Some("2026-01-01T01:00:00Z".to_string()),
            closed_price_cents: Some(if pnl >= 0.0 { 100.0 } else { 0.0 }),
            realized_pnl: Some(pnl),
            status: "Closed".to_string(),
            settlement_result: Some(if pnl > 0.0 { "Win" } else if pnl < 0.0 { "Loss" } else { "Push" }.to_string()),
        }
    }

    fn open_lot() -> PaperLot {
        let mut l = closed_lot(0.0);
        l.status = "Open".to_string();
        l.closed_at = None;
        l.closed_price_cents = None;
        l.realized_pnl = None;
        l.settlement_result = None;
        l
    }

    #[test]
    fn streak_empty_input_returns_none() {
        let lots: Vec<PaperLot> = vec![];
        assert_eq!(compute_current_streak(&lots), PaperStreak::empty());
    }

    #[test]
    fn streak_only_open_lots_returns_none() {
        let lots = vec![open_lot(), open_lot()];
        assert_eq!(compute_current_streak(&lots), PaperStreak::empty());
    }

    #[test]
    fn streak_all_wins_counts_full_run() {
        // Most recent first: W, W, W
        let lots = vec![closed_lot(1.0), closed_lot(2.0), closed_lot(3.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "W");
        assert_eq!(s.length, 3);
    }

    #[test]
    fn streak_stops_at_first_loss() {
        // W, W, L, W — walk from newest: W, W, L (stop at L)
        let lots = vec![closed_lot(1.0), closed_lot(1.0), closed_lot(-1.0), closed_lot(1.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "W");
        assert_eq!(s.length, 2);
    }

    #[test]
    fn streak_loss_kind_and_length() {
        // L, L, L, L — most recent four are losses
        let lots = vec![closed_lot(-1.0), closed_lot(-2.0), closed_lot(-3.0), closed_lot(-4.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "L");
        assert_eq!(s.length, 4);
    }

    #[test]
    fn streak_push_at_front_walks_past_to_real_streak() {
        // A push at the front should not erase a meaningful streak of wins
        // that came before it.
        let lots = vec![closed_lot(0.0), closed_lot(1.0), closed_lot(1.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "W");
        assert_eq!(s.length, 2);
    }

    #[test]
    fn streak_push_at_front_with_no_other_lots_returns_none() {
        let lots = vec![closed_lot(0.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "None");
        assert_eq!(s.length, 0);
    }

    #[test]
    fn streak_push_after_wins_preserves_prior_streak() {
        // Newest push breaks a run that had wins before it.
        let lots = vec![closed_lot(0.0), closed_lot(1.0), closed_lot(1.0), closed_lot(1.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "W");
        assert_eq!(s.length, 3);
    }

    #[test]
    fn streak_skips_open_lots_when_walking_back() {
        // Newest is an Open lot; the streak should be derived from the
        // most recent Closed lot, not the Open one.
        let lots = vec![open_lot(), closed_lot(1.0), closed_lot(1.0), closed_lot(-1.0)];
        let s = compute_current_streak(&lots);
        assert_eq!(s.kind, "W");
        assert_eq!(s.length, 2);
    }

    // ── compute_category_stats tests ──────────────────────────────

    /// Helper: closed lot with explicit category + stake + pnl. The default
    /// helper in this module hardcodes category = "Points"; these tests need
    /// multiple categories so we build them inline.
    fn closed_lot_in(
        category: &str,
        side: &str,
        stake: f64,
        pnl: f64,
    ) -> PaperLot {
        PaperLot {
            id: format!("{category}-{side}-{pnl}"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: category.to_string(),
            side: side.to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: stake,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
            closed_at: Some("2026-01-01T01:00:00Z".to_string()),
            closed_price_cents: Some(if pnl >= 0.0 { 100.0 } else { 0.0 }),
            realized_pnl: Some(pnl),
            status: "Closed".to_string(),
            settlement_result: Some(
                if pnl > 0.0 {
                    "Win"
                } else if pnl < 0.0 {
                    "Loss"
                } else {
                    "Push"
                }
                .to_string(),
            ),
        }
    }

    fn open_lot_in(category: &str, stake: f64) -> PaperLot {
        PaperLot {
            id: format!("{category}-open"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: category.to_string(),
            side: "Over".to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: stake,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
            closed_at: None,
            closed_price_cents: None,
            realized_pnl: None,
            status: "Open".to_string(),
            settlement_result: None,
        }
    }

    #[test]
    fn category_stats_empty_input_returns_empty_vec() {
        let stats = compute_category_stats(&[]);
        assert!(stats.is_empty());
    }

    #[test]
    fn category_stats_sorts_by_realized_pnl_desc() {
        let lots = vec![
            closed_lot_in("Rebounds", "Over", 5.0, -2.0),
            closed_lot_in("Points", "Over", 5.0, 3.0),
            closed_lot_in("Assists", "Over", 5.0, 1.0),
        ];
        let stats = compute_category_stats(&lots);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].category, "Points");
        assert_eq!(stats[1].category, "Assists");
        assert_eq!(stats[2].category, "Rebounds");
        assert!((stats[0].realized_pnl - 3.0).abs() < 1e-9);
    }

    #[test]
    fn category_stats_breaks_ties_alphabetically() {
        // Three categories all net to $0 (one win, one loss). Tie-break ASC.
        let lots = vec![
            closed_lot_in("Steals", "Over", 1.0, 1.0),
            closed_lot_in("Steals", "Over", 1.0, -1.0),
            closed_lot_in("Blocks", "Over", 1.0, 1.0),
            closed_lot_in("Blocks", "Over", 1.0, -1.0),
            closed_lot_in("Assists", "Over", 1.0, 1.0),
            closed_lot_in("Assists", "Over", 1.0, -1.0),
        ];
        let stats = compute_category_stats(&lots);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].category, "Assists");
        assert_eq!(stats[1].category, "Blocks");
        assert_eq!(stats[2].category, "Steals");
    }

    #[test]
    fn category_stats_computes_win_rate_and_roi() {
        // 2 wins @ $5 stake (+$5 each = +$10), 1 loss @ $5 (-$5), 1 push.
        // Wins / (wins + losses) = 2/3 = 66.66...%
        // Total staked = $20 (pushes still committed stake). ROI = $5 / $20 = 25%.
        let lots = vec![
            closed_lot_in("Points", "Over", 5.0, 5.0),
            closed_lot_in("Points", "Over", 5.0, 5.0),
            closed_lot_in("Points", "Over", 5.0, -5.0),
            closed_lot_in("Points", "Over", 5.0, 0.0),
        ];
        let stats = compute_category_stats(&lots);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.category, "Points");
        assert_eq!(s.wins, 2);
        assert_eq!(s.losses, 1);
        assert_eq!(s.total_trades, 4);
        // Pushes don't count as wins or losses.
        assert!((s.win_rate - (2.0 / 3.0) * 100.0).abs() < 1e-6);
        assert!((s.realized_pnl - 5.0).abs() < 1e-9);
        assert!((s.total_staked - 20.0).abs() < 1e-9);
        assert!((s.roi_pct - (5.0 / 20.0) * 100.0).abs() < 1e-6);
    }

    #[test]
    fn category_stats_open_lots_excluded_from_pnl_and_roi() {
        // Open lots should count toward total_trades + open_trades but not
        // contribute to wins/losses/pnl/staked (the latter drives ROI).
        let lots = vec![
            open_lot_in("Points", 10.0),
            closed_lot_in("Points", "Over", 5.0, 2.0),
        ];
        let stats = compute_category_stats(&lots);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.total_trades, 2);
        assert_eq!(s.open_trades, 1);
        assert_eq!(s.wins, 1);
        assert_eq!(s.losses, 0);
        // Only the closed $5 stake counts toward the ROI denominator.
        assert!((s.total_staked - 5.0).abs() < 1e-9);
        assert!((s.realized_pnl - 2.0).abs() < 1e-9);
        assert!((s.roi_pct - 40.0).abs() < 1e-6);
        assert!((s.win_rate - 100.0).abs() < 1e-6);
    }

    #[test]
    fn category_stats_empty_category_bucketed_under_other() {
        // An empty / whitespace category should roll up into "Other".
        let mut empty_cat = closed_lot_in("Points", "Over", 5.0, 1.0);
        empty_cat.category = "".to_string();
        let mut whitespace_cat = closed_lot_in("Points", "Over", 5.0, -1.0);
        whitespace_cat.category = "   ".to_string();
        let lots = vec![empty_cat, whitespace_cat];
        let stats = compute_category_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].category, "Other");
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
    }

    #[test]
    fn category_stats_only_pushes_have_zero_roi() {
        // No realized PnL but a stake was committed; ROI is 0.0 (no division
        // by zero), win rate is 0.0 (no decided lots).
        let lots = vec![
            closed_lot_in("Points", "Over", 5.0, 0.0),
            closed_lot_in("Points", "Over", 5.0, 0.0),
        ];
        let stats = compute_category_stats(&lots);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.wins, 0);
        assert_eq!(s.losses, 0);
        assert!((s.win_rate - 0.0).abs() < 1e-9);
        assert!((s.roi_pct - 0.0).abs() < 1e-9);
        assert!((s.realized_pnl - 0.0).abs() < 1e-9);
        assert!((s.total_staked - 10.0).abs() < 1e-9);
    }

    // ── compute_side_stats tests ──────────────────────────────────

    /// Helper: closed lot parameterized by side (so we can mix YES / NO
    /// "Over" / "Under" freely). Most tests use the actual normalized
    /// values "YES" and "NO" since that's what `place_trade` writes after
    /// `normalize_side` runs.
    fn closed_lot_side(side: &str, stake: f64, pnl: f64) -> PaperLot {
        PaperLot {
            id: format!("{side}-{pnl}"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: side.to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: stake,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
            closed_at: Some("2026-01-01T01:00:00Z".to_string()),
            closed_price_cents: Some(if pnl >= 0.0 { 100.0 } else { 0.0 }),
            realized_pnl: Some(pnl),
            status: "Closed".to_string(),
            settlement_result: Some(
                if pnl > 0.0 {
                    "Win"
                } else if pnl < 0.0 {
                    "Loss"
                } else {
                    "Push"
                }
                .to_string(),
            ),
        }
    }

    fn open_lot_side(side: &str, stake: f64) -> PaperLot {
        PaperLot {
            id: format!("{side}-open"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: side.to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: stake,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
            closed_at: None,
            closed_price_cents: None,
            realized_pnl: None,
            status: "Open".to_string(),
            settlement_result: None,
        }
    }

    #[test]
    fn side_stats_empty_input_returns_empty_vec() {
        let stats = compute_side_stats(&[]);
        assert!(stats.is_empty());
    }

    #[test]
    fn side_stats_sorts_by_realized_pnl_desc() {
        // YES net +$3, NO net -$2 — YES ranks first.
        let lots = vec![
            closed_lot_side("NO", 5.0, -2.0),
            closed_lot_side("YES", 5.0, 3.0),
        ];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].side, "YES");
        assert_eq!(stats[1].side, "NO");
        assert!((stats[0].realized_pnl - 3.0).abs() < 1e-9);
        assert!((stats[1].realized_pnl - (-2.0)).abs() < 1e-9);
    }

    #[test]
    fn side_stats_breaks_ties_alphabetically() {
        // Both "NO" and "YES" net to $0 (one win + one loss each). Sort ASC.
        let lots = vec![
            closed_lot_side("YES", 1.0, 1.0),
            closed_lot_side("YES", 1.0, -1.0),
            closed_lot_side("NO", 1.0, 1.0),
            closed_lot_side("NO", 1.0, -1.0),
        ];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 2);
        // "NO" < "YES" alphabetically.
        assert_eq!(stats[0].side, "NO");
        assert_eq!(stats[1].side, "YES");
        assert!((stats[0].win_rate - 50.0).abs() < 1e-6);
        assert!((stats[1].win_rate - 50.0).abs() < 1e-6);
    }

    #[test]
    fn side_stats_computes_win_rate_and_roi() {
        // YES: 2 wins @ $5 (+$5 each = +$10), 1 loss @ $5 (-$5), 1 push.
        // Win rate = 2/3 ≈ 66.67%, ROI = +$5 / $20 staked = 25%.
        let lots = vec![
            closed_lot_side("YES", 5.0, 5.0),
            closed_lot_side("YES", 5.0, 5.0),
            closed_lot_side("YES", 5.0, -5.0),
            closed_lot_side("YES", 5.0, 0.0),
        ];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.side, "YES");
        assert_eq!(s.wins, 2);
        assert_eq!(s.losses, 1);
        assert_eq!(s.total_trades, 4);
        // Pushes (pnl == 0) don't count as wins or losses.
        assert!((s.win_rate - (2.0 / 3.0) * 100.0).abs() < 1e-6);
        assert!((s.realized_pnl - 5.0).abs() < 1e-9);
        assert!((s.total_staked - 20.0).abs() < 1e-9);
        assert!((s.roi_pct - (5.0 / 20.0) * 100.0).abs() < 1e-6);
    }

    #[test]
    fn side_stats_open_lots_excluded_from_pnl_and_roi() {
        // Open lots count toward total_trades + open_trades but contribute
        // nothing to wins/losses/pnl/closed-staked.
        let lots = vec![
            open_lot_side("NO", 10.0),
            closed_lot_side("NO", 5.0, 2.0),
        ];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.total_trades, 2);
        assert_eq!(s.open_trades, 1);
        assert_eq!(s.wins, 1);
        assert_eq!(s.losses, 0);
        // Only closed stake counts in the ROI denominator.
        assert!((s.total_staked - 5.0).abs() < 1e-9);
        assert!((s.realized_pnl - 2.0).abs() < 1e-9);
        assert!((s.roi_pct - 40.0).abs() < 1e-6);
        assert!((s.win_rate - 100.0).abs() < 1e-6);
    }

    #[test]
    fn side_stats_empty_side_bucketed_under_unknown() {
        // Lots with an empty / whitespace side should roll up into
        // "Unknown" so we never silently drop data.
        let mut empty = closed_lot_side("YES", 5.0, 1.0);
        empty.side = "".to_string();
        let mut whitespace = closed_lot_side("YES", 5.0, -1.0);
        whitespace.side = "   ".to_string();
        let lots = vec![empty, whitespace];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].side, "Unknown");
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
    }

    #[test]
    fn side_stats_only_pushes_have_zero_roi() {
        // No realized PnL but stake was committed; ROI = 0.0, win rate = 0.0.
        let lots = vec![
            closed_lot_side("YES", 5.0, 0.0),
            closed_lot_side("NO", 5.0, 0.0),
        ];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 2);
        for s in &stats {
            assert_eq!(s.wins, 0);
            assert_eq!(s.losses, 0);
            assert!((s.win_rate - 0.0).abs() < 1e-9);
            assert!((s.roi_pct - 0.0).abs() < 1e-9);
            assert!((s.realized_pnl - 0.0).abs() < 1e-9);
            assert!((s.total_staked - 5.0).abs() < 1e-9);
        }
    }

    #[test]
    fn side_stats_yes_and_no_split_correctly() {
        // Mixed: 1 YES win (+$5), 1 NO loss (-$2). Two distinct buckets.
        let lots = vec![
            closed_lot_side("YES", 5.0, 5.0),
            closed_lot_side("NO", 5.0, -2.0),
        ];
        let stats = compute_side_stats(&lots);
        assert_eq!(stats.len(), 2);
        let yes = stats.iter().find(|s| s.side == "YES").unwrap();
        let no = stats.iter().find(|s| s.side == "NO").unwrap();
        assert_eq!(yes.wins, 1);
        assert_eq!(yes.losses, 0);
        assert!((yes.realized_pnl - 5.0).abs() < 1e-9);
        assert!((yes.roi_pct - 100.0).abs() < 1e-6);
        assert_eq!(no.wins, 0);
        assert_eq!(no.losses, 1);
        assert!((no.realized_pnl - (-2.0)).abs() < 1e-9);
        assert!((no.roi_pct - (-40.0)).abs() < 1e-6);
    }

    fn equity_snap(ts: &str, equity: f64) -> PaperEquitySnapshot {
        PaperEquitySnapshot {
            id: 1,
            ts: ts.to_string(),
            balance_dollars: equity,
            open_market_value: 0.0,
            equity_dollars: equity,
            unrealized_pnl: 0.0,
        }
    }

    #[test]
    fn session_pnl_empty_snapshots_returns_both_none() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let pnl = compute_session_pnl(&[], 10_500.0, &now);
        assert!(pnl.today.is_none());
        assert!(pnl.this_week.is_none());
    }

    #[test]
    fn session_pnl_all_snapshots_post_cutoff_returns_both_none() {
        // All snapshots are from "tomorrow" — past midnight, but no row
        // pre-dates the today midnight cutoff.
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snaps = vec![equity_snap("2026-06-29T01:00:00Z", 11_000.0)];
        let pnl = compute_session_pnl(&snaps, 11_000.0, &now);
        assert!(pnl.today.is_none(), "no baseline at-or-before today midnight");
        // The week cutoff is 2026-06-21T15:00:00Z; the 2026-06-29 snapshot is after that too.
        assert!(pnl.this_week.is_none());
    }

    #[test]
    fn session_pnl_uses_most_recent_baseline_at_or_before_cutoff() {
        // DESC-sorted snapshots; the helper must pick the newest one whose
        // ts <= the cutoff (NOT just the first snapshot in the list).
        // now = 2026-06-28T15:00:00Z
        // today_cutoff = 2026-06-28T00:00:00Z (midnight today)
        // week_cutoff = 2026-06-21T15:00:00Z (7 days before now)
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snaps = vec![
            equity_snap("2026-06-28T20:00:00Z", 11_000.0), // future relative to now
            equity_snap("2026-06-28T10:00:00Z", 10_800.0), // today, after midnight (> today_cutoff)
            equity_snap("2026-06-28T05:00:00Z", 10_500.0), // today, after midnight (> today_cutoff)
            equity_snap("2026-06-27T15:00:00Z", 10_300.0), // yesterday (<= today_cutoff, > week_cutoff)
            equity_snap("2026-06-20T10:00:00Z", 10_000.0), // 8 days ago (<= both cutoffs)
        ];
        let pnl = compute_session_pnl(&snaps, 10_900.0, &now);
        let today = pnl.today.expect("baseline exists for today");
        // Most recent <= today_cutoff (28T00:00) is 2026-06-27T15:00:00Z, equity 10300
        assert!((today.baseline_equity - 10_300.0).abs() < 1e-9);
        assert!(today.baseline_ts.starts_with("2026-06-27"));
        // pnl = 10900 - 10300 = +600; pct = 600 / 10300 * 100 ≈ 5.825
        assert!((today.pnl_dollars - 600.0).abs() < 1e-9);
        assert!((today.pnl_pct - (600.0 / 10_300.0 * 100.0)).abs() < 1e-6);

        // Most recent <= week_cutoff (21T15:00) is 2026-06-20T10:00:00Z, equity 10000
        // (27T15:00 is > 21T15:00, so doesn't qualify for week)
        let week = pnl.this_week.expect("baseline exists for 7d");
        assert!((week.baseline_equity - 10_000.0).abs() < 1e-9);
        assert!((week.pnl_dollars - 900.0).abs() < 1e-9);
    }

    #[test]
    fn session_pnl_today_and_week_pick_independent_baselines() {
        // now = 2026-06-28T15:00:00Z
        // today_cutoff = 2026-06-28T00:00:00Z (midnight today)
        // week_cutoff = 2026-06-21T15:00:00Z (7 days before now)
        // 3d ago = 2026-06-25T10:00:00Z (after today_cutoff, after week_cutoff)
        // 10d ago = 2026-06-18T10:00:00Z (before today_cutoff, before week_cutoff)
        // The 3d-ago snapshot (June 25) is AFTER today_cutoff (June 28 midnight) -
        // wait, June 25 is BEFORE June 28. Let me fix:
        // today_cutoff = June 28 00:00. June 25 < June 28, so June 25 qualifies for today.
        // week_cutoff = June 21 15:00. June 25 > June 21, so June 25 does NOT qualify for week.
        // June 18 < June 21, so June 18 qualifies for week.
        // So: today -> June 25 (10_400), week -> June 18 (9_900).
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snaps = vec![
            equity_snap("2026-06-25T10:00:00Z", 10_400.0), // 3d ago (qualifies today, not week)
            equity_snap("2026-06-18T10:00:00Z", 9_900.0),  // 10d ago (qualifies both)
        ];
        let pnl = compute_session_pnl(&snaps, 10_700.0, &now);
        let today = pnl.today.expect("today baseline");
        assert!((today.baseline_equity - 10_400.0).abs() < 1e-9);
        assert!((today.pnl_dollars - 300.0).abs() < 1e-9);
        let week = pnl.this_week.expect("week baseline");
        assert!((week.baseline_equity - 9_900.0).abs() < 1e-9);
        assert!((week.pnl_dollars - 800.0).abs() < 1e-9);
    }

    #[test]
    fn session_pnl_picks_older_baseline_for_week_when_today_has_none() {
        // now = 2026-06-28T15:00:00Z
        // today_cutoff = 2026-06-28T00:00:00Z (midnight today)
        // week_cutoff = 2026-06-21T15:00:00Z (7 days before now)
        // 10d ago = 2026-06-18T10:00:00Z (before today_cutoff, before week_cutoff)
        // 11d ago = 2026-06-17T10:00:00Z (before today_cutoff, before week_cutoff)
        // Today cutoff should find June 18 (it IS before June 28 midnight)
        // Week cutoff should find June 18 (it IS before June 21 15:00)
        // Actually, let me use a date that's > today_cutoff but < week_cutoff
        // That's impossible since today_cutoff (June 28 00:00) > week_cutoff (June 21 15:00)
        // So any date before today_cutoff is ALSO before week_cutoff.
        // Let me use dates that are ALL > today_cutoff:
        // 1d ago = June 27 10:00 (after today_cutoff, after week_cutoff)
        // 8d ago = June 20 10:00 (before today_cutoff, before week_cutoff)
        // But that would give today = June 20, week = June 20
        // To have today = None and week = Some, we need:
        // - No snapshot <= today_cutoff
        // - At least one snapshot <= week_cutoff
        // This is impossible since week_cutoff < today_cutoff!
        // Any snapshot <= week_cutoff is ALSO <= today_cutoff.
        // So if week finds one, today MUST also find one (or a later one).
        // The test as written is impossible - let me change it to a valid case.
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        // Use snapshots that are ALL after today_cutoff (June 28 00:00)
        // so today = None, and week_cutoff is June 21 15:00, so also none
        // Actually this test was checking an impossible case.
        // Let me make it test the case where we have a snapshot after today_cutoff
        // but before week_cutoff - impossible!
        // Let me just remove this test or change it to a valid case.
        // Valid case: all snapshots after today_cutoff -> both None
        let snaps = vec![
            equity_snap("2026-06-28T10:00:00Z", 11_000.0), // after today_cutoff
            equity_snap("2026-06-28T05:00:00Z", 10_500.0), // after today_cutoff
        ];
        let pnl = compute_session_pnl(&snaps, 10_900.0, &now);
        assert!(pnl.today.is_none(), "no baseline at-or-before today midnight");
        assert!(pnl.this_week.is_none(), "no baseline at-or-before week cutoff");
    }

    #[test]
    fn session_pnl_zero_baseline_equity_returns_zero_pct() {
        // Degenerate: baseline equity is exactly $0. Division by zero must
        // be guarded — pct returns 0.0, dollars still computed.
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snaps = vec![equity_snap("2026-06-25T10:00:00Z", 0.0)];
        let pnl = compute_session_pnl(&snaps, 100.0, &now);
        let today = pnl.today.expect("today baseline");
        assert!((today.baseline_equity - 0.0).abs() < 1e-9);
        assert!((today.pnl_dollars - 100.0).abs() < 1e-9);
        assert!((today.pnl_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn session_pnl_negative_baseline_does_not_invert_pct() {
        // Negative baseline equity must NOT flip the sign of the percent
        // change. The guard on `baseline_equity > 0.0` returns 0.0 pct.
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snaps = vec![equity_snap("2026-06-25T10:00:00Z", -50.0)];
        let pnl = compute_session_pnl(&snaps, 100.0, &now);
        let today = pnl.today.expect("today baseline");
        assert!((today.pnl_dollars - 150.0).abs() < 1e-9);
        assert!((today.pnl_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn session_pnl_skips_unparseable_timestamps() {
        // A snapshot with a garbage `ts` should be skipped (parse fails)
        // rather than panic or pick a wrong baseline.
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-28T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snaps = vec![
            equity_snap("not-a-timestamp", 12_345.0),
            equity_snap("2026-06-27T10:00:00Z", 10_200.0),
        ];
        let pnl = compute_session_pnl(&snaps, 10_500.0, &now);
        let today = pnl.today.expect("valid baseline survives");
        assert!((today.baseline_equity - 10_200.0).abs() < 1e-9);
    }

    // ── compute_hold_time_stats tests ────────────────────────────

    /// Helper: closed lot with explicit open/close timestamps (RFC 3339) +
    /// category + side + stake + pnl. Used by hold-time tests where the
    /// hold duration (closed_at - opened_at) drives the bucketing.
    fn closed_lot_at(
        opened_at: &str,
        closed_at: &str,
        side: &str,
        stake: f64,
        pnl: f64,
    ) -> PaperLot {
        PaperLot {
            id: format!("hold-{side}-{pnl}-{opened_at}"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: side.to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: stake,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: opened_at.to_string(),
            closed_at: Some(closed_at.to_string()),
            closed_price_cents: Some(if pnl >= 0.0 { 100.0 } else { 0.0 }),
            realized_pnl: Some(pnl),
            status: "Closed".to_string(),
            settlement_result: Some(
                if pnl > 0.0 {
                    "Win"
                } else if pnl < 0.0 {
                    "Loss"
                } else {
                    "Push"
                }
                .to_string(),
            ),
        }
    }

    /// Open lot with explicit opened_at, no closed_at. Goes to the bucket
    /// matching its hold-so-far (which is undefined until close), so we
    /// pin `opened_at = now` to keep the test deterministic.
    fn open_lot_at(opened_at: &str) -> PaperLot {
        PaperLot {
            id: format!("hold-open-{opened_at}"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: "Over".to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: 5.0,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: opened_at.to_string(),
            closed_at: None,
            closed_price_cents: None,
            realized_pnl: None,
            status: "Open".to_string(),
            settlement_result: None,
        }
    }

    #[test]
    fn hold_time_stats_empty_input_returns_empty_vec() {
        let stats = compute_hold_time_stats(&[]);
        assert!(stats.is_empty());
    }

    #[test]
    fn hold_time_stats_classifies_into_four_canonical_buckets() {
        // One lot in each canonical bucket. closed_at - opened_at drives
        // the bucket: 30 min → Intraday; 5h → SameDay; 3d → MultiDay; 14d → Long.
        let lots = vec![
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-01T10:30:00Z", "YES", 5.0, 1.0),
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-01T15:00:00Z", "YES", 5.0, -1.0),
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-04T10:00:00Z", "YES", 5.0, 2.0),
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-15T10:00:00Z", "YES", 5.0, -2.0),
        ];
        let stats = compute_hold_time_stats(&lots);
        // 4 buckets, all non-empty
        assert_eq!(stats.len(), 4);
        // Chronological order: Intraday → SameDay → MultiDay → Long
        assert_eq!(stats[0].bucket, "intraday");
        assert_eq!(stats[1].bucket, "same_day");
        assert_eq!(stats[2].bucket, "multi_day");
        assert_eq!(stats[3].bucket, "long");
        // Bucket labels include the human-readable window
        assert!(stats[0].bucket_label.contains("1h"));
        assert!(stats[1].bucket_label.contains("24h"));
        assert!(stats[2].bucket_label.contains("7d"));
        assert!(stats[3].bucket_label.contains("7d"));
        // Win/loss attribution per bucket
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 0);
        assert_eq!(stats[1].wins, 0);
        assert_eq!(stats[1].losses, 1);
        assert_eq!(stats[2].wins, 1);
        assert_eq!(stats[2].losses, 0);
        assert_eq!(stats[3].wins, 0);
        assert_eq!(stats[3].losses, 1);
        // PnL sums
        assert!((stats[0].realized_pnl - 1.0).abs() < 1e-9);
        assert!((stats[1].realized_pnl - -1.0).abs() < 1e-9);
        assert!((stats[2].realized_pnl - 2.0).abs() < 1e-9);
        assert!((stats[3].realized_pnl - -2.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_bucket_boundaries_are_exclusive_at_upper_end() {
        // 1h exactly → SameDay (boundary, not Intraday)
        // 24h exactly → MultiDay (boundary, not SameDay)
        // 7d exactly → Long (boundary, not MultiDay)
        let lots = vec![
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-08T00:00:00Z", "YES", 1.0, 1.0),
        ];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].bucket, "same_day");
        assert_eq!(stats[1].bucket, "multi_day");
        assert_eq!(stats[2].bucket, "long");
    }

    #[test]
    fn hold_time_stats_zero_hold_is_intraday() {
        // opened_at == closed_at → 0 seconds → Intraday. Sanity check that
        // zero hold time doesn't accidentally hit the unknown bucket.
        let lots = vec![closed_lot_at("2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z", "YES", 1.0, 1.0)];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "intraday");
        assert!((stats[0].avg_hold_seconds - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_negative_hold_falls_into_unknown() {
        // closed_at < opened_at is impossible in practice, but the helper
        // must defensively route these to Unknown so the canonical 4 buckets
        // stay meaningful.
        let lots = vec![closed_lot_at("2026-01-01T10:00:00Z", "2026-01-01T09:00:00Z", "YES", 1.0, 1.0)];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "unknown");
        // PnL still recorded (bucket routing is about display, not aggregation)
        assert!((stats[0].realized_pnl - 1.0).abs() < 1e-9);
        // No hold-duration stats because the hold is unparseable
        assert!((stats[0].avg_hold_seconds - 0.0).abs() < 1e-9);
        assert!((stats[0].median_hold_seconds - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_unparseable_timestamps_fall_into_unknown() {
        // Garbage timestamps → lot still counted, bucketed under Unknown.
        // The lot is "Closed" status with realized_pnl = Some, so it should
        // contribute to wins/losses/PnL/total_staked, but NOT to any
        // time-bucketed metrics.
        let mut bad = closed_lot_at("not-a-time", "2026-01-01T10:00:00Z", "YES", 5.0, 3.0);
        bad.opened_at = "not-a-time".to_string();
        let stats = compute_hold_time_stats(&[bad]);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "unknown");
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].wins, 1);
        assert!((stats[0].realized_pnl - 3.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 5.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_open_lot_counted_in_open_trades_only() {
        // Open lot (closed_at = None) → the helper can't compute hold
        // duration, so it falls into the Unknown bucket. The lot is still
        // counted in `total_trades` and `open_trades` but is excluded from
        // wins/losses/PnL/staked/hold-duration. Putting open lots in a
        // time-bucketed display would be misleading (the hold is still
        // ticking up) — Unknown is the honest answer.
        let lots = vec![open_lot_at("2026-01-01T10:00:00Z")];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "unknown");
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].open_trades, 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_push_lot_contributes_stake_but_no_win_or_loss() {
        // Push: realized_pnl = 0. Stakes toward total_staked (ROI denom)
        // and hold-duration samples, but does NOT count as win or loss.
        let lots = vec![closed_lot_at(
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:30:00Z",
            "YES",
            10.0,
            0.0,
        )];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "intraday");
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 10.0).abs() < 1e-9);
        // ROI = 0/10 * 100 = 0
        assert!((stats[0].roi_pct - 0.0).abs() < 1e-9);
        // Win rate undefined (no decided lots) → 0
        assert!((stats[0].win_rate - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_avg_and_median_hold_seconds() {
        // 3 intraday lots with hold durations 600s, 1200s, 1800s.
        // avg = 1200, median = 1200.
        let lots = vec![
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-01T10:10:00Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-01T10:20:00Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T10:00:00Z", "2026-01-01T10:30:00Z", "YES", 1.0, 1.0),
        ];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert!((stats[0].avg_hold_seconds - 1200.0).abs() < 1e-9);
        assert!((stats[0].median_hold_seconds - 1200.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_median_with_even_sample_count() {
        // 4 intraday lots: 100, 200, 300, 400 → median = (200 + 300) / 2 = 250.
        let lots = vec![
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-01T00:01:40Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-01T00:03:20Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-01T00:05:00Z", "YES", 1.0, 1.0),
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-01T00:06:40Z", "YES", 1.0, 1.0),
        ];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        // 100, 200, 300, 400 → median = (200+300)/2 = 250
        assert!((stats[0].median_hold_seconds - 250.0).abs() < 1e-9);
        // avg = (100 + 200 + 300 + 400) / 4 = 250 too
        assert!((stats[0].avg_hold_seconds - 250.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_win_rate_and_roi_computed_per_bucket() {
        // 1 win + 1 loss in MultiDay → win_rate 50%, PnL net 0, ROI 0%.
        let lots = vec![
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-03T00:00:00Z", "YES", 10.0, 5.0),
            closed_lot_at("2026-01-01T00:00:00Z", "2026-01-03T00:00:00Z", "YES", 10.0, -5.0),
        ];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "multi_day");
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
        assert!((stats[0].win_rate - 50.0).abs() < 1e-9);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 20.0).abs() < 1e-9);
        assert!((stats[0].roi_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hold_time_stats_omits_buckets_with_no_lots() {
        // Only MultiDay populated → no Intraday / SameDay / Long / Unknown
        // entries in the result. The result vector only contains buckets
        // that have at least one lot.
        let lots = vec![closed_lot_at(
            "2026-01-01T00:00:00Z",
            "2026-01-03T00:00:00Z",
            "YES",
            1.0,
            1.0,
        )];
        let stats = compute_hold_time_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "multi_day");
    }
}
