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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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

    /// Human-readable label used by the UI. Distinct from `as_str()` —
    /// the latter is the persisted DB representation, the former is
    /// the user-facing display string. Mirrors `DisagreementBucket::as_label`
    /// and `ConfidenceTier::as_label` so the UI doesn't have to
    /// hard-code the label mapping.
    pub fn as_label(self) -> &'static str {
        match self {
            PaperTradeSource::AiDecision => "AI decision",
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
    /// Optional user notes for this paper lot.
    pub notes: Option<String>,
    /// Optional comma-separated tags for categorization (e.g., "injury,regression,underdog").
    pub tags: Option<String>,
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
    /// Per-player performance breakdown. The `player` field is the player
    /// name extracted from the lot's `title` (`"<name> Over|Under <line>
    /// <stat>"` pattern), or `"Unknown"` when the title is empty /
    /// unparseable. Sorted by `realized_pnl` DESC so the strongest players
    /// surface first; ties broken alphabetically. Complements the
    /// per-category, per-side, and per-hold-time views — per-player answers
    /// "which players am I actually making money on?".
    pub player_stats: Vec<PaperPlayerStats>,
    /// Per-entry-price-bucket performance breakdown. Buckets are 20-cent
    /// wide (0-20¢, 20-40¢, 40-60¢, 60-80¢, 80-100¢) and only populated
    /// buckets appear. Sorted by `min_cents` ASC so the UI renders a stable
    /// \"cheapest to most expensive\" ladder. Helps users answer \"am I better
    /// at picking long-shots or favorites?\".
    pub entry_price_stats: Vec<PaperEntryPriceStats>,
    /// Calibration scatter: one point per closed (decided) paper lot, with
    /// the model's `fair_probability_pct` (X axis) and realized PnL in dollars
    /// (Y axis). Pushes (`realized_pnl_dollars == 0`) appear with
    /// `won = null` so the UI can render them on the X axis. `fair_probability_pct`
    /// is parsed from the lot's `decision_json` — lots with a missing or
    /// unparseable decision still appear (with `fair_probability_pct = 0` and
    /// `market_price_cents = null`) so the closed-lot count matches.
    pub calibration_points: Vec<CalibrationPoint>,
    /// Per-disagreement-bucket performance breakdown. Groups lots by the
    /// `model_disagreement` flag written to each lot's `decision_json` (a
    /// P2 milestone — |fair_probability_pct - market_price_pct| > 12pp at
    /// entry). The three canonical buckets (Disagreement / Consensus /
    /// Unknown) always appear in that fixed order so the UI renders a
    /// stable "disagree → agree → unknown" ladder. Answers the
    /// disagreement-tax question: "am I profitable on the picks where my
    /// model disagrees with the market?"
    pub paper_disagreement_stats: Vec<PaperDisagreementStats>,
    /// Per-tag performance breakdown. Tags are parsed from the lot's
    /// `tags` field (comma-separated, lowercased + trimmed), and a lot
    /// with multiple tags contributes to each tag bucket (so the
    /// `total_trades` sums across all tag buckets will exceed the unique
    /// closed-lot count). Lots with no tags are skipped (no "Untagged"
    /// bucket). Sorted by `realized_pnl` DESC with alphabetical tiebreak.
    /// Answers "which journaled play styles am I actually making money
    /// on?" — the natural follow-on to the notes/tags journaling system.
    pub tag_stats: Vec<PaperTagStats>,
    /// Per-confidence-tier performance breakdown. Confidence is parsed
    /// from the lot's `decision_json.confidence_tier` field (PascalCase
    /// string: "High" / "Medium" / "Low" / "None" — see
    /// `chat::decision_schema::ConfidenceTier`). The four canonical
    /// tiers always appear in the result vector in the order
    /// High → Medium → Low → None (highest conviction to lowest) so the
    /// UI renders a stable conviction ladder without resorting. Empty
    /// tiers are still emitted (with zeros) so the table layout doesn't
    /// shift as the user's history grows. The companion to
    /// `paper_disagreement_stats` — answers "am I profitable on the
    /// picks where the model was most confident?" (and conversely
    /// "should I skip Low/None confidence picks?").
    pub confidence_tier_stats: Vec<PaperConfidenceTierStats>,
    /// Per-source (AI vs Manual) performance breakdown. The two
    /// canonical sources (`AiDecision` → `Manual`) always appear in the
    /// result vector in that fixed order so the UI renders a stable
    /// "AI vs human" comparison without resorting. Empty sources are
    /// still emitted (with zeros) so the table layout doesn't shift as
    /// the user's history grows. The headline question this breakdown
    /// answers: **"is the AI model actually profitable vs. my manual
    /// picks?"** — the central evaluation question for the entire app.
    /// Without this view, a user has no aggregate signal that the AI
    /// component of the system is generating any edge. The data was
    /// already in `paper_lots.source` on every lot (set at fill time
    /// by `record_paper_decision` / `open_paper_trade`); this is a
    /// charting/aggregation task.
    pub source_stats: Vec<PaperSourceStats>,
    /// Top 5 closed paper lots sorted by `realized_pnl` DESC. Pushes
    /// (`realized_pnl == 0`) and open lots are excluded. Ties broken by
    /// `closed_at` ASC, then `lot_id` ASC. Empty when no closed lots
    /// have a non-zero realized PnL. The companion to
    /// `top_losers` — together they answer "what were the *specific*
    /// lots that drove my PnL?" so the user can click through to the
    /// journal and learn from them. Mirrors the per-axis-breakdown
    /// payload shape (small `Vec<…>` of records with display-ready
    /// context fields) so the UI can render the panel without
    /// follow-up `get_lot` round-trips.
    pub top_winners: Vec<PaperTopLot>,
    /// Top 5 closed paper lots sorted by `realized_pnl` ASC (most
    /// negative first). Same exclusion rules and tiebreaks as
    /// `top_winners`. Empty when no closed lots have a non-zero
    /// realized PnL. The companion to `top_winners` — the
    /// "what to learn from" mirror panel.
    pub top_losers: Vec<PaperTopLot>,
    /// Today and 7-day equity deltas for the summary card. Both windows are
    /// `None` when no baseline snapshot exists (e.g. a brand-new account).
    pub session_pnl: SessionPnl,
    pub fetched_at: String,
}

/// One closed (decided) paper lot projected onto a 2-D calibration plane.
/// `fair_probability_pct` is the model's "true" probability for the selected
/// side at the moment the lot was opened; `realized_pnl_dollars` is the
/// realized gain (positive) or loss (negative) when the lot was settled.
/// Pushes (`realized_pnl == 0.0`) are included with `won = None` so they
/// render on the X axis instead of being mis-classified as wins or losses.
/// `market_price_cents` is the implied market line at entry (0-100) so the
/// UI can compare model vs market at a glance; `None` when the lot's
/// `decision_json` is missing or unparseable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationPoint {
    pub lot_id: String,
    pub ticker: String,
    pub title: String,
    pub side: String,
    /// Model's fair probability for the selected side (0.0-100.0). 0.0 when
    /// the lot's `decision_json` was missing or unparseable.
    pub fair_probability_pct: f64,
    /// Market-implied price at entry (0-100 cents). `None` when the lot's
    /// `decision_json` is missing or unparseable.
    pub market_price_cents: Option<f64>,
    /// Realized PnL in dollars at settlement. Always 0.0 for pushes.
    pub realized_pnl_dollars: f64,
    /// Stake in dollars (used by the UI to size the scatter bubble).
    pub stake_dollars: f64,
    /// `Some(true)` for wins, `Some(false)` for losses, `None` for pushes
    /// (realized_pnl == 0).
    pub won: Option<bool>,
    /// Settlement timestamp (RFC 3339), used by the UI for hover tooltips
    /// and chronological sorting.
    pub closed_at: Option<String>,
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

/// Extract the player name from a paper-trade `title` field. PrizePicks
/// titles follow the canonical pattern `"<Player Name> <Over|Under> <line>
/// <stat>"` (e.g. `"Josh Allen Over 275.5 passing yards"`). We split on
/// the first `" Over "` or `" Under "` (case-insensitive, with surrounding
/// whitespace) and return the trimmed prefix. When the title is empty,
/// contains no separator, or the prefix is empty/whitespace after trimming,
/// the lot is bucketed under `"Unknown"` so we never silently drop it.
///
/// The parser is intentionally defensive — a malformed title routes to
/// `Unknown` rather than throwing or returning an empty string, so the
/// breakdown table always renders for the user.
fn extract_player_name(lot: &PaperLot) -> String {
    let trimmed = lot.title.trim();
    if trimmed.is_empty() {
        return "Unknown".to_string();
    }
    let lower = trimmed.to_lowercase();
    // Find the earliest occurrence of " over " or " under " so the player
    // name covers any multi-word surname before the side keyword.
    let over_idx = lower.find(" over ").map(|i| i + 1); // points to the 'o' of "over"
    let under_idx = lower.find(" under ").map(|i| i + 1);
    let cut = match (over_idx, under_idx) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => return "Unknown".to_string(),
    };
    let prefix = trimmed[..cut.unwrap()].trim();
    if prefix.is_empty() {
        "Unknown".to_string()
    } else {
        prefix.to_string()
    }
}

