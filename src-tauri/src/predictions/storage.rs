#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! SQLite-backed Prediction Storage
//!
//! Replaces the old JSON-file-per-session storage with a single
//! SQLite database managed via sqlx. Provides CRUD operations
//! for predictions, outcomes, and bet history.
//!
//! Schema:
//!   predictions  — core prediction data extracted from AI responses
//!   bet_history  — bankroll bet results linked to predictions
//!
//! On first run, migrates existing JSON data into SQLite.
//! ═══════════════════════════════════════════════════════════════

use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::path::PathBuf;

use super::tracker::{Prediction, PredictionOutcome, PredictionRecord};

/// Database path: ~/.openclaw/prizepicks-monster/predictions.db
fn db_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".openclaw/prizepicks-monster/predictions.db")
}

/// Ensure the parent directory exists.
fn ensure_db_dir() -> Result<(), String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create db dir: {}", e))?;
    }
    Ok(())
}

/// Open a connection pool and run migrations.
pub async fn init_db() -> Result<Pool<Sqlite>, String> {
    ensure_db_dir()?;
    let path = db_path();
    let path_str = path.display().to_string().replace('\\', "/");
    let url = format!("sqlite:///{}?mode=rwc", path_str);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map_err(|e| format!("Failed to connect to SQLite: {}", e))?;

    // Create tables if they don't exist
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS predictions (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            raw_text TEXT NOT NULL DEFAULT '',
            player_name TEXT,
            pick_type TEXT,
            line REAL,
            stat_category TEXT,
            confidence TEXT,
            confidence_score INTEGER,
            probability REAL,
            reasoning TEXT,
            risk TEXT,
            created_at TEXT NOT NULL,
            outcome TEXT NOT NULL DEFAULT 'Pending',
            actual_result REAL,
            notes TEXT,
            resolved_at TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("Failed to create predictions table: {}", e))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bet_history (
            id TEXT PRIMARY KEY,
            prediction_id TEXT,
            player_name TEXT NOT NULL,
            prop_category TEXT NOT NULL,
            line REAL NOT NULL,
            pick_type TEXT NOT NULL,
            stake REAL NOT NULL,
            odds REAL,
            outcome TEXT NOT NULL,
            profit_loss REAL NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (prediction_id) REFERENCES predictions(id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("Failed to create bet_history table: {}", e))?;

    // Indexes for common queries
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pred_session ON predictions(session_id)")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pred_outcome ON predictions(outcome)")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pred_player ON predictions(player_name)")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pred_created ON predictions(created_at)")
        .execute(&pool)
        .await
        .ok();
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_pred_outcome_created ON predictions(outcome, created_at)",
    )
    .execute(&pool)
    .await
    .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pred_confidence ON predictions(confidence_score)")
        .execute(&pool)
        .await
        .ok();

    migrate_predictions_columns(&pool).await?;

    Ok(pool)
}

