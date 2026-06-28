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
}
