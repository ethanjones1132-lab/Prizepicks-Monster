use crate::error::AppError;
use crate::bankroll;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use super::{PickInput, ParlayLeg};

#[tauri::command]
pub async fn get_bankroll_config() -> Result<bankroll::BankrollConfig, String> {
    Ok(bankroll::load_bankroll_config())
}

#[tauri::command]
pub async fn save_bankroll_config(config: bankroll::BankrollConfig) -> Result<(), String> {
    bankroll::save_bankroll_config(&config)
}

#[tauri::command]
pub async fn get_bankroll_summary(
    config: bankroll::BankrollConfig,
    db_pool: tauri::State<'_, sqlx::Pool<sqlx::Sqlite>>,
) -> Result<bankroll::BankrollSummary, String> {
    Ok(bankroll::get_bankroll_summary(&config, &db_pool).await)
}

#[tauri::command]
pub async fn recommend_bets(
    bankroll_config: bankroll::BankrollConfig,
    picks: Vec<PickInput>,
) -> Result<Vec<bankroll::BetRecommendation>, String> {
    let inputs: Vec<bankroll::PickInput> = picks
        .into_iter()
        .map(|p| bankroll::PickInput {
            player_name: p.player_name,
            prop_category: p.prop_category,
            line: p.line,
            pick_type: p.pick_type,
            win_probability: p.win_probability,
            confidence_score: p.confidence_score,
        })
        .collect();
    Ok(bankroll::recommend_multiple_bets(&bankroll_config, &inputs))
}

#[tauri::command]
pub async fn recommend_parlay(
    bankroll_config: bankroll::BankrollConfig,
    legs: Vec<PickInput>,
    correlation_factor: f64,
) -> Result<bankroll::ParlayRecommendation, String> {
    let inputs: Vec<bankroll::PickInput> = legs
        .into_iter()
        .map(|p| bankroll::PickInput {
            player_name: p.player_name,
            prop_category: p.prop_category,
            line: p.line,
            pick_type: p.pick_type,
            win_probability: p.win_probability,
            confidence_score: p.confidence_score,
        })
        .collect();
    Ok(bankroll::recommend_parlay(&bankroll_config, &inputs, correlation_factor))
}

#[tauri::command]
pub async fn record_bankroll_result(
    mut config: bankroll::BankrollConfig,
    stake: f64,
    won: bool,
    odds: Option<f64>,
) -> Result<bankroll::BankrollConfig, String> {
    bankroll::record_result(&mut config, stake, won, odds);
    bankroll::save_bankroll_config(&config).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(config)
}

#[tauri::command]
pub async fn get_parlay_legs(
    min_confidence: Option<u8>,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<Vec<ParlayLeg>, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;

    let legs: Vec<ParlayLeg> = all
        .iter()
        .filter(|r| r.outcome == crate::predictions::tracker::PredictionOutcome::Pending)
        .filter(|r| {
            if let Some(min) = min_confidence {
                r.prediction.confidence_score.map_or(false, |s| s >= min)
            } else {
                true
            }
        })
        .map(|r| ParlayLeg::from(r))
        .filter(|l| !l.player_name.is_empty() && !l.prop_category.is_empty())
        .collect();

    Ok(legs)
}
