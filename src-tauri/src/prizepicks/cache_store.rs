//! SQLite persistence for the PrizePicks market summary cache.
//!
//! Stores the in-memory `PrizePicksCache` (markets, fetched_at, full_catalog)
//! as a JSON blob so the dashboard can render instantly on next launch
//! without waiting for the HTTP warm to complete.
//!
//! # Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS prizepicks_cache (
//!     id INTEGER PRIMARY KEY CHECK (id = 1),   -- singleton row
//!     markets_json TEXT  NOT NULL,              -- JSON-serialized Vec<PrizePicksMarketSummary>
//!     fetched_at   INTEGER NOT NULL,            -- unix seconds of the fetch
//!     full_catalog INTEGER NOT NULL DEFAULT 0,  -- 0 = partial (quick load), 1 = full
//!     updated_at   TEXT    NOT NULL DEFAULT (datetime('now'))
//! );
//! ```

use serde_json;
use sqlx::{Pool, Row, Sqlite};

use super::models::PrizePicksCache;

/// Create the cache persistence table if it doesn't exist.
pub async fn init_cache_table(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS prizepicks_cache (
            id          INTEGER PRIMARY KEY CHECK (id = 1),
            markets_json TEXT    NOT NULL,
            fetched_at   INTEGER NOT NULL,
            full_catalog INTEGER NOT NULL DEFAULT 0,
            updated_at   TEXT    NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("failed to init cache table: {e}"))?;
    Ok(())
}

/// Persist the current in-memory cache to SQLite.
///
/// Uses INSERT OR REPLACE so the singleton row (id=1) is always overwritten.
/// The write is intentionally fire-and-forget from the caller's perspective —
/// the HTTP fetch path should not block on a DB write.
pub async fn save_cache(pool: &Pool<Sqlite>, cache: &PrizePicksCache) -> Result<(), String> {
    let json = serde_json::to_string(&cache.markets)
        .map_err(|e| format!("failed to serialize cache markets: {e}"))?;
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO prizepicks_cache (id, markets_json, fetched_at, full_catalog, updated_at)
        VALUES (1, ?1, ?2, ?3, datetime('now'))
        "#,
    )
    .bind(&json)
    .bind(cache.fetched_at as i64)
    .bind(if cache.full_catalog { 1 } else { 0 })
    .execute(pool)
    .await
    .map_err(|e| format!("failed to save cache: {e}"))?;
    Ok(())
}

/// Load the persisted cache from SQLite, if one exists.
///
/// Returns `None` when:
/// - The table has no row (first launch or cache was cleared)
/// - The JSON cannot be deserialized (schema changed between versions)
///   The caller should log the warning and fall through to a fresh HTTP fetch.
pub async fn load_cache(pool: &Pool<Sqlite>) -> Option<PrizePicksCache> {
    let row = sqlx::query(
        "SELECT markets_json, fetched_at, full_catalog FROM prizepicks_cache WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::warn!("cache_store::load_cache query failed: {e}");
        e
    })
    .ok()??;

    let json: String = row.get("markets_json");
    let fetched_at: i64 = row.get("fetched_at");
    let full_catalog: i64 = row.get("full_catalog");

    let markets: Vec<super::models::PrizePicksMarketSummary> =
        serde_json::from_str(&json).map_err(|e| {
            tracing::warn!("cache_store::load_cache deserialization failed (schema change?): {e}");
            e
        }).ok()?;

    Some(PrizePicksCache {
        markets,
        fetched_at: fetched_at as u64,
        full_catalog: full_catalog != 0,
    })
}
