use crate::weather::WeatherClient;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_game_weather(
    game: String,
    location: String,
    weather: State<'_, Arc<Mutex<WeatherClient>>>,
) -> Result<crate::weather::GameWeather, String> {
    let mut w = weather.lock().await;
    w.get_weather(&game, &location).await
}
