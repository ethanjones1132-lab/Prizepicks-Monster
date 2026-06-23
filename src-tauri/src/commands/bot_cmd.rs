use crate::config::AppConfig;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_bot_config(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<serde_json::Value, String> {
    let config = state.lock().await;
    Ok(serde_json::json!({
        "discord_webhook_url": config.discord_webhook_url,
        "telegram_bot_token": config.telegram_bot_token,
        "telegram_chat_id": config.telegram_chat_id,
        "bot_daily_picks_enabled": config.bot_daily_picks_enabled,
        "bot_game_alerts_enabled": config.bot_game_alerts_enabled,
        "bot_grading_results_enabled": config.bot_grading_results_enabled,
        "bot_daily_picks_time": config.bot_daily_picks_time,
    }))
}

#[tauri::command]
pub async fn save_bot_config(
    bot_settings: serde_json::Value,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    let mut config = state.lock().await;

    if let Some(url) = bot_settings.get("discord_webhook_url").and_then(|v| v.as_str()) {
        config.discord_webhook_url = url.to_string();
    }
    if let Some(token) = bot_settings.get("telegram_bot_token").and_then(|v| v.as_str()) {
        config.telegram_bot_token = token.to_string();
    }
    if let Some(chat_id) = bot_settings.get("telegram_chat_id").and_then(|v| v.as_str()) {
        config.telegram_chat_id = chat_id.to_string();
    }
    if let Some(enabled) = bot_settings.get("bot_daily_picks_enabled").and_then(|v| v.as_bool()) {
        config.bot_daily_picks_enabled = enabled;
    }
    if let Some(enabled) = bot_settings.get("bot_game_alerts_enabled").and_then(|v| v.as_bool()) {
        config.bot_game_alerts_enabled = enabled;
    }
    if let Some(enabled) = bot_settings.get("bot_grading_results_enabled").and_then(|v| v.as_bool()) {
        config.bot_grading_results_enabled = enabled;
    }
    if let Some(time) = bot_settings.get("bot_daily_picks_time").and_then(|v| v.as_str()) {
        config.bot_daily_picks_time = time.to_string();
    }

    crate::config::save_config(&config).map_err(|e| crate::error::AppError::Config(e.to_string()))?;
    tracing::info!("Bot configuration saved");
    Ok(())
}

#[tauri::command]
pub async fn test_discord_webhook_cmd(url: String) -> Result<String, String> {
    crate::bot::test_discord_webhook(&url).await
}

#[tauri::command]
pub async fn test_telegram_bot_cmd(bot_token: String, chat_id: String) -> Result<String, String> {
    crate::bot::test_telegram_bot(&bot_token, &chat_id).await
}

#[tauri::command]
pub async fn send_bot_test_message(
    title: String,
    body: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<String, String> {
    let config = state.lock().await.clone();

    let bot_config = crate::bot::BotDeliveryConfig {
        discord_webhook_url: config.discord_webhook_url,
        telegram_bot_token: config.telegram_bot_token,
        telegram_chat_id: config.telegram_chat_id,
        preferences: crate::bot::BotAlertPreferences::default(),
    };

    crate::bot::send_bot_notification(&bot_config, &title, &body, "info").await?;
    Ok("Test message sent successfully".to_string())
}
