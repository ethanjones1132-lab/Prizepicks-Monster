use crate::prizepicks::PrizePicksFetcher;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn snapshot_line_movements(
    fetcher: State<'_, Arc<Mutex<PrizePicksFetcher>>>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<serde_json::Value, String> {
    let props = {
        let mut f = fetcher.lock().await;
        f.fetch_props(None, true).await?
    };

    let result =
        crate::line_tracker::snapshot_props(&db_pool, &props.props, &props.source.to_string())
            .await?;

    Ok(serde_json::json!({
        "snapshots_taken": result.snapshots_taken,
        "new_props": result.new_props,
        "updated_props": result.updated_props,
        "snapshot_at": result.snapshot_at,
    }))
}

#[tauri::command]
pub async fn get_line_movements(
    filter: crate::line_tracker::LineMovementFilter,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::line_tracker::LineMovementPage, String> {
    crate::line_tracker::get_line_summaries(&db_pool, &filter).await
}

#[tauri::command]
pub async fn get_line_detail(
    prop_key: String,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Option<crate::line_tracker::LineDetailHistory>, String> {
    crate::line_tracker::get_line_detail(&db_pool, &prop_key).await
}

#[tauri::command]
pub async fn get_tracked_line_leagues(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<String>, String> {
    crate::line_tracker::get_tracked_leagues(&db_pool).await
}

#[tauri::command]
pub async fn get_tracked_line_stat_categories(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<String>, String> {
    crate::line_tracker::get_tracked_stat_categories(&db_pool).await
}

#[tauri::command]
pub async fn get_latest_line_snapshot(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Option<String>, String> {
    crate::line_tracker::get_latest_snapshot_time(&db_pool).await
}

#[tauri::command]
pub async fn prune_line_movements(
    retention_days: i64,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<u64, String> {
    crate::line_tracker::prune_old_snapshots(&db_pool, retention_days).await
}