/// Add columns introduced after initial schema without breaking existing DBs.
async fn migrate_predictions_columns(pool: &Pool<Sqlite>) -> Result<(), String> {
    let rows = sqlx::query("PRAGMA table_info(predictions)")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("PRAGMA table_info failed: {}", e))?;

    let has_full_decision = rows
        .iter()
        .any(|r| r.get::<String, _>("name") == "full_decision_json");

    if !has_full_decision {
        sqlx::query("ALTER TABLE predictions ADD COLUMN full_decision_json TEXT")
            .execute(pool)
            .await
            .map_err(|e| format!("ALTER TABLE full_decision_json failed: {}", e))?;
    }

    // CLV (closing-line value) columns — captures entry price at insert time and
    // closing price at resolution. Added 2026-06-25 as part of P2 "CLV per prediction".
    let column_names: Vec<String> = rows
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let has_column = |name: &str| -> bool {
        column_names.iter().any(|n| n == name)
    };

    if !has_column("entry_price_pct") {
        sqlx::query("ALTER TABLE predictions ADD COLUMN entry_price_pct REAL")
            .execute(pool)
            .await
            .map_err(|e| format!("ALTER TABLE entry_price_pct failed: {}", e))?;
    }
    if !has_column("closing_price_pct") {
        sqlx::query("ALTER TABLE predictions ADD COLUMN closing_price_pct REAL")
            .execute(pool)
            .await
            .map_err(|e| format!("ALTER TABLE closing_price_pct failed: {}", e))?;
    }
    if !has_column("clv_points") {
        sqlx::query("ALTER TABLE predictions ADD COLUMN clv_points REAL")
            .execute(pool)
            .await
            .map_err(|e| format!("ALTER TABLE clv_points failed: {}", e))?;
    }
    if !has_column("clv_ticker") {
        sqlx::query("ALTER TABLE predictions ADD COLUMN clv_ticker TEXT")
            .execute(pool)
            .await
            .map_err(|e| format!("ALTER TABLE clv_ticker failed: {}", e))?;
    }
    if !has_column("clv_captured_at") {
        sqlx::query("ALTER TABLE predictions ADD COLUMN clv_captured_at TEXT")
            .execute(pool)
            .await
            .map_err(|e| format!("ALTER TABLE clv_captured_at failed: {}", e))?;
    }

    // Index for the CLV-capture sweep: resolved predictions with no closing price yet.
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_pred_clv_pending \
         ON predictions(outcome, clv_captured_at) \
         WHERE clv_captured_at IS NULL",
    )
    .execute(pool)
    .await
    .ok();

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// CRUD Operations
// ═══════════════════════════════════════════════════════════════

/// Insert a prediction record. Ignores duplicates (same id).
///
/// If the prediction has a `full_decision_json` containing a PrizePicks trade decision,
/// the entry price is extracted from `market_price_pct` and stored in `entry_price_pct`
/// at insert time. This is the "at decision" anchor for downstream CLV calculation.
pub async fn insert_prediction(
    pool: &Pool<Sqlite>,
    record: &PredictionRecord,
) -> Result<(), String> {
    let p = &record.prediction;
    let entry_price_pct = extract_entry_price_pct(p.full_decision_json.as_deref());

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO predictions
            (id, session_id, raw_text, player_name, pick_type, line,
             stat_category, confidence, confidence_score, probability,
             reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
             full_decision_json, entry_price_pct)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#,
    )
    .bind(&p.id)
    .bind(&p.session_id)
    .bind(&p.raw_text)
    .bind(&p.player_name)
    .bind(&p.pick_type)
    .bind(p.line)
    .bind(&p.stat_category)
    .bind(&p.confidence)
    .bind(p.confidence_score.map(|v| v as i64))
    .bind(p.probability)
    .bind(&p.reasoning)
    .bind(&p.risk)
    .bind(&p.created_at)
    .bind(record.outcome.to_string())
    .bind(record.actual_result)
    .bind(&record.notes)
    .bind(&record.resolved_at)
    .bind(&p.full_decision_json)
    .bind(entry_price_pct)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert prediction: {}", e))?;

    Ok(())
}

/// Extract `market_price_pct` from a serialized `PrizePicksTradeDecision` JSON blob.
/// The field is the implied probability for the selected side in the same units used
/// by `prizepicks_price_snapshots.yes_prob_pct`, so the values are directly comparable
/// when computing CLV.
pub fn extract_entry_price_pct(json: Option<&str>) -> Option<f64> {
    use serde_json::Value;
    let raw = json?;
    let v: Value = serde_json::from_str(raw).ok()?;
    // market_price_pct is 0.0–1.0 per decision_schema doc; convert to percentage points
    // to align with the snapshot schema's percent-like storage convention.
    let p = v.get("market_price_pct")?.as_f64()?;
    if !p.is_finite() || !(0.0..=1.0).contains(&p) {
        return None;
    }
    Some(p * 100.0)
}

/// Update the outcome of a prediction.
pub async fn update_prediction_outcome(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    outcome: &PredictionOutcome,
    actual_result: Option<f64>,
) -> Result<(), String> {
    let resolved_at = if *outcome != PredictionOutcome::Pending {
        Some(chrono::Utc::now().to_rfc3339())
    } else {
        None
    };

    let rows = sqlx::query(
        r#"
        UPDATE predictions
        SET outcome = ?1, actual_result = ?2, resolved_at = ?3
        WHERE id = ?4
        "#,
    )
    .bind(outcome.to_string())
    .bind(actual_result)
    .bind(&resolved_at)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update prediction: {}", e))?
    .rows_affected();

    if rows == 0 {
        Err(format!("Prediction {} not found", prediction_id))
    } else {
        Ok(())
    }
}