/// Performance breakdown for a single player. The `player` field is the
/// extracted name (from the lot's `title`), or `"Unknown"` when the title
/// is empty / unparseable. Mirrors `PaperCategoryStats` /
/// `PaperSideStats` but groups by player so the user can see "which
/// players am I actually making money on?" — the answer is usually not
/// obvious from the aggregate metrics alone. Sorted by `realized_pnl` DESC
/// so the strongest players surface first; ties broken alphabetically.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperPlayerStats {
    /// Player name extracted from the lot's `title`. `"Unknown"` when
    /// the title is empty or doesn't match the expected `<name> Over|Under
    /// <line> <stat>` pattern.
    pub player: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperPlayerStats {
    fn new(player: String) -> Self {
        Self {
            player,
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

/// Bucket closed + open lots by extracted player name and compute
/// per-player stats. Lots with empty / unparseable titles are bucketed
/// under `"Unknown"` so we never silently drop a lot. Output is sorted by
/// `realized_pnl` DESC, ties broken alphabetically. This complements the
/// per-category, per-side, and per-hold-time views — per-player answers
/// "which players am I making money on?" (most prop users have a strong
/// opinion on a small set of players they watch more closely than others).
fn compute_player_stats(lots: &[PaperLot]) -> Vec<PaperPlayerStats> {
    use std::collections::BTreeMap;

    let mut buckets: BTreeMap<String, PaperPlayerStats> = BTreeMap::new();
    for l in lots {
        let key = extract_player_name(l);
        let entry = buckets
            .entry(key.clone())
            .or_insert_with(|| PaperPlayerStats::new(key));
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

    let mut out: Vec<PaperPlayerStats> = buckets.into_values().collect();
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
    // Sort: realized_pnl DESC, then player ASC for ties. Stable so the
    // BTreeMap-ordering above doesn't matter.
    out.sort_by(|a, b| {
        b.realized_pnl
            .partial_cmp(&a.realized_pnl)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.player.cmp(&b.player))
    });
    out
}

/// Bucket closed + open lots by entry price (in cents) and compute
/// per-bucket stats. Buckets are 20-cent wide: 0-20, 20-40, 40-60, 60-80,
/// 80-100 (the valid range for PrizePicks binary prices). Only buckets with
/// at least one lot are emitted. Output is sorted by `min_cents` ASC so the
/// UI renders a stable "cheapest to most expensive" ladder.
///
/// Open lots contribute to `total_trades` and `open_trades` but not to
/// realized PnL, wins/losses, or the ROI denominator (only closed stake
/// counts for ROI). Pushes (pnl == 0) contribute stake but not wins/losses.
fn compute_entry_price_stats(lots: &[PaperLot]) -> Vec<PaperEntryPriceStats> {
    use std::collections::BTreeMap;

    // Define the canonical bucket boundaries (in cents).
    const BUCKETS: &[(f64, f64, &str)] = &[
        (0.0, 20.0, "0-20¢"),
        (20.0, 40.0, "20-40¢"),
        (40.0, 60.0, "40-60¢"),
        (60.0, 80.0, "60-80¢"),
        (80.0, 100.0, "80-100¢"),
    ];

    // Use BTreeMap keyed by bucket *index* (0..BUCKETS.len()) so iteration
    // is naturally sorted. Keying by `min_cents` (f64) would not compile —
    // `BTreeMap` requires `K: Ord`, and `f64` only implements `PartialOrd`.
    // The 5 canonical buckets are stored as static f64 tuples so the bucket
    // boundaries stay readable; the index is the map key.
    let mut buckets: BTreeMap<usize, PaperEntryPriceStats> = BTreeMap::new();
    for l in lots {
        let price = l.entry_price_cents;
        // Find the matching bucket (linear scan over 5 items is trivial).
        // The default `(80.0, 100.0, "80-100¢")` is the last bucket so any
        // price >= 100.0 lands there (defensive — valid PrizePicks prices
        // are 0-100¢ exclusive of the upper bound, but we never want a panic).
        let idx = BUCKETS
            .iter()
            .position(|(lo, hi, _)| price >= *lo && price < *hi)
            .unwrap_or(BUCKETS.len() - 1);
        let (min_c, max_c, label) = BUCKETS[idx];
        let entry = buckets
            .entry(idx)
            .or_insert_with(|| PaperEntryPriceStats::new(label.to_string(), min_c, max_c));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
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

    let mut out: Vec<PaperEntryPriceStats> = buckets.into_values().collect();
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
    // Already sorted by min_cents via BTreeMap iteration order.
    out
}

/// Performance breakdown for a single entry-price bucket (e.g. "0-20¢",
/// "20-40¢", ..., "80-100¢"). Mirrors `PaperCategoryStats` / `PaperSideStats`
/// but groups by the lot's `entry_price_cents` at trade time. Helps users
/// answer "am I better at picking long-shots or favorites?" — the answer
/// is usually not obvious from aggregate metrics alone. Buckets are emitted
/// in ascending order (cheapest → most expensive) so the UI can render a
/// stable "long-shot to favorite" ladder. Only populated buckets appear.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperEntryPriceStats {
    /// Human-readable bucket label, e.g. "0-20¢", "20-40¢", "80-100¢".
    pub bucket: String,
    /// Lower bound of the bucket in cents (inclusive).
    pub min_cents: f64,
    /// Upper bound of the bucket in cents (exclusive).
    pub max_cents: f64,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperEntryPriceStats {
    fn new(bucket: String, min_cents: f64, max_cents: f64) -> Self {
        Self {
            bucket,
            min_cents,
            max_cents,
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

/// One closed (decided) paper lot, projected onto a calibration scatter plane.
/// Walks the lot list and emits one `CalibrationPoint` per closed lot. Open
/// lots are skipped (they have no realized PnL yet). Lots whose
/// `decision_json` is missing or unparseable still produce a point — but with
/// `fair_probability_pct = 0.0` and `market_price_cents = None` — so the
/// scatter preserves the closed-lot count even when an older lot predates
/// the decision-JSON migration. Pushes (realized_pnl == 0) produce a point
/// with `won = None` so they sit on the X axis without being mis-classified
/// as wins or losses. Output order matches input order so the UI can render
/// the most-recently-closed lot on top (the caller can reverse if needed).
///
/// The function is pure (no DB access) so it is trivially testable.
fn compute_calibration_points(lots: &[PaperLot]) -> Vec<CalibrationPoint> {
    lots.iter()
        .filter(|l| l.status == "Closed")
        .map(|l| {
            // Realized PnL — closed lots always have `realized_pnl` set, but
            // be defensive: treat `None` as 0.0 (effectively a push).
            let pnl = l.realized_pnl.unwrap_or(0.0);
            let won = if pnl > 0.0 {
                Some(true)
            } else if pnl < 0.0 {
                Some(false)
            } else {
                None
            };

            // Try to parse `fair_probability_pct` and `market_price_pct` from
            // the lot's `decision_json`. Both fields default to neutral
            // values when the JSON is missing, unparseable, or the fields
            // are absent. `market_price_pct` lives on a 0-1 scale in the
            // decision schema (per `chat/decision_schema.rs`); multiply by
            // 100 to convert to the cents-style 0-100 scale the rest of the
            // analytics use. Out-of-range values are clamped rather than
            // dropped so a stray 1.2 doesn't poison the X axis.
            let (fair_prob, market_cents) = match l.decision_json.as_deref() {
                Some(json) => match serde_json::from_str::<serde_json::Value>(json) {
                    Ok(v) => {
                        let fair = v
                            .get("fair_probability_pct")
                            .and_then(|x| x.as_f64())
                            .unwrap_or(0.0)
                            .clamp(0.0, 100.0);
                        let market = v.get("market_price_pct").and_then(|x| x.as_f64()).map(|m| {
                            // Schema stores market_price_pct on 0-1, but
                            // some older serializations wrote 0-100. Detect
                            // the scale by magnitude and normalize.
                            let normalized = if m <= 1.0 { m * 100.0 } else { m };
                            normalized.clamp(0.0, 100.0)
                        });
                        (fair, market)
                    }
                    Err(_) => (0.0, None),
                },
                None => (0.0, None),
            };

            CalibrationPoint {
                lot_id: l.id.clone(),
                ticker: l.ticker.clone(),
                title: l.title.clone(),
                side: l.side.clone(),
                fair_probability_pct: fair_prob,
                market_price_cents: market_cents,
                realized_pnl_dollars: pnl,
                stake_dollars: l.stake_dollars,
                won,
                closed_at: l.closed_at.clone(),
            }
        })
        .collect()
}

/// Canonical disagreement-bucket identifier. The three buckets always appear
/// in `compute_disagreement_stats` output, in the order: `Disagreement`
/// (|fair - market| > 12pp at entry), `Consensus` (≤ 12pp), `Unknown`
/// (decision_json missing or unparseable). The 12pp threshold matches the
/// P2 `model_disagreement` flag in `chat/decision_schema.rs`. The enum is
/// serialized in snake_case for stable IPC.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DisagreementBucket {
    /// |fair_probability_pct - market_price_pct| > 12pp at entry.
    Disagreement,
    /// |fair - market| ≤ 12pp at entry (or the model and market agree within
    /// the threshold).
    Consensus,
    /// `decision_json` was missing or unparseable — bucket by absence
    /// rather than dropping the lot, so the closed-lot count still matches
    /// the rest of the analytics.
    Unknown,
}

impl DisagreementBucket {
    /// Human-readable label, e.g. `"Disagreement (>12pp)"`. The UI should
    /// prefer this over the raw enum variant for display.
    pub fn as_label(self) -> &'static str {
        match self {
            DisagreementBucket::Disagreement => "Disagreement (>12pp)",
            DisagreementBucket::Consensus => "Consensus (≤12pp)",
            DisagreementBucket::Unknown => "Unknown",
        }
    }
}

/// Performance breakdown for a single model-vs-market disagreement bucket.
/// Mirrors `PaperCategoryStats` / `PaperSideStats` but groups by whether
/// the model disagreed with the market at entry. A user who is profitable
/// on consensus picks but bleeding on disagreement picks has a clear
/// actionable signal: skip the disagreement tax. Sorted by
/// `realized_pnl` DESC so the strongest bucket surfaces first; ties broken
/// alphabetically by `bucket_label`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperDisagreementStats {
    /// Raw enum variant (snake_case) for machine-readable comparison. The
    /// UI should prefer `bucket_label` for display.
    pub bucket: DisagreementBucket,
    /// Human-readable label, e.g. `"Disagreement (>12pp)"`. Mirrors
    /// `PaperHoldTimeStats.bucket_label` so the UI doesn't have to hard-code
    /// the label mapping.
    pub bucket_label: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperDisagreementStats {
    fn new(bucket: DisagreementBucket) -> Self {
        Self {
            bucket_label: bucket.as_label().to_string(),
            bucket,
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

/// Canonical 12pp threshold for `model_disagreement` (matches
/// `chat/decision_schema.rs::compute`).
const DISAGREEMENT_THRESHOLD_PP: f64 = 12.0;

/// Read `model_disagreement` (bool) and `disagreement_points` (f64) out of
/// a lot's `decision_json`. Returns `(bucket, parsed_flag)` — `parsed_flag`
/// is `true` when both fields were present in the JSON so the caller can
/// distinguish "actually read false" from "field was absent". Used as the
/// bucketing key in `compute_disagreement_stats`.
fn lot_disagreement_bucket(lot: &PaperLot) -> DisagreementBucket {
    match lot.decision_json.as_deref() {
        Some(json) => match serde_json::from_str::<serde_json::Value>(json) {
            Ok(v) => {
                // Prefer the boolean `model_disagreement` flag when present
                // (matches what the decision pipeline sets); fall back to
                // comparing `disagreement_points` against the 12pp threshold
                // for legacy serializations that only wrote the raw delta.
                if let Some(flag) = v.get("model_disagreement").and_then(|x| x.as_bool()) {
                    if flag {
                        DisagreementBucket::Disagreement
                    } else {
                        DisagreementBucket::Consensus
                    }
                } else if let Some(pts) =
                    v.get("disagreement_points").and_then(|x| x.as_f64())
                {
                    if pts.abs() > DISAGREEMENT_THRESHOLD_PP {
                        DisagreementBucket::Disagreement
                    } else {
                        DisagreementBucket::Consensus
                    }
                } else {
                    DisagreementBucket::Unknown
                }
            }
            Err(_) => DisagreementBucket::Unknown,
        },
        None => DisagreementBucket::Unknown,
    }
}

/// Bucket closed + open lots by model-vs-market disagreement bucket and
/// compute per-bucket stats. The three canonical buckets (Disagreement /
/// Consensus / Unknown) always appear in the result vector in that fixed
/// order — not sorted by PnL — so the UI renders a stable
/// "disagree → agree → unknown" ladder without resorting. Empty buckets
/// are still emitted (with zeros) so the table layout doesn't shift as
/// the user's history grows.
///
/// Win/loss/PnL aggregation semantics match the other breakdown helpers
/// exactly: closed lots count toward wins/losses/realized_pnl/
/// total_staked; pushes (pnl == 0) contribute stake but not wins/losses;
/// open lots count toward total_trades + open_trades but contribute
/// nothing to the PnL/ROI aggregations. ROI denominator is closed stake
/// only — open positions are excluded from the per-bucket ROI math.
fn compute_disagreement_stats(lots: &[PaperLot]) -> Vec<PaperDisagreementStats> {
    // Fixed bucket order: Disagreement → Consensus → Unknown. The output
    // is emitted in this exact order so the UI doesn't have to sort.
    let bucket_order = [
        DisagreementBucket::Disagreement,
        DisagreementBucket::Consensus,
        DisagreementBucket::Unknown,
    ];

    // Walk the lots once, bucketing each by its disagreement flag.
    let mut buckets: std::collections::BTreeMap<DisagreementBucket, PaperDisagreementStats> =
        std::collections::BTreeMap::new();
    for l in lots {
        let key = lot_disagreement_bucket(l);
        let entry = buckets
            .entry(key)
            .or_insert_with(|| PaperDisagreementStats::new(key));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
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

    // Ensure every canonical bucket is present, even if empty. This keeps
    // the table layout stable for users who have only one type of trade
    // (e.g. only consensus picks so far).
    for &b in &bucket_order {
        buckets.entry(b).or_insert_with(|| PaperDisagreementStats::new(b));
    }

    // Finalize win-rate + ROI per bucket, then emit in canonical order.
    let mut out: Vec<PaperDisagreementStats> = Vec::with_capacity(bucket_order.len());
    for &b in &bucket_order {
        let mut s = buckets.remove(&b).unwrap_or_else(|| PaperDisagreementStats::new(b));
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
        out.push(s);
    }
    out
}

/// Canonical confidence-tier identifier. The four tiers always appear
/// in `compute_confidence_tier_stats` output, in the order:
/// `High` → `Medium` → `Low` → `None`. Serialized in snake_case for
/// stable IPC; the human-readable label is exposed via `bucket_label`
/// on the companion stats struct.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceTier {
    /// Strong conviction + excellent data quality.
    High,
    /// Moderate conviction + good data quality.
    Medium,
    /// Weak conviction or incomplete data.
    Low,
    /// No confidence — default for PASS decisions, or for lots whose
    /// `decision_json` was missing/unparseable.
    None,
}

impl ConfidenceTier {
    /// Human-readable label, e.g. `"High"`. The UI should prefer this
    /// over the raw enum variant for display.
    pub fn as_label(self) -> &'static str {
        match self {
            ConfidenceTier::High => "High",
            ConfidenceTier::Medium => "Medium",
            ConfidenceTier::Low => "Low",
            ConfidenceTier::None => "None",
        }
    }
}

/// Performance breakdown for a single model-confidence tier. Mirrors
/// `PaperCategoryStats` / `PaperSideStats` / `PaperDisagreementStats`
/// but groups by the model's stated confidence at entry
/// (parsed from `decision_json.confidence_tier`). The four canonical
/// tiers always appear in the result vector in the fixed order
/// High → Medium → Low → None (highest conviction to lowest) so the
/// UI renders a stable "conviction ladder" without resorting. Empty
/// tiers are still emitted (zeros when empty) so the table layout
/// doesn't shift as the user's history grows. The companion to
/// `paper_disagreement_stats` — together they answer the question
/// "is the model self-aware?" (i.e. are the high-confidence picks
/// actually the profitable ones, and are the disagreement picks the
/// ones I'm losing on?).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperConfidenceTierStats {
    /// Raw enum variant (snake_case) for machine-readable comparison.
    /// The UI should prefer `bucket_label` for display.
    pub bucket: ConfidenceTier,
    /// Human-readable label, e.g. `"High"`. Mirrors
    /// `PaperDisagreementStats.bucket_label` so the UI doesn't have
    /// to hard-code the label mapping.
    pub bucket_label: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperConfidenceTierStats {
    fn new(bucket: ConfidenceTier) -> Self {
        Self {
            bucket_label: bucket.as_label().to_string(),
            bucket,
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

/// Read `confidence_tier` (PascalCase string: "High" / "Medium" / "Low" /
/// "None") out of a lot's `decision_json` and map it to a
/// `ConfidenceTier`. Returns `ConfidenceTier::None` for missing or
/// unparseable JSON (defensively routes legacy lots that pre-date
/// the confidence_tier field to the None bucket so the closed-lot
/// count still matches the rest of the analytics).
fn lot_confidence_tier(lot: &PaperLot) -> ConfidenceTier {
    match lot.decision_json.as_deref() {
        Some(json) => match serde_json::from_str::<serde_json::Value>(json) {
            Ok(v) => {
                if let Some(s) = v.get("confidence_tier").and_then(|x| x.as_str()) {
                    match s {
                        "High" => ConfidenceTier::High,
                        "Medium" => ConfidenceTier::Medium,
                        "Low" => ConfidenceTier::Low,
                        // "None", or any unrecognized value (e.g. "pass",
                        // legacy "Skip"), routes to the None bucket.
                        _ => ConfidenceTier::None,
                    }
                } else {
                    ConfidenceTier::None
                }
            }
            Err(_) => ConfidenceTier::None,
        },
        None => ConfidenceTier::None,
    }
}

/// Bucket closed + open lots by model-stated confidence tier and
/// compute per-bucket stats. The four canonical tiers (High / Medium /
/// Low / None) always appear in the result vector in that fixed order
/// — not sorted by PnL — so the UI renders a stable
/// "high → medium → low → none" conviction ladder without resorting.
/// Empty tiers are still emitted (with zeros) so the table layout
/// doesn't shift as the user's history grows.
///
/// Win/loss/PnL aggregation semantics match the other breakdown
/// helpers exactly: closed lots count toward wins/losses/
/// realized_pnl/total_staked; pushes (pnl == 0) contribute stake but
/// not wins/losses; open lots count toward total_trades + open_trades
/// but contribute nothing to the PnL/ROI aggregations. ROI denominator
/// is closed stake only — open positions are excluded from the
/// per-bucket ROI math.
fn compute_confidence_tier_stats(lots: &[PaperLot]) -> Vec<PaperConfidenceTierStats> {
    // Fixed tier order: High → Medium → Low → None. The output is
    // emitted in this exact order so the UI doesn't have to sort.
    let tier_order = [
        ConfidenceTier::High,
        ConfidenceTier::Medium,
        ConfidenceTier::Low,
        ConfidenceTier::None,
    ];

    // Walk the lots once, bucketing each by its confidence tier.
    let mut buckets: std::collections::BTreeMap<ConfidenceTier, PaperConfidenceTierStats> =
        std::collections::BTreeMap::new();
    for l in lots {
        let key = lot_confidence_tier(l);
        let entry = buckets
            .entry(key)
            .or_insert_with(|| PaperConfidenceTierStats::new(key));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
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

    // Ensure every canonical tier is present, even if empty. This
    // keeps the table layout stable for users who have only one tier
    // in their history (e.g. only High picks so far).
    for &t in &tier_order {
        buckets.entry(t).or_insert_with(|| PaperConfidenceTierStats::new(t));
    }

    // Finalize win-rate + ROI per bucket, then emit in canonical
    // order. The tier order uses serde_json order... but the actual
    // ordinal is alphabetical (High < Low < Medium < None) so we
    // iterate the explicit `tier_order` array rather than the map.
    let mut out: Vec<PaperConfidenceTierStats> = Vec::with_capacity(tier_order.len());
    for &t in &tier_order {
        let mut s = buckets
            .remove(&t)
            .unwrap_or_else(|| PaperConfidenceTierStats::new(t));
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
        out.push(s);
    }
    out
}

/// is the dollar change between the most-recent equity snapshot and the
/// baseline snapshot; `pnl_pct` is `pnl_dollars / baseline_equity * 100`
/// (returns 0.0 when `baseline_equity <= 0`). `baseline_ts` is the timestamp
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

/// Performance breakdown for a single user-supplied tag. Mirrors
/// `PaperCategoryStats` / `PaperSideStats` but groups by tag rather than
/// by structural property. Tags are pulled from the lot's `tags` field
/// (comma-separated, e.g. `"injury,regression,underdog"`) and are
/// lowercased + trimmed so capitalization differences don't fragment
/// the data. A lot with multiple tags contributes to *each* tag bucket
/// (so the tag totals don't sum to `total_trades`). Lots with no tags
/// are skipped — an "Untagged" bucket would dwarf everything for users
/// who only journal a fraction of their trades.
///
/// Sorted by `realized_pnl` DESC so the strongest tags surface first;
/// ties broken alphabetically for deterministic output. The
/// `total_trades` count is the number of *lots* in the bucket (which
/// can exceed the number of *unique* closed lots when a lot carries
/// multiple tags), and `open_trades` only counts lots whose status is
/// `"Open"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperTagStats {
    /// Canonical tag name (lowercased + trimmed). The UI should prefer
    /// this for display and for chip coloring.
    pub tag: String,
    /// Number of lots that carried this tag (a lot with two tags counts
    /// toward both). Note this can exceed the number of unique closed
    /// lots for the same reason.
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperTagStats {
    fn new(tag: String) -> Self {
        Self {
            tag,
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

/// Split a lot's `tags` field into individual canonical tag names.
/// Returns an empty `Vec` for `None`, empty, or whitespace-only inputs.
/// Each non-empty comma-separated segment is trimmed; empty segments
/// (from `"a,,b"` or trailing commas) are dropped. Output is
/// lowercased so `"Injury"` and `"injury"` collapse to the same
/// bucket. Order is preserved (so a test can assert on the resulting
/// list) but the bucketing logic itself is order-independent.
fn split_tags(tags: Option<&str>) -> Vec<String> {
    let raw = match tags {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Vec::new(),
    };
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Bucket closed + open lots by their user-supplied tags and compute
/// per-tag stats. A lot with tags `"injury,regression"` contributes
/// to both the `injury` and `regression` buckets. Lots with no tags
/// are skipped — the UI surfaces this as an empty state ("tag your
/// trades to see per-tag performance"). Win/loss/PnL aggregation
/// semantics match the other breakdown helpers exactly: closed lots
/// count toward wins/losses/realized_pnl/total_staked; pushes (pnl ==
/// 0) contribute stake but not wins/losses; open lots count toward
/// `total_trades + open_trades` but contribute nothing to the PnL/ROI
/// aggregations. ROI denominator is closed stake only — open positions
/// are excluded from the per-tag ROI math.
///
/// Output is sorted by `realized_pnl` DESC with alphabetical tiebreak,
/// so the strongest tags surface first. Empty tags (`None`,
/// whitespace-only, or only commas) are silently dropped — no
/// "Untagged" bucket — because for users who only journal a small
/// fraction of their trades that bucket would dwarf everything else
/// and provide a misleading signal.
fn compute_tag_stats(lots: &[PaperLot]) -> Vec<PaperTagStats> {
    use std::collections::BTreeMap;

    let mut buckets: BTreeMap<String, PaperTagStats> = BTreeMap::new();
    for l in lots {
        let tags = split_tags(l.tags.as_deref());
        if tags.is_empty() {
            continue;
        }
        let pnl = if l.status == "Closed" {
            l.realized_pnl.unwrap_or(0.0)
        } else {
            0.0
        };
        let is_push = l.status == "Closed" && pnl == 0.0;
        for tag in tags {
            let entry = buckets
                .entry(tag.clone())
                .or_insert_with(|| PaperTagStats::new(tag));
            entry.total_trades += 1;
            if l.status == "Open" {
                entry.open_trades += 1;
                continue;
            }
            if !is_push {
                if pnl > 0.0 {
                    entry.wins += 1;
                } else if pnl < 0.0 {
                    entry.losses += 1;
                }
            }
            entry.realized_pnl += pnl;
            entry.total_staked += l.stake_dollars;
        }
    }

    // Finalize win-rate + ROI per bucket, then sort by PnL DESC with
    // alphabetical tiebreak (matches `compute_category_stats` and
    // `compute_side_stats`).
    let mut out: Vec<PaperTagStats> = buckets.into_values().collect();
    for s in &mut out {
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
    out.sort_by(|a, b| {
        b.realized_pnl
            .partial_cmp(&a.realized_pnl)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.tag.cmp(&b.tag))
    });
    out
}

/// Per-source (AI vs Manual) performance breakdown. The two canonical
/// sources always appear in the result vector in the fixed order
/// `AiDecision` → `Manual` (so the UI renders a stable "model vs human"
/// ladder without resorting). An empty input still emits both buckets
/// with zeros so the table layout is stable.
///
/// The headline question this breakdown answers: **"Is the AI model
/// actually profitable vs. my manual picks?"** That is the central
/// evaluation question for the entire app — without this view, a user
/// has no aggregate signal that the AI component of the system is
/// generating any edge. The data is already in `paper_lots.source` on
/// every lot (set at fill time by `record_paper_decision` /
/// `open_paper_trade`); this is a charting/aggregation task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperSourceStats {
    /// Raw enum variant (snake_case via `#[serde(rename_all = "snake_case")]`)
    /// for machine-readable comparison. The UI should prefer `source_label`
    /// for display.
    pub source: PaperTradeSource,
    /// Human-readable label, e.g. `"AI decision"` / `"Manual"`. Mirrors
    /// `PaperHoldTimeStats.bucket_label` / `PaperDisagreementStats.bucket_label`
    /// so the UI doesn't have to hard-code the label mapping.
    pub source_label: String,
    pub total_trades: u32,
    pub open_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub total_staked: f64,
    pub roi_pct: f64,
}

impl PaperSourceStats {
    fn new(source: PaperTradeSource) -> Self {
        Self {
            source_label: source.as_label().to_string(),
            source,
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

/// Bucket closed + open lots by their `PaperTradeSource` and compute
/// per-source stats. The two canonical sources (AiDecision, Manual)
/// always appear in the result vector in the fixed order
/// `AiDecision` → `Manual` — not sorted by PnL — so the UI renders a
/// stable "AI vs manual" comparison without resorting. Empty buckets
/// are still emitted (with zeros) so the table layout doesn't shift
/// as the user's history grows.
///
/// Win/loss/PnL aggregation semantics match the other breakdown helpers
/// exactly: closed lots count toward wins/losses/realized_pnl/
/// total_staked; pushes (pnl == 0) contribute stake but not
/// wins/losses; open lots count toward `total_trades + open_trades`
/// but contribute nothing to the PnL/ROI aggregations. ROI denominator
/// is closed stake only — open positions are excluded from the
/// per-source ROI math.
fn compute_source_stats(lots: &[PaperLot]) -> Vec<PaperSourceStats> {
    // Fixed source order: AiDecision → Manual. The output is emitted
    // in this exact order so the UI doesn't have to sort.
    let source_order = [PaperTradeSource::AiDecision, PaperTradeSource::Manual];

    // Walk the lots once, bucketing each by its `source` enum.
    let mut buckets: std::collections::BTreeMap<PaperTradeSource, PaperSourceStats> =
        std::collections::BTreeMap::new();
    for l in lots {
        let entry = buckets
            .entry(l.source)
            .or_insert_with(|| PaperSourceStats::new(l.source));
        entry.total_trades += 1;
        if l.status == "Open" {
            entry.open_trades += 1;
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

    // Ensure every canonical source is present, even if empty. This
    // keeps the table layout stable for users who have only opened
    // one type of trade (e.g. only manual picks so far).
    for &s in &source_order {
        buckets.entry(s).or_insert_with(|| PaperSourceStats::new(s));
    }

    // Finalize win-rate + ROI per source, then emit in canonical order.
    let mut out: Vec<PaperSourceStats> = Vec::with_capacity(source_order.len());
    for &s in &source_order {
        let mut stat = buckets
            .remove(&s)
            .unwrap_or_else(|| PaperSourceStats::new(s));
        let decided = stat.wins + stat.losses;
        stat.win_rate = if decided > 0 {
            (stat.wins as f64 / decided as f64) * 100.0
        } else {
            0.0
        };
        stat.roi_pct = if stat.total_staked > 0.0 {
            (stat.realized_pnl / stat.total_staked) * 100.0
        } else {
            0.0
        };
        out.push(stat);
    }
    out
}

/// One closed paper lot surfaced in the "top winners / top losers" panel.
/// Carries enough context (title, side, prices, stake, settlement result,
/// close timestamp) for the React side to render a one-line "lesson
/// learned" row without re-fetching the lot. Mirrors the field shape of
/// `CalibrationPoint` but with realized PnL as the headline number — the
/// calibration chart's headline is the model fair %, this one is the
/// dollar result. Open lots are excluded (no realized PnL yet); pushes
/// (`realized_pnl == 0`) are also excluded from both lists so the panel
/// only shows the genuinely informative outcomes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperTopLot {
    /// The lot's primary key — used by the UI as a React `key` and (in a
    /// future iteration) to deep-link to the journal editor.
    pub lot_id: String,
    /// The PrizePicks ticker (e.g. `"NFL-QB-JOSHALLEN-Passyds-275.5"`).
    pub ticker: String,
    /// Human-readable title (e.g. `"Josh Allen Over 275.5 passing yards"`).
    /// Stored directly on the lot at fill time, so this is the same string
    /// the user saw in the prediction list.
    pub title: String,
    /// Stat category (e.g. `"Points"`, `"Rebounds"`). Carried over so the
    /// row can be tagged with its category without an extra join.
    pub category: String,
    /// `"Over"` / `"Under"`. The lot's `side` field is the canonical
    /// human-readable string (the `"YES"` / `"NO"` storage form is
    /// normalized on write by `place_trade`).
    pub side: String,
    /// Dollar PnL for this lot. Always non-zero for entries in
    /// `top_winners` (positive) and `top_losers` (negative) — see the
    /// helper docstring for the exclusion rules.
    pub realized_pnl: f64,
    /// Dollar stake on the lot. Used to render the ROI multiplier next to
    /// the headline PnL (`+$5.00 on $2.00 stake → 2.5x`) so the user can
    /// see at a glance whether the big winner was also a high-conviction
    /// size-up.
    pub stake_dollars: f64,
    /// Entry price in cents. Carried so the row can show the price the
    /// user entered at without a follow-up `get_lot` round-trip.
    pub entry_price_cents: f64,
    /// Exit price in cents, when the lot was closed via the manual
    /// `close_lot` path. `None` for auto-graded lots where the resolved
    /// outcome is `"Win"` / `"Loss"` but no explicit exit price was
    /// recorded (the implementation collapses a 99.99¢ settlement to
    /// `Win` without writing `closed_price_cents`).
    pub closed_price_cents: Option<f64>,
    /// ISO-8601 close timestamp. Lets the UI surface "Closed 3 days ago"
    /// context without a separate query.
    pub closed_at: Option<String>,
    /// `"Win"` / `"Loss"` — the settlement result the helper computed at
    /// close time. Always non-Push for entries in either list.
    pub settlement_result: Option<String>,
}

/// Default number of top winners / top losers to surface. Five matches the
/// implicit convention of the per-axis breakdowns (which emit only
/// populated buckets — the closest analog is the size of the "top of the
/// leaderboard" you'd want to see in a single screen of the panel).
const TOP_LOTS_LIMIT: usize = 5;

/// Build the `top_winners` + `top_losers` pair for the `PaperAnalytics`
/// payload.
///
/// **Exclusions (apply to both lists):**
///   - Open lots (no realized PnL).
///   - Pushes (realized PnL == 0 — neither a win nor a loss).
///   - Lots where `realized_pnl` is `None` (defensive — should not happen
///     for a Closed lot, but the helper is total over its input).
///
/// **Sort order:**
///   - `top_winners`: `realized_pnl` DESC (biggest PnL first). Ties
///     broken by `closed_at` ASC (older wins first), then by `lot_id`
///     ASC for determinism.
///   - `top_losers`:  `realized_pnl` ASC (most negative first). Same
///     tiebreak.
///
/// **Capping:** each list is capped at `TOP_LOTS_LIMIT` (5) so a user
/// with 500 closed lots doesn't get a panel that scrolls forever. The
/// panel mirrors the "top 5" mental model users already have from the
/// equity-curve chart and the per-axis leaderboards.
///
/// **Output size:** always `0` (empty) when there are no qualifying
/// winners/losers, never padded. The UI can render the empty state copy
/// from `stats.length === 0` checks.
fn compute_top_lots(lots: &[PaperLot]) -> (Vec<PaperTopLot>, Vec<PaperTopLot>) {
    // Walk once, splitting the decided lots into separate winners /
    // losers pools. Doing the split up front (rather than relying on
    // the sort to partition) means a sign-confused lot — e.g. a
    // negative PnL that the sort places above a positive PnL because of
    // a NaN in the underlying column — can never bleed into the wrong
    // panel. The "decided" gate is identical for both pools: status
    // must be "Closed" and realized_pnl must be Some(p) with p != 0.0.
    let mut winners_only: Vec<&PaperLot> = Vec::new();
    let mut losers_only: Vec<&PaperLot> = Vec::new();
    for l in lots {
        if l.status != "Closed" {
            continue;
        }
        match l.realized_pnl {
            Some(p) if p > 0.0 => winners_only.push(l),
            Some(p) if p < 0.0 => losers_only.push(l),
            _ => continue, // None or push (==0) — excluded from both
        }
    }

    // Project to `PaperTopLot`. Defined as a closure so the projection
    // logic lives in exactly one place even though it's invoked from
    // both sort-then-take branches below.
    let to_top_lot = |l: &PaperLot| PaperTopLot {
        lot_id: l.id.clone(),
        ticker: l.ticker.clone(),
        title: l.title.clone(),
        category: l.category.clone(),
        side: l.side.clone(),
        realized_pnl: l.realized_pnl.unwrap_or(0.0),
        stake_dollars: l.stake_dollars,
        entry_price_cents: l.entry_price_cents,
        closed_price_cents: l.closed_price_cents,
        closed_at: l.closed_at.clone(),
        settlement_result: l.settlement_result.clone(),
    };

    // Sort winners DESC by realized_pnl, ties broken by closed_at ASC
    // then lot_id ASC. The tuple-comparison key keeps the tiebreak
    // chain in a single sort call. We already filtered out non-positive
    // PnLs so every entry here is a real win.
    winners_only.sort_by(|a, b| {
        b.realized_pnl
            .unwrap_or(0.0)
            .partial_cmp(&a.realized_pnl.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.closed_at.cmp(&b.closed_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    let top_winners: Vec<PaperTopLot> = winners_only
        .iter()
        .take(TOP_LOTS_LIMIT)
        .map(|l| to_top_lot(l))
        .collect();

    // Sort losers ASC by realized_pnl, same tiebreak. The "older first"
    // tiebreak direction matches the winners branch so the two lists
    // share a consistent "depth-first" mental model.
    losers_only.sort_by(|a, b| {
        a.realized_pnl
            .unwrap_or(0.0)
            .partial_cmp(&b.realized_pnl.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.closed_at.cmp(&b.closed_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    let top_losers: Vec<PaperTopLot> = losers_only
        .iter()
        .take(TOP_LOTS_LIMIT)
        .map(|l| to_top_lot(l))
        .collect();

    (top_winners, top_losers)
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
                settlement_result TEXT,
                notes TEXT,
                tags TEXT
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to create paper_lots table: {}", e))?;

        // Migration: add notes and tags columns if they don't exist (for existing DBs)
        sqlx::query("ALTER TABLE paper_lots ADD COLUMN notes TEXT")
            .execute(pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE paper_lots ADD COLUMN tags TEXT")
            .execute(pool)
            .await
            .ok();

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
               realized_pnl, status, settlement_result, notes, tags
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
               realized_pnl, status, settlement_result, notes, tags
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
               realized_pnl, status, settlement_result, notes, tags
        FROM paper_lots WHERE status = 'Open' ORDER BY opened_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch open paper lots: {}", e))?;

    Ok(rows.iter().map(row_to_lot).collect())
}

/// Flexible lot listing for the UI's journal view. Supports an optional
/// `status_filter` (e.g. `Some("Open")` to only see open positions) and an
/// optional `limit` (most recent first). Both are passed through to the SQL
/// `WHERE` / `LIMIT` clauses; a `None` value for either disables the filter
/// (returns every lot, in `opened_at DESC` order). All 18 columns — including
/// the newly-added `notes` and `tags` — are projected so the journal editor
/// can read both fields back from the same row.
pub async fn list_lots(
    pool: &Pool<Sqlite>,
    status_filter: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<PaperLot>, String> {
    // Build the optional WHERE clause. Using a static `"1=1"` prefix lets us
    // unconditionally `AND` the status filter without branching the SQL
    // syntax — simpler than two `match` arms with different query strings.
    let where_clause = match status_filter {
        Some(_) => "WHERE status = ?",
        None => "WHERE 1=1",
    };
    let limit_clause = match limit {
        Some(_) => "LIMIT ?",
        None => "",
    };
    let sql = format!(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result, notes, tags
        FROM paper_lots
        {}
        ORDER BY opened_at DESC
        {}
        "#,
        where_clause, limit_clause
    );

    let mut q = sqlx::query(&sql);
    if let Some(s) = status_filter {
        q = q.bind(s);
    }
    if let Some(n) = limit {
        q = q.bind(n);
    }

    let rows = q
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to list paper lots: {}", e))?;

    Ok(rows.iter().map(row_to_lot).collect())
}

/// Update notes and/or tags on a paper lot.
pub async fn update_lot_notes(
    pool: &Pool<Sqlite>,
    lot_id: &str,
    notes: Option<String>,
    tags: Option<String>,
) -> Result<PaperLot, String> {
    // Validate that at least one field is provided
    if notes.is_none() && tags.is_none() {
        return Err("At least one of notes or tags must be provided".into());
    }

    let mut query = "UPDATE paper_lots SET ".to_string();
    let mut bindings: Vec<String> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref n) = notes {
        query.push_str(&format!("notes = ?{}, ", param_idx));
        bindings.push(n.clone());
        param_idx += 1;
    }
    if let Some(ref t) = tags {
        query.push_str(&format!("tags = ?{}, ", param_idx));
        bindings.push(t.clone());
        param_idx += 1;
    }

    // Remove trailing comma and space
    query = query.trim_end_matches(", ").to_string();
    query.push_str(&format!(" WHERE id = ?{}", param_idx));

    let mut q = sqlx::query(&query);
    for b in bindings {
        q = q.bind(b);
    }
    q = q.bind(lot_id);

    q.execute(pool)
        .await
        .map_err(|e| format!("Failed to update paper lot notes/tags: {}", e))?;

    get_lot(pool, lot_id).await
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
        notes: r.get("notes"),
        tags: r.get("tags"),
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
    let player_stats = compute_player_stats(&all);
    let entry_price_stats = compute_entry_price_stats(&all);
    let calibration_points = compute_calibration_points(&all);
    let paper_disagreement_stats = compute_disagreement_stats(&all);
    let tag_stats = compute_tag_stats(&all);
    let confidence_tier_stats = compute_confidence_tier_stats(&all);
    let source_stats = compute_source_stats(&all);
    let (top_winners, top_losers) = compute_top_lots(&all);

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
        player_stats,
        entry_price_stats,
        calibration_points,
        paper_disagreement_stats,
        tag_stats,
        confidence_tier_stats,
        source_stats,
        top_winners,
        top_losers,
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
            notes: None,
            tags: None,
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

            notes: None,
            tags: None,
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
            notes: None,
            tags: None,
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

            notes: None,
            tags: None,
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
            notes: None,
            tags: None,
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

            notes: None,
            tags: None,
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
            notes: None,
            tags: None,
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

    // ── extract_player_name tests ─────────────────────────────────

    /// Helper: build a `PaperLot` with a free-form title. Most other
    /// helpers hardcode `title = "T"`, so these tests need to vary the
    /// title to exercise the player-name parser.
    fn titled_lot(title: &str) -> PaperLot {
        PaperLot {
            id: format!("t-{}", title.len()),
            ticker: "TEST".to_string(),
            title: title.to_string(),
            category: "Points".to_string(),
            side: "YES".to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: 1.0,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
            closed_at: Some("2026-01-01T01:00:00Z".to_string()),
            closed_price_cents: Some(100.0),
            realized_pnl: Some(1.0),
            status: "Closed".to_string(),
            settlement_result: Some("Win".to_string()),
            notes: None,
            tags: None,
        }
    }

    #[test]
    fn extract_player_over_separates_name() {
        // Canonical "Over" form: player name lives before " Over ".
        let l = titled_lot("Josh Allen Over 275.5 passing yards");
        assert_eq!(extract_player_name(&l), "Josh Allen");
    }

    #[test]
    fn extract_player_under_separates_name() {
        // "Under" is also a valid separator.
        let l = titled_lot("LeBron James Under 25.5 points");
        assert_eq!(extract_player_name(&l), "LeBron James");
    }

    #[test]
    fn extract_player_case_insensitive_side() {
        // Side keyword can appear in any case.
        let l = titled_lot("patrick mahomes OVER 285.5 pass yds");
        assert_eq!(extract_player_name(&l), "patrick mahomes");
    }

    #[test]
    fn extract_player_unknown_for_empty_title() {
        let l = titled_lot("");
        assert_eq!(extract_player_name(&l), "Unknown");
    }

    #[test]
    fn extract_player_unknown_for_whitespace_title() {
        let l = titled_lot("   ");
        assert_eq!(extract_player_name(&l), "Unknown");
    }

    #[test]
    fn extract_player_unknown_when_no_separator() {
        // No " Over " / " Under " keyword → fall back to Unknown rather
        // than returning the whole raw title.
        let l = titled_lot("Travis Kelce receiving yards");
        assert_eq!(extract_player_name(&l), "Unknown");
    }

    #[test]
    fn extract_player_strips_leading_trailing_whitespace() {
        let l = titled_lot("  Stephen Curry   Over  4.5 threes ");
        // The prefix is "  Stephen Curry  " → after trim → "Stephen Curry".
        assert_eq!(extract_player_name(&l), "Stephen Curry");
    }

    // ── compute_player_stats tests ────────────────────────────────

    #[test]
    fn player_stats_empty_input_returns_empty() {
        let lots: Vec<PaperLot> = vec![];
        assert!(compute_player_stats(&lots).is_empty());
    }

    #[test]
    fn player_stats_buckets_by_extracted_name() {
        // Two lots for "Josh Allen" and one for "LeBron James". Each
        // player should be its own bucket with the right totals.
        let lots = vec![
            titled_lot("Josh Allen Over 275.5 passing yards"),
            titled_lot("Josh Allen Under 0.5 interceptions"),
            titled_lot("LeBron James Over 25.5 points"),
        ];
        let stats = compute_player_stats(&lots);
        assert_eq!(stats.len(), 2);
        // All three lots are wins (pnl=1.0), so any sort order is fine —
        // the realized_pnl tiebreaker picks alphabetical. Find each
        // player and verify the totals.
        let allen = stats.iter().find(|s| s.player == "Josh Allen").unwrap();
        assert_eq!(allen.total_trades, 2);
        assert_eq!(allen.wins, 2);
        assert_eq!(allen.losses, 0);
        assert!((allen.win_rate - 100.0).abs() < 1e-9);
        let lebron = stats.iter().find(|s| s.player == "LeBron James").unwrap();
        assert_eq!(lebron.total_trades, 1);
        assert_eq!(lebron.wins, 1);
    }

    #[test]
    fn player_stats_sort_by_pnl_desc_with_alphabetical_tiebreak() {
        // Three players, distinct realized_pnl — sort puts highest first.
        // Two players share a pnl to verify alphabetical tiebreak.
        let mut a = titled_lot("Alpha Player Over 1.0");
        a.realized_pnl = Some(5.0);
        a.settlement_result = Some("Win".to_string());
        let mut b = titled_lot("Bravo Player Over 1.0");
        b.realized_pnl = Some(10.0);
        b.settlement_result = Some("Win".to_string());
        let mut c = titled_lot("Charlie Player Over 1.0");
        c.realized_pnl = Some(5.0);
        c.settlement_result = Some("Win".to_string());
        let stats = compute_player_stats(&[a, b, c]);
        assert_eq!(stats.len(), 3);
        // Bravo (10) > Alpha/Charlie (5) > ... Alpha < Charlie alphabetically
        assert_eq!(stats[0].player, "Bravo Player");
        assert_eq!(stats[1].player, "Alpha Player");
        assert_eq!(stats[2].player, "Charlie Player");
    }

    #[test]
    fn player_stats_unknown_bucket_for_unparseable_titles() {
        // Lots with no " Over " / " Under " keyword all fall under
        // "Unknown" so the user can see they exist in the journal.
        let mut no_sep = titled_lot("Travis Kelce receiving yards");
        no_sep.realized_pnl = Some(2.0);
        no_sep.settlement_result = Some("Win".to_string());
        let stats = compute_player_stats(&[no_sep]);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].player, "Unknown");
        assert_eq!(stats[0].wins, 1);
    }

    #[test]
    fn player_stats_open_lot_routed_to_correct_player_with_zero_pnl() {
        // Open lots count toward `open_trades` + `total_trades` but
        // contribute nothing to wins/losses/realized_pnl.
        let mut open = titled_lot("Josh Allen Over 275.5 passing yards");
        open.status = "Open".to_string();
        open.closed_at = None;
        open.closed_price_cents = None;
        open.realized_pnl = None;
        open.settlement_result = None;
        let stats = compute_player_stats(&[open]);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].player, "Josh Allen");
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].open_trades, 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
    }

    #[test]
    fn player_stats_win_rate_and_roi_computed() {
        // 2 wins + 1 loss for the same player → win_rate = 2/3 ≈ 66.67%,
        // realized_pnl = 5 - 3 = 2, roi = 2 / (5+5+5) * 100 ≈ 13.33%.
        let mut w1 = titled_lot("Player A Over 1.0");
        w1.realized_pnl = Some(5.0);
        w1.stake_dollars = 5.0;
        w1.settlement_result = Some("Win".to_string());
        let mut w2 = titled_lot("Player A Over 1.0");
        w2.realized_pnl = Some(5.0);
        w2.stake_dollars = 5.0;
        w2.settlement_result = Some("Win".to_string());
        let mut l1 = titled_lot("Player A Under 1.0");
        l1.realized_pnl = Some(-3.0);
        l1.stake_dollars = 5.0;
        l1.settlement_result = Some("Loss".to_string());
        let stats = compute_player_stats(&[w1, w2, l1]);
        assert_eq!(stats.len(), 1);
        assert!((stats[0].win_rate - (2.0 / 3.0 * 100.0)).abs() < 1e-6);
        assert!((stats[0].realized_pnl - 7.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 15.0).abs() < 1e-9);
        assert!((stats[0].roi_pct - (7.0 / 15.0 * 100.0)).abs() < 1e-6);
    }

    #[test]
    fn player_stats_push_contributes_stake_but_not_win_or_loss() {
        // pnl == 0 → stake counts, win_rate stays 0/0 → 0%, PnL 0.
        let mut p = titled_lot("Push Player Over 1.0");
        p.realized_pnl = Some(0.0);
        p.stake_dollars = 5.0;
        p.settlement_result = Some("Push".to_string());
        let stats = compute_player_stats(&[p]);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].win_rate - 0.0).abs() < 1e-9);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 5.0).abs() < 1e-9);
    }

    // ── compute_entry_price_stats tests ─────────────────────────

    /// Helper: closed lot parameterized by entry price (cents) so we can
    /// drive the entry-price bucket math directly. Other fields (title,
    /// category, side, etc.) are placeholders — the entry-price test path
    /// only reads `entry_price_cents`, `stake_dollars`, `realized_pnl`,
    /// and `status`.
    fn closed_lot_price(entry_cents: f64, stake: f64, pnl: f64) -> PaperLot {
        PaperLot {
            id: format!("p{entry_cents}-{pnl}"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: "Over".to_string(),
            entry_price_cents: entry_cents,
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

            notes: None,
            tags: None,
        }
    }

    fn open_lot_price(entry_cents: f64, stake: f64) -> PaperLot {
        PaperLot {
            id: format!("p{entry_cents}-open"),
            ticker: "TEST".to_string(),
            title: "T".to_string(),
            category: "Points".to_string(),
            side: "Over".to_string(),
            entry_price_cents: entry_cents,
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
            notes: None,
            tags: None,
        }
    }

    #[test]
    fn entry_price_stats_empty_input_returns_empty_vec() {
        let stats = compute_entry_price_stats(&[]);
        assert!(stats.is_empty());
    }

    #[test]
    fn entry_price_stats_buckets_lots_into_20_cent_bands() {
        // 5¢ → 0-20¢, 25¢ → 20-40¢, 45¢ → 40-60¢, 65¢ → 60-80¢, 85¢ → 80-100¢
        let lots = vec![
            closed_lot_price(5.0, 5.0, 1.0),
            closed_lot_price(25.0, 5.0, 1.0),
            closed_lot_price(45.0, 5.0, 1.0),
            closed_lot_price(65.0, 5.0, 1.0),
            closed_lot_price(85.0, 5.0, 1.0),
        ];
        let stats = compute_entry_price_stats(&lots);
        assert_eq!(stats.len(), 5);
        // Output is sorted by min_cents ASC (BTreeMap iteration order).
        assert_eq!(stats[0].bucket, "0-20¢");
        assert_eq!(stats[1].bucket, "20-40¢");
        assert_eq!(stats[2].bucket, "40-60¢");
        assert_eq!(stats[3].bucket, "60-80¢");
        assert_eq!(stats[4].bucket, "80-100¢");
        // Each bucket has exactly one winning closed lot.
        for s in &stats {
            assert_eq!(s.total_trades, 1);
            assert_eq!(s.wins, 1);
            assert_eq!(s.losses, 0);
        }
    }

    #[test]
    fn entry_price_stats_omits_empty_buckets() {
        // Only lots in two bands — the other three should NOT appear.
        let lots = vec![
            closed_lot_price(10.0, 5.0, 1.0),
            closed_lot_price(30.0, 5.0, -1.0),
        ];
        let stats = compute_entry_price_stats(&lots);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].bucket, "0-20¢");
        assert_eq!(stats[1].bucket, "20-40¢");
    }

    #[test]
    fn entry_price_stats_open_lot_counted_in_open_trades_only() {
        // Open lots count toward total_trades + open_trades, but contribute
        // zero realized PnL, zero wins/losses, and no stake to the ROI
        // denominator (only closed stake counts for ROI per the doc).
        let lots = vec![
            open_lot_price(15.0, 5.0),
            closed_lot_price(75.0, 5.0, 1.0),
        ];
        let stats = compute_entry_price_stats(&lots);
        assert_eq!(stats.len(), 2);
        let open_bucket = stats.iter().find(|s| s.bucket == "0-20¢").unwrap();
        assert_eq!(open_bucket.total_trades, 1);
        assert_eq!(open_bucket.open_trades, 1);
        assert_eq!(open_bucket.wins, 0);
        assert_eq!(open_bucket.losses, 0);
        assert!((open_bucket.realized_pnl - 0.0).abs() < 1e-9);
        assert!((open_bucket.total_staked - 0.0).abs() < 1e-9);
        // Closed bucket: 1 trade, 1 win, +$1, $5 staked → 20% ROI.
        let closed_bucket = stats.iter().find(|s| s.bucket == "60-80¢").unwrap();
        assert_eq!(closed_bucket.total_trades, 1);
        assert_eq!(closed_bucket.wins, 1);
        assert!((closed_bucket.realized_pnl - 1.0).abs() < 1e-9);
        assert!((closed_bucket.total_staked - 5.0).abs() < 1e-9);
        assert!((closed_bucket.roi_pct - 20.0).abs() < 1e-6);
    }

    #[test]
    fn entry_price_stats_push_lot_contributes_stake_but_not_win_or_loss() {
        // pnl == 0 → stake counts for ROI, win_rate stays 0/0 → 0%.
        let mut p = closed_lot_price(35.0, 5.0, 0.0);
        p.settlement_result = Some("Push".to_string());
        let stats = compute_entry_price_stats(&[p]);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].win_rate - 0.0).abs() < 1e-9);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 5.0).abs() < 1e-9);
        assert!((stats[0].roi_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn entry_price_stats_win_rate_and_roi_computed_per_bucket() {
        // 0-20¢ bucket: 2 wins / 0 losses, +$6 on $10 → 60% ROI, 100% win
        // 60-80¢ bucket: 1 win / 1 loss, -$2 on $10 → -20% ROI, 50% win
        let lots = vec![
            closed_lot_price(10.0, 5.0, 3.0),  // 0-20¢ win
            closed_lot_price(15.0, 5.0, 3.0),  // 0-20¢ win
            closed_lot_price(70.0, 5.0, 2.0),  // 60-80¢ win
            closed_lot_price(75.0, 5.0, -4.0), // 60-80¢ loss
        ];
        let stats = compute_entry_price_stats(&lots);
        let cheap = stats.iter().find(|s| s.bucket == "0-20¢").unwrap();
        assert_eq!(cheap.total_trades, 2);
        assert_eq!(cheap.wins, 2);
        assert_eq!(cheap.losses, 0);
        assert!((cheap.win_rate - 100.0).abs() < 1e-6);
        assert!((cheap.realized_pnl - 6.0).abs() < 1e-9);
        assert!((cheap.total_staked - 10.0).abs() < 1e-9);
        assert!((cheap.roi_pct - 60.0).abs() < 1e-6);

        let pricey = stats.iter().find(|s| s.bucket == "60-80¢").unwrap();
        assert_eq!(pricey.total_trades, 2);
        assert_eq!(pricey.wins, 1);
        assert_eq!(pricey.losses, 1);
        assert!((pricey.win_rate - 50.0).abs() < 1e-6);
        assert!((pricey.realized_pnl - -2.0).abs() < 1e-9);
        assert!((pricey.total_staked - 10.0).abs() < 1e-9);
        assert!((pricey.roi_pct - -20.0).abs() < 1e-6);
    }

    #[test]
    fn entry_price_stats_price_at_upper_bound_routes_to_next_bucket() {
        // entry_price_cents = 20.0 falls into the 20-40¢ band (lower inclusive,
        // upper exclusive). The `find((lo, hi, _) => price >= *lo && price < *hi)`
        // check sends 20.0 into 20-40¢, not 0-20¢. 19.99 stays in 0-20¢.
        let lots = vec![
            closed_lot_price(19.99, 5.0, 1.0),
            closed_lot_price(20.0, 5.0, 1.0),
        ];
        let stats = compute_entry_price_stats(&lots);
        assert_eq!(stats.len(), 2);
        let low = stats.iter().find(|s| s.bucket == "0-20¢").unwrap();
        let mid = stats.iter().find(|s| s.bucket == "20-40¢").unwrap();
        assert_eq!(low.total_trades, 1);
        assert_eq!(mid.total_trades, 1);
    }

    #[test]
    fn entry_price_stats_out_of_range_price_falls_into_top_bucket() {
        // Defensive: a price >= 100.0 should land in 80-100¢ (the unwrap_or
        // fallback) rather than panic. Valid PrizePicks prices are strictly
        // 0-100¢ exclusive, but we never want a panic on bad data.
        let lots = vec![closed_lot_price(150.0, 5.0, 1.0)];
        let stats = compute_entry_price_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bucket, "80-100¢");
    }

    // ── compute_calibration_points ──────────────────────────────────────
    //
    // Helper: build a closed lot with a known `decision_json` so we can
    // exercise the JSON parser paths. Mirrors `closed_lot_price` but adds
    // a serialized `PrizePicksTradeDecision` snippet.
    fn closed_lot_with_decision(
        entry_cents: f64,
        stake: f64,
        pnl: f64,
        fair_pct: f64,
        market_pct: f64,
    ) -> PaperLot {
        let mut lot = closed_lot_price(entry_cents, stake, pnl);
        // Build a minimal `PrizePicksTradeDecision`-shaped JSON. We use a
        // raw `serde_json::json!` instead of the struct to keep the test
        // self-contained and avoid depending on optional fields the
        // parser doesn't care about.
        lot.decision_json = Some(
            serde_json::json!({
                "fair_probability_pct": fair_pct,
                "market_price_pct": market_pct,
            })
            .to_string(),
        );
        lot
    }

    #[test]
    fn calibration_points_empty_input_returns_empty_vec() {
        let points = compute_calibration_points(&[]);
        assert!(points.is_empty());
    }

    #[test]
    fn calibration_points_skips_open_lots() {
        // Open lots have no realized_pnl yet, so they should be excluded
        // from the scatter — the X axis would otherwise be misleading.
        let open = open_lot_price(50.0, 5.0);
        let points = compute_calibration_points(&[open]);
        assert!(points.is_empty());
    }

    #[test]
    fn calibration_points_emits_one_per_closed_lot_in_input_order() {
        let lots = vec![
            closed_lot_with_decision(50.0, 10.0, 2.0, 65.0, 0.55),
            closed_lot_with_decision(40.0, 8.0, -1.5, 45.0, 0.50),
            closed_lot_with_decision(60.0, 12.0, 3.5, 70.0, 0.58),
        ];
        let points = compute_calibration_points(&lots);
        assert_eq!(points.len(), 3);
        // Order preserved (input → output).
        assert_eq!(points[0].lot_id, "p50-2");
        assert_eq!(points[1].lot_id, "p40--1.5");
        assert_eq!(points[2].lot_id, "p60-3.5");
    }

    #[test]
    fn calibration_points_won_true_for_positive_pnl_false_for_negative() {
        let win = closed_lot_with_decision(50.0, 10.0, 5.0, 65.0, 0.55);
        let loss = closed_lot_with_decision(40.0, 8.0, -5.0, 45.0, 0.50);
        let points = compute_calibration_points(&[win, loss]);
        assert_eq!(points[0].won, Some(true));
        assert_eq!(points[1].won, Some(false));
    }

    #[test]
    fn calibration_points_push_produces_won_none_and_zero_pnl() {
        // pnl == 0 → push → `won = None` (not Some(true) or Some(false))
        // so the UI can render the point on the X axis without
        // mis-classifying it as a win or loss.
        let push = closed_lot_with_decision(50.0, 10.0, 0.0, 60.0, 0.50);
        let points = compute_calibration_points(&[push]);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].won, None);
        assert_eq!(points[0].realized_pnl_dollars, 0.0);
        assert_eq!(points[0].fair_probability_pct, 60.0);
    }

    #[test]
    fn calibration_points_missing_decision_json_routes_to_zero_fair() {
        // Lots without `decision_json` (e.g. placed before the JSON
        // migration) still produce a point so the closed-lot count
        // matches, but `fair_probability_pct = 0.0` and
        // `market_price_cents = None` so the UI can flag them as
        // "no-decision" if it wants.
        let plain = closed_lot_price(50.0, 10.0, 2.0);
        // closed_lot_price() sets `decision_json = None` by default.
        assert!(plain.decision_json.is_none());
        let points = compute_calibration_points(&[plain]);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].fair_probability_pct, 0.0);
        assert_eq!(points[0].market_price_cents, None);
        // PnL is still recorded.
        assert_eq!(points[0].realized_pnl_dollars, 2.0);
    }

    #[test]
    fn calibration_points_unparseable_decision_json_routes_to_zero_fair() {
        // Malformed JSON should not crash; treat as a missing-decision lot.
        let mut bad = closed_lot_price(50.0, 10.0, 1.0);
        bad.decision_json = Some("{not valid json".to_string());
        let points = compute_calibration_points(&[bad]);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].fair_probability_pct, 0.0);
        assert_eq!(points[0].market_price_cents, None);
    }

    #[test]
    fn calibration_points_market_price_pct_normalizes_0_1_to_cents() {
        // Schema stores `market_price_pct` on a 0-1 scale, but the rest
        // of the analytics use 0-100 (cents). Verify the scale
        // normalization works. (Use approximate equality — `0.55 * 100.0`
        // produces `55.00000000000001` from floating-point math.)
        let lot = closed_lot_with_decision(50.0, 10.0, 2.0, 65.0, 0.55);
        let points = compute_calibration_points(&[lot]);
        let market = points[0].market_price_cents.expect("market_price_cents parsed");
        assert!((market - 55.0).abs() < 1e-9, "got {market}");
    }

    #[test]
    fn calibration_points_market_price_pct_preserves_0_100_scale() {
        // Some older serializations may have written 0-100 directly. If
        // the value is already > 1.0, we treat it as cents and don't
        // multiply by 100.
        let lot = closed_lot_with_decision(50.0, 10.0, 2.0, 65.0, 55.0);
        let points = compute_calibration_points(&[lot]);
        assert_eq!(points[0].market_price_cents, Some(55.0));
    }

    #[test]
    fn calibration_points_out_of_range_fair_clamped_to_0_100() {
        // A stray negative or > 100 value should be clamped so a rogue
        // 1.2 doesn't poison the X axis.
        let neg = closed_lot_with_decision(50.0, 10.0, 1.0, -10.0, 0.50);
        let over = closed_lot_with_decision(50.0, 10.0, 1.0, 150.0, 0.50);
        let points = compute_calibration_points(&[neg, over]);
        assert_eq!(points[0].fair_probability_pct, 0.0);
        assert_eq!(points[1].fair_probability_pct, 100.0);
    }

    #[test]
    fn calibration_points_stake_and_pnl_propagated() {
        // The UI sizes the bubble by `stake_dollars` and positions it by
        // PnL — both must round-trip exactly.
        let lot = closed_lot_with_decision(50.0, 42.0, 7.5, 65.0, 0.55);
        let points = compute_calibration_points(&[lot]);
        assert_eq!(points[0].stake_dollars, 42.0);
        assert_eq!(points[0].realized_pnl_dollars, 7.5);
    }

    // ── compute_disagreement_stats ────────────────────────────────────
    //
    // Helper: build a closed lot with a known disagreement flag baked
    // into the decision JSON. Mirrors `closed_lot_with_decision` but
    // writes the `model_disagreement` bool directly so the bucketing
    // logic stays testable without depending on the threshold math.
    fn disagreement_lot(flag: Option<bool>, stake: f64, pnl: f64) -> PaperLot {
        let mut lot = closed_lot_price(50.0, stake, pnl);
        match flag {
            Some(b) => {
                lot.decision_json = Some(
                    serde_json::json!({
                        "model_disagreement": b,
                        "disagreement_points": if b { 20.0 } else { 5.0 },
                    })
                    .to_string(),
                );
            }
            None => {
                lot.decision_json = None;
            }
        }
        lot
    }

    #[test]
    fn disagreement_stats_empty_input_returns_three_zero_buckets() {
        // Even with no lots, all three canonical buckets must appear
        // (with zeros) so the UI table layout is stable.
        let stats = compute_disagreement_stats(&[]);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].bucket, DisagreementBucket::Disagreement);
        assert_eq!(stats[1].bucket, DisagreementBucket::Consensus);
        assert_eq!(stats[2].bucket, DisagreementBucket::Unknown);
        for s in &stats {
            assert_eq!(s.total_trades, 0);
            assert_eq!(s.wins, 0);
            assert_eq!(s.losses, 0);
            assert_eq!(s.realized_pnl, 0.0);
        }
    }

    #[test]
    fn disagreement_stats_buckets_by_model_disagreement_flag() {
        // Two disagreement lots (one win, one loss) + two consensus lots
        // (one win, one loss). The Disagreement bucket should aggregate
        // the flag=true lots, and the Consensus bucket the flag=false
        // lots. The Unknown bucket should not appear (no unparseable /
        // missing JSON).
        let lots = vec![
            disagreement_lot(Some(true), 10.0, 4.0),  // disagree + win
            disagreement_lot(Some(true), 10.0, -3.0), // disagree + loss
            disagreement_lot(Some(false), 10.0, 2.0), // consensus + win
            disagreement_lot(Some(false), 10.0, -1.0), // consensus + loss
        ];
        let stats = compute_disagreement_stats(&lots);
        // Disagreement bucket
        assert_eq!(stats[0].bucket, DisagreementBucket::Disagreement);
        assert_eq!(stats[0].total_trades, 2);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
        assert!((stats[0].realized_pnl - 1.0).abs() < 1e-9);
        assert!((stats[0].win_rate - 50.0).abs() < 1e-9);
        // ROI = 1.0 / 20.0 * 100 = 5.0%
        assert!((stats[0].roi_pct - 5.0).abs() < 1e-9);
        // Consensus bucket
        assert_eq!(stats[1].bucket, DisagreementBucket::Consensus);
        assert_eq!(stats[1].total_trades, 2);
        assert_eq!(stats[1].wins, 1);
        assert_eq!(stats[1].losses, 1);
        assert!((stats[1].realized_pnl - 1.0).abs() < 1e-9);
        // Unknown bucket
        assert_eq!(stats[2].bucket, DisagreementBucket::Unknown);
        assert_eq!(stats[2].total_trades, 0);
    }

    #[test]
    fn disagreement_stats_missing_decision_json_routes_to_unknown() {
        // Lots with no `decision_json` (or unparseable JSON) bucket under
        // Unknown so the closed-lot count still matches the rest of the
        // analytics.
        let mut no_json = closed_lot_price(50.0, 5.0, 1.0);
        no_json.decision_json = None;
        let mut bad_json = closed_lot_price(50.0, 5.0, 1.0);
        bad_json.decision_json = Some("{not valid json".to_string());
        let mut empty_flag = closed_lot_price(50.0, 5.0, 1.0);
        empty_flag.decision_json = Some(r#"{"some_other_field": 42}"#.to_string());
        let lots = vec![no_json, bad_json, empty_flag];
        let stats = compute_disagreement_stats(&lots);
        assert_eq!(stats[2].bucket, DisagreementBucket::Unknown);
        assert_eq!(stats[2].total_trades, 3);
        assert_eq!(stats[2].wins, 3);
        assert_eq!(stats[2].losses, 0);
        // Disagreement and Consensus buckets are zero (no parseable flag).
        assert_eq!(stats[0].total_trades, 0);
        assert_eq!(stats[1].total_trades, 0);
    }

    #[test]
    fn disagreement_stats_open_lot_counted_in_open_trades_only() {
        // Open lots count toward total_trades + open_trades, but
        // contribute nothing to wins/losses/PnL/ROI. They still bucket
        // by their disagreement flag.
        let lots = vec![
            {
                let mut l = open_lot_price(50.0, 5.0);
                l.decision_json = Some(
                    serde_json::json!({"model_disagreement": true, "disagreement_points": 20.0})
                        .to_string(),
                );
                l
            },
        ];
        let stats = compute_disagreement_stats(&lots);
        assert_eq!(stats[0].bucket, DisagreementBucket::Disagreement);
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].open_trades, 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 0.0).abs() < 1e-9);
    }

    #[test]
    fn disagreement_stats_push_lot_contributes_stake_but_not_win_or_loss() {
        // pnl == 0 (push) → stake counts toward total_staked, but
        // neither wins nor losses count it.
        let lots = vec![
            disagreement_lot(Some(false), 10.0, 0.0), // consensus + push
        ];
        let stats = compute_disagreement_stats(&lots);
        assert_eq!(stats[1].bucket, DisagreementBucket::Consensus);
        assert_eq!(stats[1].total_trades, 1);
        assert_eq!(stats[1].wins, 0);
        assert_eq!(stats[1].losses, 0);
        assert!((stats[1].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[1].total_staked - 10.0).abs() < 1e-9);
        // ROI = 0 / 10 * 100 = 0
        assert!((stats[1].roi_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn disagreement_stats_falls_back_to_threshold_when_flag_absent() {
        // Legacy serializations may write only `disagreement_points`
        // (no bool). The helper should compare the absolute value
        // against the 12pp threshold as a fallback.
        let mut legacy_disagree = closed_lot_price(50.0, 5.0, 1.0);
        legacy_disagree.decision_json =
            Some(r#"{"disagreement_points": 15.0}"#.to_string());
        let mut legacy_consensus = closed_lot_price(50.0, 5.0, 1.0);
        legacy_consensus.decision_json =
            Some(r#"{"disagreement_points": 8.0}"#.to_string());
        let mut legacy_boundary = closed_lot_price(50.0, 5.0, 1.0);
        // 12.0 is NOT > 12.0 (the threshold is strictly greater), so
        // this should bucket as Consensus.
        legacy_boundary.decision_json =
            Some(r#"{"disagreement_points": 12.0}"#.to_string());
        let lots = vec![legacy_disagree, legacy_consensus, legacy_boundary];
        let stats = compute_disagreement_stats(&lots);
        assert_eq!(stats[0].bucket, DisagreementBucket::Disagreement);
        assert_eq!(stats[0].total_trades, 1); // legacy_disagree (15.0pp)
        assert_eq!(stats[1].bucket, DisagreementBucket::Consensus);
        assert_eq!(stats[1].total_trades, 2); // legacy_consensus + legacy_boundary
    }

    #[test]
    fn disagreement_stats_bucket_label_matches_enum_variant() {
        // The UI should be able to render `bucket_label` without any
        // hard-coded mapping. Verify the three canonical labels.
        let stats = compute_disagreement_stats(&[]);
        assert_eq!(stats[0].bucket_label, "Disagreement (>12pp)");
        assert_eq!(stats[1].bucket_label, "Consensus (≤12pp)");
        assert_eq!(stats[2].bucket_label, "Unknown");
    }

    #[test]
    fn disagreement_stats_emits_canonical_order_regardless_of_input() {
        // Output must always be Disagreement → Consensus → Unknown
        // (NOT sorted by PnL DESC like the other breakdowns) so the UI
        // can render a stable "disagree → agree → unknown" ladder.
        // We mix the input order: lots that would sort "Unknown" first
        // (no decision_json) before "Disagreement" lots, and verify the
        // output order is fixed.
        let mut unknown_first = closed_lot_price(50.0, 5.0, 1.0);
        unknown_first.decision_json = None;
        let lots = vec![
            unknown_first,
            disagreement_lot(Some(true), 5.0, 1.0),
            disagreement_lot(Some(false), 5.0, 1.0),
        ];
        let stats = compute_disagreement_stats(&lots);
        assert_eq!(stats[0].bucket, DisagreementBucket::Disagreement);
        assert_eq!(stats[1].bucket, DisagreementBucket::Consensus);
        assert_eq!(stats[2].bucket, DisagreementBucket::Unknown);
    }

    // ── update_lot_notes tests ─────────────────────────────────────

    /// Build an in-memory pool with the production paper schema so the
    /// full update roundtrip can be exercised end-to-end.
    async fn fresh_paper_pool() -> Pool<Sqlite> {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_paper_tables(&pool).await.unwrap();
        // `init_paper_tables` already bootstraps the singleton paper_account
        // row with DEFAULT_STARTING_BALANCE — no need to seed it here.
        pool
    }

    /// Insert a minimal paper_lot row and return its id. Mirrors the
    /// production schema used by `place_trade` — title is empty so the
    /// analytics extractors default to "Unknown" buckets.
    async fn insert_test_lot(pool: &Pool<Sqlite>, id: &str) {
        sqlx::query(
            r#"
            INSERT INTO paper_lots
                (id, ticker, title, category, side, entry_price_cents, qty,
                 stake_dollars, source, decision_json, opened_at, closed_at,
                 closed_price_cents, realized_pnl, status, settlement_result)
            VALUES
                (?1, 'TEST', 'T', 'Points', 'Over', 50.0, 1.0, 1.0, 'Manual',
                 NULL, '2026-01-01T00:00:00Z', NULL, NULL, NULL, 'Open', NULL)
            "#,
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Validation: passing both fields as `None` must short-circuit with
    /// the documented error before touching the DB.
    #[tokio::test]
    async fn update_lot_notes_rejects_both_none() {
        let pool = fresh_paper_pool().await;
        let err = update_lot_notes(&pool, "any-id", None, None).await.unwrap_err();
        // Case-insensitive check: the source-of-truth string starts with
        // "At least one" (capital A); we just want to confirm the user
        // got a meaningful validation error before any DB write.
        assert!(err.to_lowercase().contains("at least one"), "unexpected error: {err}");
    }

    /// Roundtrip: updating notes only persists the new value and leaves
    /// tags unchanged.
    #[tokio::test]
    async fn update_lot_notes_writes_notes_only() {
        let pool = fresh_paper_pool().await;
        insert_test_lot(&pool, "lot-notes-only").await;
        let updated = update_lot_notes(
            &pool,
            "lot-notes-only",
            Some("injury watch — late scratch risk".to_string()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.notes.as_deref(), Some("injury watch — late scratch risk"));
        assert!(updated.tags.is_none(), "tags should remain untouched");
    }

    /// Roundtrip: updating tags only persists the new value and leaves
    /// notes unchanged.
    #[tokio::test]
    async fn update_lot_notes_writes_tags_only() {
        let pool = fresh_paper_pool().await;
        insert_test_lot(&pool, "lot-tags-only").await;
        let updated = update_lot_notes(
            &pool,
            "lot-tags-only",
            None,
            Some("regression,underdog".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(updated.tags.as_deref(), Some("regression,underdog"));
        assert!(updated.notes.is_none(), "notes should remain untouched");
    }

    /// Roundtrip: updating both fields writes both, with the new values
    /// reflected in the returned `PaperLot`.
    #[tokio::test]
    async fn update_lot_notes_writes_both_fields() {
        let pool = fresh_paper_pool().await;
        insert_test_lot(&pool, "lot-both").await;
        let updated = update_lot_notes(
            &pool,
            "lot-both",
            Some("sharp money, value play".to_string()),
            Some("value,sharp".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(updated.notes.as_deref(), Some("sharp money, value play"));
        assert_eq!(updated.tags.as_deref(), Some("value,sharp"));
    }

    /// Roundtrip: a second update should overwrite the first, not append.
    #[tokio::test]
    async fn update_lot_notes_overwrites_previous_values() {
        let pool = fresh_paper_pool().await;
        insert_test_lot(&pool, "lot-overwrite").await;
        let _ = update_lot_notes(
            &pool,
            "lot-overwrite",
            Some("first draft".to_string()),
            Some("a,b".to_string()),
        )
        .await
        .unwrap();
        let updated = update_lot_notes(
            &pool,
            "lot-overwrite",
            Some("second draft".to_string()),
            Some("c".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(updated.notes.as_deref(), Some("second draft"));
        assert_eq!(updated.tags.as_deref(), Some("c"));
    }

    /// Insert a paper lot with a chosen status and opened_at. Mirrors the
    /// `insert_test_lot` schema but lets the tests vary the status (Open vs
    /// Closed) and the timestamp (so we can assert DESC ordering). The
    /// `opened_at` is RFC 3339 — sqlite stores it as TEXT and ORDER BY uses
    /// lexicographic comparison, which is correct for ISO 8601.
    async fn insert_test_lot_with_status(
        pool: &Pool<Sqlite>,
        id: &str,
        status: &str,
        opened_at: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO paper_lots
                (id, ticker, title, category, side, entry_price_cents, qty,
                 stake_dollars, source, decision_json, opened_at, closed_at,
                 closed_price_cents, realized_pnl, status, settlement_result)
            VALUES
                (?1, 'TEST', 'T', 'Points', 'Over', 50.0, 1.0, 1.0, 'Manual',
                 NULL, ?2, NULL, NULL, NULL, ?3, NULL)
            "#,
        )
        .bind(id)
        .bind(opened_at)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    /// No filter, no limit — every lot in insertion order (which is
    /// `opened_at DESC` since we set later timestamps on later inserts).
    #[tokio::test]
    async fn list_lots_no_filters_returns_all_lots() {
        let pool = fresh_paper_pool().await;
        insert_test_lot_with_status(&pool, "a", "Open", "2026-01-01T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "b", "Closed", "2026-01-02T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "c", "Open", "2026-01-03T00:00:00Z").await;
        let all = list_lots(&pool, None, None).await.unwrap();
        assert_eq!(all.len(), 3);
        // DESC by opened_at: c (Jan 3) → b (Jan 2) → a (Jan 1).
        assert_eq!(all[0].id, "c");
        assert_eq!(all[1].id, "b");
        assert_eq!(all[2].id, "a");
    }

    /// Status filter restricts the result to lots whose `status` column
    /// matches exactly (case-sensitive SQL equality).
    #[tokio::test]
    async fn list_lots_with_status_filter_returns_only_matching() {
        let pool = fresh_paper_pool().await;
        insert_test_lot_with_status(&pool, "open-1", "Open", "2026-01-01T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "closed-1", "Closed", "2026-01-02T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "open-2", "Open", "2026-01-03T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "closed-2", "Closed", "2026-01-04T00:00:00Z").await;
        let open_only = list_lots(&pool, Some("Open"), None).await.unwrap();
        assert_eq!(open_only.len(), 2);
        assert!(open_only.iter().all(|l| l.status == "Open"));
        assert_eq!(open_only[0].id, "open-2");
        assert_eq!(open_only[1].id, "open-1");
    }

    /// Limit is honored: passing Some(2) on a 3-lot table returns only 2.
    /// Combined with a status filter to make sure the WHERE still applies
    /// before the LIMIT (otherwise an empty-filter+limit could mask a bug).
    #[tokio::test]
    async fn list_lots_with_limit_caps_result_size() {
        let pool = fresh_paper_pool().await;
        insert_test_lot_with_status(&pool, "a", "Open", "2026-01-01T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "b", "Open", "2026-01-02T00:00:00Z").await;
        insert_test_lot_with_status(&pool, "c", "Open", "2026-01-03T00:00:00Z").await;
        let capped = list_lots(&pool, Some("Open"), Some(2)).await.unwrap();
        assert_eq!(capped.len(), 2);
        // Newest two first: c, b.
        assert_eq!(capped[0].id, "c");
        assert_eq!(capped[1].id, "b");
    }

    /// The 18 columns (including the newly-added `notes` and `tags`) all
    /// survive the round-trip — protects against the same SELECT-list
    /// omission that broke `update_lot_notes` initially. Inserts a lot,
    /// sets non-null notes + tags, then reads it back via list_lots.
    #[tokio::test]
    async fn list_lots_round_trips_notes_and_tags() {
        let pool = fresh_paper_pool().await;
        insert_test_lot_with_status(&pool, "rt", "Closed", "2026-01-05T00:00:00Z").await;
        let _ = update_lot_notes(
            &pool,
            "rt",
            Some("regression play, line moved 1.5pts".to_string()),
            Some("regression,line-move".to_string()),
        )
        .await
        .unwrap();
        let listed = list_lots(&pool, None, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].notes.as_deref(),
            Some("regression play, line moved 1.5pts")
        );
        assert_eq!(listed[0].tags.as_deref(), Some("regression,line-move"));
    }

    /// Empty pool — no lots, no filter, no limit. Result is an empty Vec
    /// (not an error). The journal UI relies on this for the empty state.
    #[tokio::test]
    async fn list_lots_empty_pool_returns_empty_vec() {
        let pool = fresh_paper_pool().await;
        let result = list_lots(&pool, None, None).await.unwrap();
        assert!(result.is_empty());
        // Status filter that matches nothing is also empty, not an error.
        let no_open = list_lots(&pool, Some("Open"), None).await.unwrap();
        assert!(no_open.is_empty());
    }

    // ── split_tags + compute_tag_stats ──────────────────────────────
    //
    // These tests cover the per-tag breakdown helper. `split_tags` is
    // extracted as its own pure function so the parser edge cases can be
    // tested in isolation; `compute_tag_stats` then exercises the
    // bucketing + aggregation.

    /// `split_tags` on a basic comma-separated string returns each
    /// segment in order, lowercased + trimmed. Empty segments from
    /// trailing/leading commas are dropped.
    #[test]
    fn split_tags_basic_comma_separated() {
        assert_eq!(
            split_tags(Some("injury,regression,underdog")),
            vec!["injury", "regression", "underdog"],
        );
    }

    /// `split_tags` lowercases capital letters so `"Injury"` and
    /// `"injury"` collapse to the same bucket when the analytics layer
    /// groups them.
    #[test]
    fn split_tags_lowercases_segments() {
        assert_eq!(
            split_tags(Some("Injury,Sharp,Mixed-Case")),
            vec!["injury", "sharp", "mixed-case"],
        );
    }

    /// `split_tags` trims surrounding whitespace around each segment
    /// (common when the user types `"a , b , c"` in the journal UI).
    #[test]
    fn split_tags_trims_whitespace_around_segments() {
        assert_eq!(
            split_tags(Some("  a , b ,  c  ")),
            vec!["a", "b", "c"],
        );
    }

    /// `split_tags` drops empty segments that result from a trailing
    /// comma, a leading comma, or `"a,,b"`. These would otherwise create
    /// a noisy empty-string bucket.
    #[test]
    fn split_tags_drops_empty_segments() {
        assert_eq!(split_tags(Some("a,,b")), vec!["a", "b"]);
        assert_eq!(split_tags(Some(",a,b,")), vec!["a", "b"]);
        assert_eq!(split_tags(Some(",,")), Vec::<String>::new());
    }

    /// `split_tags` returns an empty `Vec` for `None`, empty, or
    /// whitespace-only inputs. The bucketing logic uses this to skip
    /// untagged lots.
    #[test]
    fn split_tags_returns_empty_for_blank_inputs() {
        assert!(split_tags(None).is_empty());
        assert!(split_tags(Some("")).is_empty());
        assert!(split_tags(Some("   ")).is_empty());
        assert!(split_tags(Some(", , ,")).is_empty());
    }

    /// `compute_tag_stats` on an empty input returns an empty `Vec` (no
    /// spurious "Untagged" bucket — that was a deliberate design choice
    /// to avoid dwarfing every other bucket for users who only journal
    /// a fraction of their trades).
    #[test]
    fn tag_stats_empty_input_returns_empty_vec() {
        let stats = compute_tag_stats(&[]);
        assert!(stats.is_empty());
    }

    /// Single tag, single win — bucket should have 1 trade, 1 win,
    /// 100% win rate, full stake as staked, and the positive PnL.
    #[test]
    fn tag_stats_single_tag_single_win() {
        let lots = vec![tagged_closed_lot("injury", 10.0, 4.0)];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].tag, "injury");
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].win_rate - 100.0).abs() < 1e-9);
        assert!((stats[0].realized_pnl - 4.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 10.0).abs() < 1e-9);
        assert!((stats[0].roi_pct - 40.0).abs() < 1e-9);
    }

    /// A lot with two tags contributes to *both* buckets. This is the
    /// most important correctness property — the helper exists
    /// specifically because the user might journal a single trade
    /// under multiple categories. The `total_trades` for each tag
    /// reflects that lot, so the sum of `total_trades` across tag
    /// buckets can exceed the number of unique closed lots.
    #[test]
    fn tag_stats_lot_with_two_tags_contributes_to_both_buckets() {
        let lots = vec![tagged_closed_lot("injury,regression", 10.0, 2.0)];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 2);
        // Sorted by PnL DESC; both are 2.0 → alphabetical tiebreak.
        assert_eq!(stats[0].tag, "injury");
        assert_eq!(stats[1].tag, "regression");
        for s in &stats {
            assert_eq!(s.total_trades, 1);
            assert_eq!(s.wins, 1);
            assert!((s.realized_pnl - 2.0).abs() < 1e-9);
            assert!((s.total_staked - 10.0).abs() < 1e-9);
        }
    }

    /// Sort by PnL DESC with alphabetical tiebreak. Two tags, one wins
    /// big, one wins small. The big winner must come first; the
    /// tiebreak is only relevant for equal-PnL buckets.
    #[test]
    fn tag_stats_sorted_by_pnl_desc_with_alphabetical_tiebreak() {
        let lots = vec![
            tagged_closed_lot("regression", 10.0, -2.0), // loss
            tagged_closed_lot("sharp", 10.0, 5.0),       // big win
            tagged_closed_lot("value", 10.0, 1.0),       // small win
        ];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].tag, "sharp");   // +5.0
        assert_eq!(stats[1].tag, "value");    // +1.0
        assert_eq!(stats[2].tag, "regression"); // -2.0
    }

    /// Tags differing only by case collapse to the same bucket
    /// (lowercased canonicalization). Two lots with `"Injury"` and
    /// `"injury"` should land in a single `injury` bucket.
    #[test]
    fn tag_stats_case_insensitive_bucketing() {
        let lots = vec![
            tagged_closed_lot("Injury", 10.0, 2.0),
            tagged_closed_lot("injury", 10.0, 1.0),
            tagged_closed_lot("INJURY", 10.0, -1.0),
        ];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].tag, "injury");
        assert_eq!(stats[0].total_trades, 3);
        assert_eq!(stats[0].wins, 2);
        assert_eq!(stats[0].losses, 1);
        assert!((stats[0].realized_pnl - 2.0).abs() < 1e-9);
    }

    /// Untagged lots (None, empty, or whitespace-only `tags`) are
    /// silently skipped — no "Untagged" bucket is emitted. This is the
    /// documented behavior; if it ever changes, the UI's empty-state
    /// copy will need to be updated too.
    #[test]
    fn tag_stats_untagged_lots_are_skipped() {
        let lots = vec![
            untagged_closed_lot(None, 10.0, 2.0),
            untagged_closed_lot(Some(""), 10.0, 1.0),
            untagged_closed_lot(Some("   "), 10.0, -1.0),
            untagged_closed_lot(Some(",,,"), 10.0, 3.0),
        ];
        let stats = compute_tag_stats(&lots);
        assert!(stats.is_empty());
    }

    /// Open lots count toward `total_trades` and `open_trades` for
    /// their tag, but contribute nothing to wins/losses/PnL/ROI
    /// (matches the other breakdown helpers' semantics).
    #[test]
    fn tag_stats_open_lot_counted_in_open_trades_only() {
        let lots = vec![tagged_open_lot("injury", 5.0)];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].tag, "injury");
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].open_trades, 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 0.0).abs() < 1e-9);
    }

    /// Push (pnl == 0) contributes stake to the bucket but does not
    /// count as a win or loss. ROI = 0/10 = 0%, win rate = 0/0 = 0%.
    /// This protects the same `is_push` edge case that bit the other
    /// breakdown helpers when re-implemented here.
    #[test]
    fn tag_stats_push_lot_contributes_stake_but_not_win_or_loss() {
        let lots = vec![tagged_closed_lot("push-test", 10.0, 0.0)];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert!((stats[0].win_rate - 0.0).abs() < 1e-9);
        assert!((stats[0].realized_pnl - 0.0).abs() < 1e-9);
        assert!((stats[0].total_staked - 10.0).abs() < 1e-9);
        assert!((stats[0].roi_pct - 0.0).abs() < 1e-9);
    }

    /// End-to-end: mixed multi-tag lots, multi-bucket aggregation.
    /// Verifies the math on a non-trivial input: 4 lots, 3 distinct
    /// tags (with one tag shared across two lots), pushing ROI math
    /// through the aggregator. This is the closest test to a real
    /// user dataset and guards the bookkeeping against off-by-one bugs.
    #[test]
    fn tag_stats_mixed_multi_tag_aggregation() {
        let lots = vec![
            // "injury,regression" → +3 on both tags, $10 stake each
            tagged_closed_lot("injury,regression", 10.0, 3.0),
            // "injury" → -2 on injury, $10 stake
            tagged_closed_lot("injury", 10.0, -2.0),
            // "value" → +1 on value, $20 stake
            tagged_closed_lot("value", 20.0, 1.0),
            // "regression" → -1 on regression, $5 stake (push → 0/0)
            tagged_closed_lot("regression", 5.0, 0.0),
        ];
        let stats = compute_tag_stats(&lots);
        assert_eq!(stats.len(), 3);
        // injury: 2 lots, 1 win, 1 loss, +1 pnl, $20 staked, 50% WR, 5% ROI
        let injury = stats.iter().find(|s| s.tag == "injury").unwrap();
        assert_eq!(injury.total_trades, 2);
        assert_eq!(injury.wins, 1);
        assert_eq!(injury.losses, 1);
        assert!((injury.realized_pnl - 1.0).abs() < 1e-9);
        assert!((injury.total_staked - 20.0).abs() < 1e-9);
        assert!((injury.win_rate - 50.0).abs() < 1e-9);
        assert!((injury.roi_pct - 5.0).abs() < 1e-9);
        // value: 1 lot, 1 win, +1, $20 staked, 100% WR, 5% ROI
        let value = stats.iter().find(|s| s.tag == "value").unwrap();
        assert_eq!(value.total_trades, 1);
        assert_eq!(value.wins, 1);
        assert!((value.realized_pnl - 1.0).abs() < 1e-9);
        assert!((value.total_staked - 20.0).abs() < 1e-9);
        // regression: 2 lots, 1 win, 1 push, +3, $15 staked, 100% WR (1W/0L)
        let regression = stats.iter().find(|s| s.tag == "regression").unwrap();
        assert_eq!(regression.total_trades, 2);
        assert_eq!(regression.wins, 1);
        assert_eq!(regression.losses, 0);
        assert!((regression.realized_pnl - 3.0).abs() < 1e-9);
        assert!((regression.total_staked - 15.0).abs() < 1e-9);
    }

    // ── Test helpers for tag stats ─────────────────────────────────

    /// Build a closed paper lot with the given `tags_string` (raw,
    /// comma-separated) and (stake, pnl). The lot's other fields
    /// are placeholders — the tag-stats tests only read `tags`,
    /// `stake_dollars`, `realized_pnl`, and `status`.
    fn tagged_closed_lot(tags_string: &str, stake: f64, pnl: f64) -> PaperLot {
        let mut lot = closed_lot_price(50.0, stake, pnl);
        lot.tags = Some(tags_string.to_string());
        lot
    }

    /// Build an open paper lot with the given `tags_string`. Open lots
    /// have no `realized_pnl` yet.
    fn tagged_open_lot(tags_string: &str, stake: f64) -> PaperLot {
        let mut lot = open_lot_price(50.0, stake);
        lot.tags = Some(tags_string.to_string());
        lot
    }

    /// Build a closed lot with the given `tags` (Option<String>), used
    /// to exercise the untagged-lot routing (None, empty, whitespace).
    fn untagged_closed_lot(tags: Option<&str>, stake: f64, pnl: f64) -> PaperLot {
        let mut lot = closed_lot_price(50.0, stake, pnl);
        lot.tags = tags.map(|s| s.to_string());
        lot
    }

    // ── compute_confidence_tier_stats ──────────────────────────────
    //
    // Helper: build a closed lot with a known confidence_tier baked
    // into the decision JSON. Mirrors `disagreement_lot` so the
    // bucketing logic stays testable without depending on the
    // upstream `compute()` method.
    fn confidence_lot(tier: Option<&str>, stake: f64, pnl: f64) -> PaperLot {
        let mut lot = closed_lot_price(50.0, stake, pnl);
        match tier {
            Some(t) => {
                lot.decision_json = Some(
                    serde_json::json!({
                        "confidence_tier": t,
                    })
                    .to_string(),
                );
            }
            None => {
                lot.decision_json = None;
            }
        }
        lot
    }

    #[test]
    fn confidence_tier_stats_empty_input_returns_four_zero_tiers() {
        // Even with no lots, all four canonical tiers must appear (with
        // zeros) so the UI table layout is stable. Order is
        // High → Medium → Low → None (highest conviction to lowest).
        let stats = compute_confidence_tier_stats(&[]);
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[0].bucket, ConfidenceTier::High);
        assert_eq!(stats[1].bucket, ConfidenceTier::Medium);
        assert_eq!(stats[2].bucket, ConfidenceTier::Low);
        assert_eq!(stats[3].bucket, ConfidenceTier::None);
        for s in &stats {
            assert_eq!(s.total_trades, 0);
            assert_eq!(s.wins, 0);
            assert_eq!(s.losses, 0);
            assert_eq!(s.realized_pnl, 0.0);
        }
    }

    #[test]
    fn confidence_tier_stats_buckets_by_confidence_tier_field() {
        // Two High lots (one win, one loss) + two Medium lots
        // (one win, one loss). The High bucket should aggregate
        // the "High" lots, the Medium bucket the "Medium" lots.
        // Low and None buckets should be zero.
        let lots = vec![
            confidence_lot(Some("High"), 10.0, 4.0),    // High + win
            confidence_lot(Some("High"), 10.0, -3.0),   // High + loss
            confidence_lot(Some("Medium"), 10.0, 2.0),  // Medium + win
            confidence_lot(Some("Medium"), 10.0, -1.0), // Medium + loss
        ];
        let stats = compute_confidence_tier_stats(&lots);
        // High bucket
        assert_eq!(stats[0].bucket, ConfidenceTier::High);
        assert_eq!(stats[0].total_trades, 2);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
        assert!((stats[0].realized_pnl - 1.0).abs() < 1e-9);
        assert!((stats[0].win_rate - 50.0).abs() < 1e-9);
        // ROI = 1.0 / 20.0 * 100 = 5.0%
        assert!((stats[0].roi_pct - 5.0).abs() < 1e-9);
        // Medium bucket
        assert_eq!(stats[1].bucket, ConfidenceTier::Medium);
        assert_eq!(stats[1].total_trades, 2);
        assert_eq!(stats[1].wins, 1);
        assert_eq!(stats[1].losses, 1);
        assert!((stats[1].realized_pnl - 1.0).abs() < 1e-9);
        // Low + None buckets are zero
        assert_eq!(stats[2].bucket, ConfidenceTier::Low);
        assert_eq!(stats[2].total_trades, 0);
        assert_eq!(stats[3].bucket, ConfidenceTier::None);
        assert_eq!(stats[3].total_trades, 0);
    }

    #[test]
    fn confidence_tier_stats_missing_decision_json_routes_to_none() {
        // Lots with no `decision_json` (or unparseable JSON) bucket
        // under None so the closed-lot count still matches the rest
        // of the analytics.
        let mut no_json = closed_lot_price(50.0, 5.0, 1.0);
        no_json.decision_json = None;
        let mut bad_json = closed_lot_price(50.0, 5.0, 1.0);
        bad_json.decision_json = Some("{not valid json".to_string());
        let mut no_tier = closed_lot_price(50.0, 5.0, 1.0);
        no_tier.decision_json = Some(r#"{"some_other_field": 42}"#.to_string());
        let lots = vec![no_json, bad_json, no_tier];
        let stats = compute_confidence_tier_stats(&lots);
        assert_eq!(stats[3].bucket, ConfidenceTier::None);
        assert_eq!(stats[3].total_trades, 3);
        assert_eq!(stats[3].wins, 3);
        assert_eq!(stats[3].losses, 0);
        // High, Medium, Low buckets are zero.
        assert_eq!(stats[0].total_trades, 0);
        assert_eq!(stats[1].total_trades, 0);
        assert_eq!(stats[2].total_trades, 0);
    }

    #[test]
    fn confidence_tier_stats_unrecognized_value_routes_to_none() {
        // Defensive: a value that doesn't match any of the four
        // canonical tier strings routes to None (rather than
        // panicking or being silently dropped). This protects
        // against forward-compat: if a future schema adds a
        // "VeryHigh" tier, old code still produces a valid
        // (None-bucketed) result instead of dropping the lot.
        let lots = vec![confidence_lot(Some("VeryHigh"), 5.0, 1.0)];
        let stats = compute_confidence_tier_stats(&lots);
        assert_eq!(stats[3].bucket, ConfidenceTier::None);
        assert_eq!(stats[3].total_trades, 1);
        assert_eq!(stats[3].wins, 1);
    }

    #[test]
    fn confidence_tier_stats_open_lot_counted_in_open_trades_only() {
        // Open lots count toward total_trades + open_trades, but
        // contribute nothing to wins/losses/PnL/ROI. They still
        // bucket by their confidence tier.
        let mut open = open_lot_price(50.0, 5.0);
        open.decision_json = Some(r#"{"confidence_tier": "High"}"#.to_string());
        let stats = compute_confidence_tier_stats(&[open]);
        assert_eq!(stats[0].bucket, ConfidenceTier::High);
        assert_eq!(stats[0].total_trades, 1);
        assert_eq!(stats[0].open_trades, 1);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert_eq!(stats[0].realized_pnl, 0.0);
        assert_eq!(stats[0].total_staked, 0.0);
    }

    #[test]
    fn confidence_tier_stats_push_lot_contributes_stake_but_not_win_or_loss() {
        // pnl == 0 (push) → stake counts toward total_staked, but
        // neither wins nor losses count it.
        let lots = vec![confidence_lot(Some("Medium"), 10.0, 0.0)];
        let stats = compute_confidence_tier_stats(&lots);
        assert_eq!(stats[1].bucket, ConfidenceTier::Medium);
        assert_eq!(stats[1].total_trades, 1);
        assert_eq!(stats[1].wins, 0);
        assert_eq!(stats[1].losses, 0);
        assert_eq!(stats[1].realized_pnl, 0.0);
        assert!((stats[1].total_staked - 10.0).abs() < 1e-9);
        // ROI = 0 / 10 * 100 = 0
        assert_eq!(stats[1].roi_pct, 0.0);
    }

    #[test]
    fn confidence_tier_stats_bucket_label_matches_enum_variant() {
        // The `bucket_label` is what the UI displays. Verify each
        // canonical tier has the expected human-readable label.
        let stats = compute_confidence_tier_stats(&[]);
        assert_eq!(stats[0].bucket_label, "High");
        assert_eq!(stats[1].bucket_label, "Medium");
        assert_eq!(stats[2].bucket_label, "Low");
        assert_eq!(stats[3].bucket_label, "None");
    }

    #[test]
    fn confidence_tier_stats_canonical_output_order_preserved() {
        // The output order must be High → Medium → Low → None
        // regardless of the input order. (Insert lots in reverse
        // tier order and verify the output is still canonical.)
        let lots = vec![
            confidence_lot(Some("None"), 5.0, 1.0),
            confidence_lot(Some("Low"), 5.0, 1.0),
            confidence_lot(Some("Medium"), 5.0, 1.0),
            confidence_lot(Some("High"), 5.0, 1.0),
        ];
        let stats = compute_confidence_tier_stats(&lots);
        assert_eq!(stats[0].bucket, ConfidenceTier::High);
        assert_eq!(stats[1].bucket, ConfidenceTier::Medium);
        assert_eq!(stats[2].bucket, ConfidenceTier::Low);
        assert_eq!(stats[3].bucket, ConfidenceTier::None);
    }

    // ───────────────────────────────────────────────────────────
    // Per-source (AI vs Manual) breakdown tests
    // ───────────────────────────────────────────────────────────

    /// Build a closed paper lot tagged with the given `PaperTradeSource`.
    /// Mirrors `disagreement_lot` / `confidence_lot` so the bucketing
    /// logic stays testable without depending on the upstream
    /// `record_paper_decision` write path.
    fn source_lot(source: PaperTradeSource, stake: f64, pnl: f64) -> PaperLot {
        let mut lot = closed_lot_price(50.0, stake, pnl);
        lot.source = source;
        lot
    }

    /// Build an open paper lot tagged with the given `PaperTradeSource`.
    /// Open lots count toward `total_trades + open_trades` but are
    /// excluded from the per-source PnL/ROI aggregations (mirrors the
    /// other breakdown helpers).
    fn source_lot_open(source: PaperTradeSource, stake: f64) -> PaperLot {
        let mut lot = open_lot_price(50.0, stake);
        lot.source = source;
        lot
    }

    #[test]
    fn source_stats_empty_input_returns_two_zero_buckets() {
        // Even with no lots, both canonical sources must appear
        // (with zeros) so the UI table layout is stable for users
        // who haven't placed any paper trades yet.
        let stats = compute_source_stats(&[]);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[0].total_trades, 0);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert_eq!(stats[0].win_rate, 0.0);
        assert_eq!(stats[0].realized_pnl, 0.0);
        assert_eq!(stats[0].total_staked, 0.0);
        assert_eq!(stats[0].roi_pct, 0.0);
        assert_eq!(stats[1].source, PaperTradeSource::Manual);
        assert_eq!(stats[1].total_trades, 0);
    }

    #[test]
    fn source_stats_buckets_lots_by_source_field() {
        // 2 AI wins + 1 AI loss vs. 1 Manual win + 1 Manual loss.
        // Verify the win/loss/PnL aggregation per source and the
        // explicit ROI math.
        let lots = vec![
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, -1.0),
            source_lot(PaperTradeSource::Manual, 10.0, 2.0),
            source_lot(PaperTradeSource::Manual, 10.0, -1.0),
        ];
        let stats = compute_source_stats(&lots);
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[0].total_trades, 3);
        assert_eq!(stats[0].wins, 2);
        assert_eq!(stats[0].losses, 1);
        // win_rate = 2 / (2 + 1) * 100 = 66.666...
        assert!((stats[0].win_rate - 66.66666666666666).abs() < 0.001);
        assert_eq!(stats[0].realized_pnl, 1.0);
        assert_eq!(stats[0].total_staked, 30.0);
        // ROI = 1.0 / 30.0 * 100 = 3.333...%
        assert!((stats[0].roi_pct - 3.3333333333333335).abs() < 0.001);
        assert_eq!(stats[1].source, PaperTradeSource::Manual);
        assert_eq!(stats[1].total_trades, 2);
        assert_eq!(stats[1].wins, 1);
        assert_eq!(stats[1].losses, 1);
        assert_eq!(stats[1].win_rate, 50.0);
        assert_eq!(stats[1].realized_pnl, 1.0);
        assert_eq!(stats[1].total_staked, 20.0);
        // ROI = 1.0 / 20.0 * 100 = 5.0%
        assert!((stats[1].roi_pct - 5.0).abs() < 0.001);
    }

    #[test]
    fn source_stats_canonical_order_ai_then_manual_regardless_of_input() {
        // Insert lots in reverse canonical order (Manual first, then
        // AI) and verify the output is still AiDecision → Manual so
        // the UI renders a stable "AI vs human" comparison without
        // resorting.
        let lots = vec![
            source_lot(PaperTradeSource::Manual, 5.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 5.0, 1.0),
        ];
        let stats = compute_source_stats(&lots);
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[1].source, PaperTradeSource::Manual);
    }

    #[test]
    fn source_stats_only_ai_lots_emits_zero_manual_bucket() {
        // User has only ever placed AI-decision trades. The Manual
        // bucket must still appear (with zeros) so the table layout
        // is stable — the answer to "how am I doing on manual picks"
        // is currently "no data" rather than a missing row.
        let lots = vec![
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, -1.0),
        ];
        let stats = compute_source_stats(&lots);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[0].total_trades, 2);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
        assert_eq!(stats[1].source, PaperTradeSource::Manual);
        assert_eq!(stats[1].total_trades, 0);
        assert_eq!(stats[1].wins, 0);
        assert_eq!(stats[1].losses, 0);
        assert_eq!(stats[1].realized_pnl, 0.0);
    }

    #[test]
    fn source_stats_open_lot_counted_in_open_trades_only() {
        // Open lots count toward `total_trades` and `open_trades` but
        // contribute nothing to the per-source PnL/ROI aggregations
        // (mirrors the other breakdown helpers — open positions are
        // excluded from the per-source ROI math).
        let lots = vec![
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot_open(PaperTradeSource::AiDecision, 10.0),
        ];
        let stats = compute_source_stats(&lots);
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[0].total_trades, 2);
        assert_eq!(stats[0].open_trades, 1);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 0);
        // Only the closed lot contributes to PnL/staked.
        assert_eq!(stats[0].realized_pnl, 1.0);
        assert_eq!(stats[0].total_staked, 10.0);
        // ROI denominator is closed stake only.
        assert!((stats[0].roi_pct - 10.0).abs() < 0.001);
    }

    #[test]
    fn source_stats_push_lot_contributes_stake_but_not_win_or_loss() {
        // Pushes (realized_pnl == 0) are decided but neither wins
        // nor losses — they contribute to `total_staked` (which
        // feeds the ROI denominator) but not to `wins`/`losses`
        // (which feed win_rate). Mirrors the other breakdown helpers.
        let lots = vec![
            source_lot(PaperTradeSource::AiDecision, 10.0, 0.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
        ];
        let stats = compute_source_stats(&lots);
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[0].total_trades, 2);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 0);
        assert_eq!(stats[0].realized_pnl, 1.0);
        assert_eq!(stats[0].total_staked, 20.0);
        // ROI = 1.0 / 20.0 * 100 = 5.0%
        assert!((stats[0].roi_pct - 5.0).abs() < 0.001);
    }

    #[test]
    fn source_stats_source_label_matches_enum_variant() {
        // The `source_label` is what the UI displays. Verify each
        // canonical source has the expected human-readable label.
        let stats = compute_source_stats(&[]);
        assert_eq!(stats[0].source_label, "AI decision");
        assert_eq!(stats[1].source_label, "Manual");
    }

    #[test]
    fn source_stats_ai_beats_manual_demonstrates_headline_question() {
        // Demonstrates the headline question this breakdown answers:
        // "is the AI model actually profitable vs. my manual picks?".
        // AI: 3 wins + 1 loss, $4 PnL on $40 staked → 50% win rate, 10% ROI.
        // Manual: 1 win + 3 losses, $-2 PnL on $40 staked → 25% win rate, -5% ROI.
        let lots = vec![
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::AiDecision, 10.0, 1.0),
            source_lot(PaperTradeSource::Manual, 10.0, 1.0),
            source_lot(PaperTradeSource::Manual, 10.0, -1.0),
            source_lot(PaperTradeSource::Manual, 10.0, -1.0),
            source_lot(PaperTradeSource::Manual, 10.0, -1.0),
        ];
        let stats = compute_source_stats(&lots);
        // AI: 4 wins, 0 losses, $4 PnL on $40 staked.
        assert_eq!(stats[0].source, PaperTradeSource::AiDecision);
        assert_eq!(stats[0].wins, 4);
        assert_eq!(stats[0].losses, 0);
        assert_eq!(stats[0].win_rate, 100.0);
        assert_eq!(stats[0].realized_pnl, 4.0);
        assert!((stats[0].roi_pct - 10.0).abs() < 0.001);
        // Manual: 1 win, 3 losses, $-2 PnL on $40 staked.
        assert_eq!(stats[1].source, PaperTradeSource::Manual);
        assert_eq!(stats[1].wins, 1);
        assert_eq!(stats[1].losses, 3);
        // win_rate = 1 / (1 + 3) * 100 = 25.0%
        assert_eq!(stats[1].win_rate, 25.0);
        assert_eq!(stats[1].realized_pnl, -2.0);
        // ROI = -2 / 40 * 100 = -5.0%
        assert!((stats[1].roi_pct - -5.0).abs() < 0.001);
    }

    // ── compute_top_lots tests ───────────────────────────────────

    /// Helper for the top-lots tests. Mirrors `closed_lot_price` but lets
    /// the test author pin `id` + `title` + `category` + `side` +
    /// `closed_at` individually so the projection fields can be asserted
    /// (e.g. `assert_eq!(top.title, "Josh Allen Over 275.5 passing yards")`).
    fn top_lot(
        id: &str,
        title: &str,
        category: &str,
        side: &str,
        pnl: f64,
        closed_at: &str,
    ) -> PaperLot {
        PaperLot {
            id: id.to_string(),
            ticker: format!("T-{id}"),
            title: title.to_string(),
            category: category.to_string(),
            side: side.to_string(),
            entry_price_cents: 50.0,
            qty: 1.0,
            stake_dollars: 5.0,
            source: PaperTradeSource::Manual,
            decision_json: None,
            opened_at: "2026-01-01T00:00:00Z".to_string(),
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
            notes: None,
            tags: None,
        }
    }

    #[test]
    fn top_lots_empty_input_returns_two_empty_vecs() {
        // With no lots, both lists are empty Vecs (not padded to size 5
        // with zero placeholders) so the UI can show its empty-state copy.
        let (winners, losers) = compute_top_lots(&[]);
        assert!(winners.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn top_lots_excludes_open_lots() {
        // Open lots have no realized PnL; they must not appear in either
        // list. The single open lot here is the only input — the helper
        // should return two empty Vecs.
        let lots = vec![open_lot()];
        let (winners, losers) = compute_top_lots(&lots);
        assert!(winners.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn top_lots_excludes_push_lots() {
        // A push (realized_pnl == 0) is neither a winner nor a loser.
        // The single push lot here is the only input — both lists empty.
        let lots = vec![top_lot(
            "push1",
            "Test Push",
            "Points",
            "Over",
            0.0,
            "2026-01-02T00:00:00Z",
        )];
        let (winners, losers) = compute_top_lots(&lots);
        assert!(winners.is_empty());
        assert!(losers.is_empty());
    }

    #[test]
    fn top_winners_sorted_by_realized_pnl_desc() {
        // Five winners with non-decreasing PnL. Order in the input is
        // scrambled so we can verify the sort actually runs.
        let lots = vec![
            top_lot("w2", "B", "Points", "Over", 2.0, "2026-01-02T00:00:00Z"),
            top_lot("w5", "E", "Points", "Over", 5.0, "2026-01-05T00:00:00Z"),
            top_lot("w1", "A", "Points", "Over", 1.0, "2026-01-01T00:00:00Z"),
            top_lot("w4", "D", "Points", "Over", 4.0, "2026-01-04T00:00:00Z"),
            top_lot("w3", "C", "Points", "Over", 3.0, "2026-01-03T00:00:00Z"),
        ];
        let (winners, _) = compute_top_lots(&lots);
        assert_eq!(winners.len(), 5);
        // DESC order by PnL.
        assert_eq!(winners[0].lot_id, "w5");
        assert_eq!(winners[1].lot_id, "w4");
        assert_eq!(winners[2].lot_id, "w3");
        assert_eq!(winners[3].lot_id, "w2");
        assert_eq!(winners[4].lot_id, "w1");
    }

    #[test]
    fn top_losers_sorted_by_realized_pnl_asc() {
        // Five losers with non-increasing (more negative) PnL. Scrambled
        // input order.
        let lots = vec![
            top_lot("l-2", "B", "Points", "Under", -2.0, "2026-01-02T00:00:00Z"),
            top_lot("l-5", "E", "Points", "Under", -5.0, "2026-01-05T00:00:00Z"),
            top_lot("l-1", "A", "Points", "Under", -1.0, "2026-01-01T00:00:00Z"),
            top_lot("l-4", "D", "Points", "Under", -4.0, "2026-01-04T00:00:00Z"),
            top_lot("l-3", "C", "Points", "Under", -3.0, "2026-01-03T00:00:00Z"),
        ];
        let (_, losers) = compute_top_lots(&lots);
        assert_eq!(losers.len(), 5);
        // ASC order by PnL (most negative first).
        assert_eq!(losers[0].lot_id, "l-5");
        assert_eq!(losers[1].lot_id, "l-4");
        assert_eq!(losers[2].lot_id, "l-3");
        assert_eq!(losers[3].lot_id, "l-2");
        assert_eq!(losers[4].lot_id, "l-1");
    }

    #[test]
    fn top_lots_caps_each_list_at_five() {
        // 8 winners + 8 losers → each list capped at the canonical 5.
        let mut lots = Vec::new();
        for i in 1..=8 {
            lots.push(top_lot(
                &format!("w{i}"),
                "T",
                "Points",
                "Over",
                i as f64,
                &format!("2026-01-{i:02}T00:00:00Z"),
            ));
        }
        for i in 1..=8 {
            lots.push(top_lot(
                &format!("l-{i}"),
                "T",
                "Points",
                "Under",
                -(i as f64),
                &format!("2026-02-{i:02}T00:00:00Z"),
            ));
        }
        let (winners, losers) = compute_top_lots(&lots);
        assert_eq!(winners.len(), 5, "winners must be capped at 5");
        assert_eq!(losers.len(), 5, "losers must be capped at 5");
        // The 6th winner (pnl=3) should NOT be in the output — the cap
        // dropped it, and the top-5 are pnl=8,7,6,5,4.
        assert!(!winners.iter().any(|w| w.lot_id == "w3"));
        // The 6th loser (pnl=-3) should NOT be in the output.
        assert!(!losers.iter().any(|l| l.lot_id == "l-3"));
    }

    #[test]
    fn top_lots_ties_break_by_closed_at_asc() {
        // Three lots all with pnl == 2.0 (tie). The helper should fall
        // back to closed_at ASC (older first) as the first tiebreak.
        let lots = vec![
            top_lot("t-new", "New", "Points", "Over", 2.0, "2026-01-05T00:00:00Z"),
            top_lot("t-old", "Old", "Points", "Over", 2.0, "2026-01-01T00:00:00Z"),
            top_lot("t-mid", "Mid", "Points", "Over", 2.0, "2026-01-03T00:00:00Z"),
        ];
        let (winners, _) = compute_top_lots(&lots);
        assert_eq!(winners.len(), 3);
        // Older closed_at first.
        assert_eq!(winners[0].lot_id, "t-old");
        assert_eq!(winners[1].lot_id, "t-mid");
        assert_eq!(winners[2].lot_id, "t-new");
    }

    #[test]
    fn top_lots_projection_carries_all_display_fields() {
        // The whole point of `PaperTopLot` is that the UI can render a
        // one-line row without a follow-up `get_lot` call. Verify every
        // projection field round-trips from the source lot.
        let source = top_lot(
            "l1",
            "Josh Allen Over 275.5 passing yards",
            "Passing Yards",
            "Over",
            7.5,
            "2026-03-15T18:30:00Z",
        );
        let (winners, _) = compute_top_lots(&[source]);
        assert_eq!(winners.len(), 1);
        let w = &winners[0];
        assert_eq!(w.lot_id, "l1");
        assert_eq!(w.ticker, "T-l1");
        assert_eq!(w.title, "Josh Allen Over 275.5 passing yards");
        assert_eq!(w.category, "Passing Yards");
        assert_eq!(w.side, "Over");
        assert!((w.realized_pnl - 7.5).abs() < 1e-9);
        assert!((w.stake_dollars - 5.0).abs() < 1e-9);
        assert!((w.entry_price_cents - 50.0).abs() < 1e-9);
        assert_eq!(w.closed_price_cents, Some(100.0));
        assert_eq!(w.closed_at.as_deref(), Some("2026-03-15T18:30:00Z"));
        assert_eq!(w.settlement_result.as_deref(), Some("Win"));
    }

    #[test]
    fn top_lots_mixed_winners_losers_and_pushes_sorted_into_separate_lists() {
        // 3 wins, 2 losses, 1 push, 1 open. The push and open should be
        // excluded; the 3 wins should land in `top_winners` DESC; the
        // 2 losses should land in `top_losers` ASC. This is the
        // headline behavior — winners and losers are sorted
        // independently and never bleed into the other list.
        let lots = vec![
            top_lot("w-big", "Big Win", "Points", "Over", 10.0, "2026-01-10T00:00:00Z"),
            top_lot("w-mid", "Mid Win", "Rebounds", "Over", 5.0, "2026-01-09T00:00:00Z"),
            top_lot("w-sml", "Sm Win", "Assists", "Over", 1.0, "2026-01-08T00:00:00Z"),
            top_lot("l-mid", "Mid Loss", "Points", "Under", -3.0, "2026-01-07T00:00:00Z"),
            top_lot("l-big", "Big Loss", "Rebounds", "Under", -8.0, "2026-01-06T00:00:00Z"),
            top_lot("p1", "Push", "Points", "Over", 0.0, "2026-01-05T00:00:00Z"),
            open_lot(),
        ];
        let (winners, losers) = compute_top_lots(&lots);
        assert_eq!(winners.len(), 3);
        assert_eq!(losers.len(), 2);
        // Winners: big → mid → sml
        assert_eq!(winners[0].lot_id, "w-big");
        assert_eq!(winners[1].lot_id, "w-mid");
        assert_eq!(winners[2].lot_id, "w-sml");
        // Losers: big (most negative) → mid
        assert_eq!(losers[0].lot_id, "l-big");
        assert_eq!(losers[1].lot_id, "l-mid");
        // No contamination: winners should not contain any losers,
        // losers should not contain any winners, neither should contain
        // the push or the open lot.
        for w in &winners {
            assert!(!w.lot_id.starts_with('l'));
            assert!(w.lot_id != "p1");
        }
        for l in &losers {
            assert!(!l.lot_id.starts_with('w'));
            assert!(l.lot_id != "p1");
        }
    }
}
