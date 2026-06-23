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