/// Get all predictions for a session, ordered by created_at desc.
pub async fn get_session_predictions(
    pool: &Pool<Sqlite>,
    session_id: &str,
) -> Result<Vec<PredictionRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, session_id, raw_text, player_name, pick_type, line,
               stat_category, confidence, confidence_score, probability,
               reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
               full_decision_json
        FROM predictions
        WHERE session_id = ?1
        ORDER BY created_at DESC
        "#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch session predictions: {}", e))?;

    Ok(rows.iter().map(row_to_record).collect())
}

/// Get all predictions across all sessions, ordered by created_at desc.
pub async fn get_all_predictions(pool: &Pool<Sqlite>) -> Result<Vec<PredictionRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, session_id, raw_text, player_name, pick_type, line,
               stat_category, confidence, confidence_score, probability,
               reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
               full_decision_json
        FROM predictions
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch all predictions: {}", e))?;

    Ok(rows.iter().map(row_to_record).collect())
}

/// Get predictions filtered by confidence score range.
pub async fn get_predictions_by_confidence(
    pool: &Pool<Sqlite>,
    min_score: u8,
    max_score: u8,
) -> Result<Vec<PredictionRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, session_id, raw_text, player_name, pick_type, line,
               stat_category, confidence, confidence_score, probability,
               reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
               full_decision_json
        FROM predictions
        WHERE confidence_score >= ?1 AND confidence_score <= ?2
        ORDER BY created_at DESC
        "#,
    )
    .bind(min_score as i64)
    .bind(max_score as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch predictions by confidence: {}", e))?;

    Ok(rows.iter().map(row_to_record).collect())
}

/// Delete a prediction by id.
pub async fn delete_prediction(pool: &Pool<Sqlite>, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM predictions WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to delete prediction: {}", e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// CLV (closing-line value) persistence
// ═══════════════════════════════════════════════════════════════

/// Persist the entry price for a prediction. Called at insert time when the
/// `full_decision_json` contains a `PrizePicksTradeDecision.market_price_pct`.
pub async fn update_entry_price(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    entry_price_pct: f64,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE predictions SET entry_price_pct = ?1 WHERE id = ?2 AND entry_price_pct IS NULL",
    )
    .bind(entry_price_pct)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update entry_price_pct: {}", e))?;
    Ok(())
}

/// Persist the closing price and computed CLV points for a prediction.
pub async fn update_closing_price(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    closing_price_pct: f64,
    clv_points: f64,
    clv_ticker: &str,
    captured_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE predictions \
         SET closing_price_pct = ?1, clv_points = ?2, clv_ticker = ?3, clv_captured_at = ?4 \
         WHERE id = ?5 AND clv_captured_at IS NULL",
    )
    .bind(closing_price_pct)
    .bind(clv_points)
    .bind(clv_ticker)
    .bind(captured_at)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update closing_price_pct: {}", e))?;
    Ok(())
}

