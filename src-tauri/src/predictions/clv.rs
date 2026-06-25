//! Closing-line value (CLV) capture background task.
//!
//! Walks resolved PrizePicks predictions that don't yet have a captured closing
//! price, looks up the most recent `prizepicks_price_snapshots` row for the
//! same ticker (where the snapshot timestamp is on or before the prediction's
//! `resolved_at`), and persists the closing price and CLV points.
//!
//! Spawned once on app startup. Runs every `poll_interval_secs` (default 5 min)
//! so newly-resolved predictions are CLV-tagged without user intervention.

use sqlx::{Pool, Sqlite};
use std::time::Duration;

use super::storage;

/// Spawn the CLV-capture background task.
///
/// Safe to call multiple times — the interval is long enough that a few
/// concurrent sweeps are harmless. The DB query is bounded by `LIMIT 500` in
/// `capture_closing_prices_for_resolved`.
pub fn spawn_clv_capture_task(db_pool: Pool<Sqlite>, poll_interval_secs: u64) {
    let interval = poll_interval_secs.max(60);
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        ticker.tick().await; // skip the immediate first tick
        loop {
            ticker.tick().await;
            match storage::capture_closing_prices_for_resolved(&db_pool).await {
                Ok(n) if n > 0 => {
                    tracing::info!("[PrizePicks] CLV capture: {} predictions updated", n);
                }
                Ok(_) => {
                    // nothing to capture — quiet
                }
                Err(e) => {
                    tracing::warn!("[PrizePicks] CLV capture failed: {}", e);
                }
            }
        }
    });
}
