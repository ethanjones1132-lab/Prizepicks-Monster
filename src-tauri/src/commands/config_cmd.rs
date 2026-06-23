use crate::config;
use crate::config::AppConfig;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<Mutex<AppConfig>>>) -> Result<AppConfig, String> {
    Ok(state.lock().await.clone())
}

#[tauri::command]
pub async fn save_config(
    config: AppConfig,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    config::save_config(&config).map_err(|e| crate::error::AppError::Config(e.to_string()))?;
    let mut guard = state.lock().await;
    *guard = config;
    Ok(())
}

#[tauri::command]
pub async fn check_api_status(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<config::ApiStatus, String> {
    let config = state.lock().await.clone();
    Ok(config::check_api_status(&config).await)
}

#[tauri::command]
pub async fn get_available_models() -> Result<Vec<config::ModelInfo>, String> {
    Ok(config::available_models())
}