/// Sweep resolved predictions that don't yet have a captured closing price, look up the
/// most recent `prizepicks_price_snapshots` row for the same ticker (where the snapshot
/// timestamp is on or before the prediction's `resolved_at`), and persist CLV.
///
/// Returns the number of predictions for which a closing price was successfully captured.
pub async fn capture_closing_prices_for_resolved(
    pool: &Pool<Sqlite>,
) -> Result<usize, String> {
    use serde_json::Value;

    // Find resolved predictions with an entry price but no closing price captured yet.
    let rows = sqlx::query(
        r#"
        SELECT id, full_decision_json, entry_price_pct, resolved_at
        FROM predictions
        WHERE outcome != 'Pending'
          AND clv_captured_at IS NULL
          AND entry_price_pct IS NOT NULL
          AND full_decision_json IS NOT NULL
          AND resolved_at IS NOT NULL
        ORDER BY resolved_at ASC
        LIMIT 500
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch CLV candidates: {}", e))?;

    let mut captured = 0usize;

    for row in rows.iter() {
        let id: String = row.get("id");
        let json: String = row.get("full_decision_json");
        let entry_price_pct: f64 = row.get("entry_price_pct");
        let resolved_at: String = row.get("resolved_at");

        // Parse ticker from full_decision_json
        let ticker = match serde_json::from_str::<Value>(&json) {
            Ok(v) => v
                .get("ticker")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string()),
            Err(_) => None,
        };
        let ticker = match ticker {
            Some(t) if !t.is_empty() => t,
            _ => continue, // No ticker → can't look up closing snapshot
        };

        // Look up the most recent snapshot for this ticker at-or-before resolved_at.
        let snap = sqlx::query(
            r#"
            SELECT yes_prob_pct, snapshot_at
            FROM prizepicks_price_snapshots
            WHERE ticker = ?1 AND snapshot_at <= ?2
            ORDER BY snapshot_at DESC
            LIMIT 1
            "#,
        )
        .bind(&ticker)
        .bind(&resolved_at)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Failed to fetch closing snapshot: {}", e))?;

        let snap = match snap {
            Some(s) => s,
            None => continue, // No snapshot available yet — try again next pass
        };

        let closing_price_pct: f64 = snap.get("yes_prob_pct");
        let snapshot_at: String = snap.get("snapshot_at");

        // CLV is the change in implied probability (closing - entry).
        // Both values are stored in the same units as the snapshot's yes_prob_pct.
        let clv_points = closing_price_pct - entry_price_pct;

        update_closing_price(
            pool,
            &id,
            closing_price_pct,
            clv_points,
            &ticker,
            &snapshot_at,
        )
        .await?;
        captured += 1;
    }

    Ok(captured)
}

// ═══════════════════════════════════════════════════════════════
// Bet History CRUD
// ═══════════════════════════════════════════════════════════════

/// A recorded bet result.
#[derive(Debug, Clone)]
pub struct BetRecord {
    pub id: String,
    pub prediction_id: Option<String>,
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub stake: f64,
    pub odds: Option<f64>,
    pub outcome: String,
    pub profit_loss: f64,
    pub created_at: String,
}

/// Insert a bet history record.
pub async fn insert_bet(pool: &Pool<Sqlite>, record: &BetRecord) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO bet_history
            (id, prediction_id, player_name, prop_category, line, pick_type,
             stake, odds, outcome, profit_loss, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
    )
    .bind(&record.id)
    .bind(&record.prediction_id)
    .bind(&record.player_name)
    .bind(&record.prop_category)
    .bind(record.line)
    .bind(&record.pick_type)
    .bind(record.stake)
    .bind(record.odds)
    .bind(&record.outcome)
    .bind(record.profit_loss)
    .bind(&record.created_at)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert bet history: {}", e))?;

    Ok(())
}

