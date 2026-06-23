use crate::error::AppError;
use crate::config::AppConfig;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_notifications(
    limit: Option<i64>,
    pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<crate::notification::AppNotification>, String> {
    crate::notification::get_notifications(&pool, limit).await
}

#[tauri::command]
pub async fn get_unread_notification_count(pool: State<'_, Pool<Sqlite>>) -> Result<i64, String> {
    crate::notification::get_unread_count(&pool).await
}

#[tauri::command]
pub async fn mark_notification_read(
    id: String,
    pool: State<'_, Pool<Sqlite>>,
) -> Result<(), String> {
    crate::notification::mark_read(&pool, &id).await
}

#[tauri::command]
pub async fn mark_all_notifications_read(pool: State<'_, Pool<Sqlite>>) -> Result<(), String> {
    crate::notification::mark_all_read(&pool).await
}

#[tauri::command]
pub async fn dismiss_notification_cmd(
    id: String,
    pool: State<'_, Pool<Sqlite>>,
) -> Result<(), String> {
    crate::notification::dismiss_notification(&pool, &id).await
}

#[tauri::command]
pub async fn get_notification_settings(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<crate::notification::NotificationSettings, String> {
    Ok(state.lock().await.notification_settings.clone())
}

#[tauri::command]
pub async fn save_notification_settings(
    settings: crate::notification::NotificationSettings,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    let settings = settings.normalized();
    let mut config = state.lock().await;
    let previous_settings = config.notification_settings.clone();
    config.notification_settings = settings.clone();
    if let Err(e) = crate::config::save_config(&config) {
        config.notification_settings = previous_settings;
        return Err(AppError::Config(e.to_string()).into());
    }
    tracing::info!("Notification settings updated: {:?}", settings);
    Ok(())
}
