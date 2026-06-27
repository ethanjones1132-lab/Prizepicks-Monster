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