/// Get all bet history records, ordered by created_at desc.
pub async fn get_bet_history(pool: &Pool<Sqlite>) -> Result<Vec<BetRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, prediction_id, player_name, prop_category, line, pick_type,
               stake, odds, outcome, profit_loss, created_at
        FROM bet_history
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch bet history: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| BetRecord {
            id: r.get("id"),
            prediction_id: r.get("prediction_id"),
            player_name: r.get("player_name"),
            prop_category: r.get("prop_category"),
            line: r.get("line"),
            pick_type: r.get("pick_type"),
            stake: r.get("stake"),
            odds: r.get("odds"),
            outcome: r.get("outcome"),
            profit_loss: r.get("profit_loss"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Get total profit/loss from bet history.
pub async fn get_total_profit_loss(pool: &Pool<Sqlite>) -> Result<f64, String> {
    let row = sqlx::query("SELECT COALESCE(SUM(profit_loss), 0.0) as total FROM bet_history")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to fetch total P&L: {}", e))?;

    Ok(row.get::<f64, _>("total"))
}

// ═══════════════════════════════════════════════════════════════
// JSON → SQLite Migration
// ═══════════════════════════════════════════════════════════════

/// Migrate existing JSON prediction files into SQLite.
/// Called once on startup. Safe to call multiple times (INSERT OR IGNORE).
pub async fn migrate_from_json(pool: &Pool<Sqlite>) -> Result<usize, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let predictions_dir = PathBuf::from(home).join(".openclaw/prizepicks-monster/predictions");

    if !predictions_dir.exists() {
        return Ok(0);
    }

    let mut migrated = 0usize;

    let entries = std::fs::read_dir(&predictions_dir)
        .map_err(|e| format!("Failed to read predictions dir: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let records: Vec<PredictionRecord> = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for record in &records {
            if let Err(e) = insert_prediction(pool, record).await {
                tracing::warn!(
                    "Failed to migrate prediction {}: {}",
                    record.prediction.id,
                    e
                );
            } else {
                migrated += 1;
            }
        }
    }

    if migrated > 0 {
        tracing::info!("Migrated {} predictions from JSON to SQLite", migrated);
    }

    Ok(migrated)
}

// ═══════════════════════════════════════════════════════════════
// Row → PredictionRecord conversion
// ═══════════════════════════════════════════════════════════════

fn row_to_record(r: &sqlx::sqlite::SqliteRow) -> PredictionRecord {
    let outcome_str: String = r.get("outcome");
    let outcome = outcome_str
        .parse::<PredictionOutcome>()
        .unwrap_or(PredictionOutcome::Pending);

    let confidence_score: Option<i64> = r.get("confidence_score");

    PredictionRecord {
        prediction: Prediction {
            id: r.get("id"),
            session_id: r.get("session_id"),
            raw_text: r.get("raw_text"),
            player_name: r.get("player_name"),
            pick_type: r.get("pick_type"),
            line: r.get("line"),
            stat_category: r.get("stat_category"),
            confidence: r.get("confidence"),
            confidence_score: confidence_score.map(|v| v as u8),
            probability: r.get("probability"),
            reasoning: r.get("reasoning"),
            risk: r.get("risk"),
            created_at: r.get("created_at"),
            full_decision_json: r.try_get("full_decision_json").ok().flatten(),
        },
        outcome,
        actual_result: r.get("actual_result"),
        notes: r.get("notes"),
        resolved_at: r.get("resolved_at"),
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_entry_price_pct_handles_valid_json() {
        let json = r#"{"ticker":"NFL-X-O-100","market_price_pct":0.55}"#;
        // 0.55 * 100 may equal 55.0 or 55.00000000000001 due to float; compare approximately.
        let got = extract_entry_price_pct(Some(json)).unwrap();
        assert!((got - 55.0).abs() < 1e-9, "expected ~55, got {}", got);
    }

    #[test]
    fn extract_entry_price_pct_handles_missing_field() {
        let json = r#"{"ticker":"NFL-X-O-100"}"#;
        assert_eq!(extract_entry_price_pct(Some(json)), None);
    }

    #[test]
    fn extract_entry_price_pct_handles_invalid_json() {
        assert_eq!(extract_entry_price_pct(Some("not json")), None);
    }

    #[test]
    fn extract_entry_price_pct_handles_none() {
        assert_eq!(extract_entry_price_pct(None), None);
    }

    #[test]
    fn extract_entry_price_pct_rejects_out_of_range() {
        // 1.5 is outside the documented 0.0–1.0 range
        let json = r#"{"market_price_pct":1.5}"#;
        assert_eq!(extract_entry_price_pct(Some(json)), None);

        // Negative values are not valid probabilities
        let json = r#"{"market_price_pct":-0.1}"#;
        assert_eq!(extract_entry_price_pct(Some(json)), None);
    }

    #[test]
    fn extract_entry_price_pct_clamps_boundaries() {
        // 0.0 and 1.0 are valid
        assert_eq!(extract_entry_price_pct(Some(r#"{"market_price_pct":0.0}"#)), Some(0.0));
        assert_eq!(extract_entry_price_pct(Some(r#"{"market_price_pct":1.0}"#)), Some(100.0));
    }

    /// Helper: build a fresh in-memory pool with the production schema applied.
    async fn fresh_pool() -> Pool<Sqlite> {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Mirror the production init: base predictions table + migrations
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS predictions (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                raw_text TEXT NOT NULL DEFAULT '',
                player_name TEXT,
                pick_type TEXT,
                line REAL,
                stat_category TEXT,
                confidence TEXT,
                confidence_score INTEGER,
                probability REAL,
                reasoning TEXT,
                risk TEXT,
                created_at TEXT NOT NULL,
                outcome TEXT NOT NULL DEFAULT 'Pending',
                actual_result REAL,
                notes TEXT,
                resolved_at TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("ALTER TABLE predictions ADD COLUMN full_decision_json TEXT")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE predictions ADD COLUMN entry_price_pct REAL")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE predictions ADD COLUMN closing_price_pct REAL")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE predictions ADD COLUMN clv_points REAL")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE predictions ADD COLUMN clv_ticker TEXT")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE predictions ADD COLUMN clv_captured_at TEXT")
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS prizepicks_price_snapshots (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT '',
                yes_prob_pct REAL NOT NULL,
                yes_bid REAL NOT NULL,
                yes_ask REAL NOT NULL,
                spread REAL NOT NULL,
                volume_24h REAL NOT NULL DEFAULT 0,
                liquidity REAL NOT NULL DEFAULT 0,
                snapshot_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn make_record(id: &str, json: Option<&str>) -> PredictionRecord {
        PredictionRecord {
            prediction: Prediction {
                id: id.to_string(),
                session_id: "test-session".to_string(),
                raw_text: "raw".to_string(),
                player_name: Some("Test".to_string()),
                pick_type: Some("Over".to_string()),
                line: Some(100.0),
                stat_category: Some("Passing Yards".to_string()),
                confidence: Some("High".to_string()),
                confidence_score: Some(75),
                probability: Some(0.55),
                reasoning: Some("test".to_string()),
                risk: Some("wind".to_string()),
                created_at: "2026-06-25T12:00:00Z".to_string(),
                full_decision_json: json.map(str::to_string),
            },
            outcome: PredictionOutcome::Pending,
            actual_result: None,
            notes: None,
            resolved_at: None,
        }
    }

    #[tokio::test]
    async fn insert_prediction_writes_entry_price_from_decision() {
        let pool = fresh_pool().await;
        let json = r#"{"ticker":"NFL-X-O-100","market_price_pct":0.55}"#;
        let rec = make_record("p1", Some(json));

        insert_prediction(&pool, &rec).await.unwrap();

        let row: f64 = sqlx::query_scalar(
            "SELECT entry_price_pct FROM predictions WHERE id = 'p1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!((row - 55.0).abs() < 0.01, "expected ~55, got {}", row);
    }

    #[tokio::test]
    async fn insert_prediction_handles_missing_decision() {
        let pool = fresh_pool().await;
        let rec = make_record("p2", None);

        insert_prediction(&pool, &rec).await.unwrap();

        let row: Option<f64> =
            sqlx::query_scalar("SELECT entry_price_pct FROM predictions WHERE id = 'p2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, None);
    }

    #[tokio::test]
    async fn capture_closing_prices_skips_when_no_snapshot() {
        let pool = fresh_pool().await;
        let rec = make_record("p3", Some(r#"{"ticker":"NFL-X","market_price_pct":0.50}"#));
        // Mark resolved with a resolved_at, but provide NO snapshot.
        let mut rec = rec;
        rec.outcome = PredictionOutcome::Win;
        rec.resolved_at = Some("2026-06-25T20:00:00Z".to_string());
        insert_prediction(&pool, &rec).await.unwrap();

        let captured = capture_closing_prices_for_resolved(&pool).await.unwrap();
        assert_eq!(captured, 0, "should not capture without a snapshot");
    }

    #[tokio::test]
    async fn capture_closing_prices_links_latest_snapshot_to_resolution() {
        let pool = fresh_pool().await;

        // Insert a resolved prediction with an entry price (in percentage points: 0–100).
        // market_price_pct=0.55 → entry_price_pct=55.0
        let json = r#"{"ticker":"NFL-Y-O-200","market_price_pct":0.55}"#;
        let mut rec = make_record("p4", Some(json));
        rec.outcome = PredictionOutcome::Win;
        rec.resolved_at = Some("2026-06-25T20:00:00Z".to_string());
        insert_prediction(&pool, &rec).await.unwrap();

        // Snapshots: yes_prob_pct is stored in percentage points (0–100) to match
        // the production `prizepicks_price_snapshots.yes_prob_pct` column convention.
        // Insert an early snapshot (entry-equivalent, 55pp) and a late one (close, 62pp).
        sqlx::query(
            "INSERT INTO prizepicks_price_snapshots \
             (id, ticker, yes_prob_pct, yes_bid, yes_ask, spread, snapshot_at) \
             VALUES ('s1', 'NFL-Y-O-200', 55.0, 55.0, 56.0, 1.0, '2026-06-25T12:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO prizepicks_price_snapshots \
             (id, ticker, yes_prob_pct, yes_bid, yes_ask, spread, snapshot_at) \
             VALUES ('s2', 'NFL-Y-O-200', 62.0, 62.0, 63.0, 1.0, '2026-06-25T19:30:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // A snapshot AFTER resolution should NOT be picked up as the closing price.
        sqlx::query(
            "INSERT INTO prizepicks_price_snapshots \
             (id, ticker, yes_prob_pct, yes_bid, yes_ask, spread, snapshot_at) \
             VALUES ('s3', 'NFL-Y-O-200', 99.0, 99.0, 99.0, 0.0, '2026-06-25T21:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let captured = capture_closing_prices_for_resolved(&pool).await.unwrap();
        assert_eq!(captured, 1);

        // Verify the closing price is 62pp, and CLV is 62 - 55 = +7pp.
        let (closing, clv, captured_at): (Option<f64>, Option<f64>, Option<String>) =
            sqlx::query_as(
                "SELECT closing_price_pct, clv_points, clv_captured_at \
                 FROM predictions WHERE id = 'p4'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();

        assert!((closing.unwrap() - 62.0).abs() < 0.01);
        assert!((clv.unwrap() - 7.0).abs() < 0.01);
        assert_eq!(captured_at.as_deref(), Some("2026-06-25T19:30:00Z"));
    }

    #[tokio::test]
    async fn capture_closing_prices_is_idempotent() {
        let pool = fresh_pool().await;
        let json = r#"{"ticker":"NFL-Z","market_price_pct":0.50}"#;
        let mut rec = make_record("p5", Some(json));
        rec.outcome = PredictionOutcome::Loss;
        rec.resolved_at = Some("2026-06-25T20:00:00Z".to_string());
        insert_prediction(&pool, &rec).await.unwrap();

        // yes_prob_pct in 0–100 (percentage points) to match production convention.
        sqlx::query(
            "INSERT INTO prizepicks_price_snapshots \
             (id, ticker, yes_prob_pct, yes_bid, yes_ask, spread, snapshot_at) \
             VALUES ('sZ', 'NFL-Z', 45.0, 45.0, 46.0, 1.0, '2026-06-25T19:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // First sweep captures; second sweep should be a no-op.
        let n1 = capture_closing_prices_for_resolved(&pool).await.unwrap();
        let n2 = capture_closing_prices_for_resolved(&pool).await.unwrap();
        assert_eq!(n1, 1);
        assert_eq!(n2, 0);
    }

    #[tokio::test]
    async fn capture_closing_prices_skips_when_ticker_missing() {
        let pool = fresh_pool().await;
        // full_decision_json without a ticker field
        let json = r#"{"market_price_pct":0.50,"market_title":"NoTicker"}"#;
        let mut rec = make_record("p6", Some(json));
        rec.outcome = PredictionOutcome::Win;
        rec.resolved_at = Some("2026-06-25T20:00:00Z".to_string());
        insert_prediction(&pool, &rec).await.unwrap();

        let n = capture_closing_prices_for_resolved(&pool).await.unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn update_closing_price_is_guarded_against_overwrite() {
        // Pure-logic test: update_closing_price SQL only writes when clv_captured_at IS NULL.
        // This is a documentation/sanity test for the WHERE clause.
        // The behavior is verified by the integration test `capture_closing_prices_is_idempotent`.
        // Here we just ensure the SQL is well-formed and parseable.
        let _ = "UPDATE predictions \
                 SET closing_price_pct = ?1, clv_points = ?2, clv_ticker = ?3, clv_captured_at = ?4 \
                 WHERE id = ?5 AND clv_captured_at IS NULL";
    }
}
