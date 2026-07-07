use super::PrizePicksState;
use sqlx::{Pool, Sqlite};
use tauri::State;

#[tauri::command]
pub async fn paper_get_analytics(
    db_pool: State<'_, Pool<Sqlite>>,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<crate::paper::PaperAnalytics, String> {
    let client = prizepicks.lock().await;
    crate::paper::get_analytics(&db_pool, Some(&*client)).await
}

#[tauri::command]
pub async fn paper_get_positions(
    db_pool: State<'_, Pool<Sqlite>>,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<Vec<crate::paper::PaperPosition>, String> {
    let client = prizepicks.lock().await;
    crate::paper::aggregate_positions(&db_pool, Some(&*client)).await
}

#[tauri::command]
pub async fn paper_settle_pending(
    db_pool: State<'_, Pool<Sqlite>>,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<crate::paper::PaperSettlementSummary, String> {
    let client = prizepicks.lock().await;
    crate::paper::settle_pending(&db_pool, &*client).await
}

#[tauri::command]
pub async fn paper_reset_account(
    db_pool: State<'_, Pool<Sqlite>>,
    starting_balance: Option<f64>,
) -> Result<crate::paper::PaperAccount, String> {
    crate::paper::reset_account(&db_pool, starting_balance).await
}

/// Fetch historical equity snapshots for the paper-trading account.
/// `limit` is the maximum number of snapshots to return (most recent first).
/// Defaults to 200 (~enough for a full season of daily snapshots).
#[tauri::command]
pub async fn paper_get_equity_history(
    db_pool: State<'_, Pool<Sqlite>>,
    limit: Option<i64>,
) -> Result<Vec<crate::paper::PaperEquitySnapshot>, String> {
    crate::paper::get_equity_snapshots(&db_pool, limit.unwrap_or(200)).await
}

/// Update notes and/or tags on a paper lot.
#[tauri::command]
pub async fn paper_update_lot_notes(
    db_pool: State<'_, Pool<Sqlite>>,
    lot_id: String,
    notes: Option<String>,
    tags: Option<String>,
) -> Result<crate::paper::PaperLot, String> {
    crate::paper::update_lot_notes(&db_pool, &lot_id, notes, tags).await
}

/// Fetch paper lots (trade fills), most recent first.
/// `status_filter` is optional — when Some, restricts the result to lots whose
/// `status` column matches (e.g. Some("Open") to show only open positions).
/// `limit` is optional — when None, returns every lot; the SQL `ORDER BY
/// opened_at DESC` keeps the newest fills on top either way.
#[tauri::command]
pub async fn paper_get_lots(
    db_pool: State<'_, Pool<Sqlite>>,
    status_filter: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<crate::paper::PaperLot>, String> {
    crate::paper::list_lots(&db_pool, status_filter.as_deref(), limit).await
}

/// Export all paper lots as CSV.
/// Returns a UTF-8 CSV string with all columns from `paper_lots`
/// including notes and tags, ordered by `opened_at DESC` (most recent first).
#[tauri::command]
pub async fn paper_export_lots_csv(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<String, String> {
    crate::paper::export_paper_lots_csv(&db_pool).await
}
