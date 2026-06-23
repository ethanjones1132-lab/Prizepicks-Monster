use crate::config;
use crate::error::AppError;
use crate::predictions::grading::{self, GradingSummary};
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use super::csv_export;

#[tauri::command]
pub async fn get_session_predictions(
    session_id: String,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<PredictionRecord>, String> {
    let t = tracker.lock().await;
    Ok(t.get_session_predictions(&session_id).await)
}

#[tauri::command]
pub async fn get_all_predictions(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<PredictionRecord>, String> {
    let t = tracker.lock().await;
    Ok(t.get_all_predictions().await)
}

#[tauri::command]
pub async fn get_prediction_stats(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<config::PredictionStats, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;
    let stats = t.get_stats(&all);
    Ok(config::PredictionStats::from_tracker(&stats))
}

#[tauri::command]
pub async fn get_predictions_by_confidence(
    min_score: Option<u8>,
    max_score: Option<u8>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<PredictionRecord>, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;
    let min = min_score.unwrap_or(0);
    let max = max_score.unwrap_or(100);
    Ok(all
        .into_iter()
        .filter(|r| {
            r.prediction
                .confidence_score
                .map_or(false, |s| s >= min && s <= max)
        })
        .collect())
}

#[tauri::command]
pub async fn get_overall_trend(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<crate::predictions::tracker::OverallTrend, String> {
    let t = tracker.lock().await;
    Ok(t.get_overall_trend().await)
}

#[tauri::command]
pub async fn get_player_trend(
    player_name: String,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Option<crate::predictions::tracker::PlayerTrend>, String> {
    let t = tracker.lock().await;
    Ok(t.get_player_trend(&player_name).await)
}

#[tauri::command]
pub async fn get_stat_category_trend(
    stat_category: String,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Option<crate::predictions::tracker::StatCategoryTrend>, String> {
    let t = tracker.lock().await;
    Ok(t.get_stat_category_trend(&stat_category).await)
}

#[tauri::command]
pub async fn get_trend_player_list(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<(String, u32)>, String> {
    let t = tracker.lock().await;
    Ok(t.get_player_list().await)
}

#[tauri::command]
pub async fn get_trend_stat_category_list(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<(String, u32)>, String> {
    let t = tracker.lock().await;
    Ok(t.get_stat_category_list().await)
}

#[tauri::command]
pub async fn update_prediction_outcome(
    prediction_id: String,
    outcome: String,
    actual_result: Option<f64>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<(), String> {
    let outcome = outcome
        .parse::<PredictionOutcome>()
        .map_err(|e| AppError::Validation(format!("Invalid outcome: {}", e)))?;
    let t = tracker.lock().await;
    t.update_outcome(&prediction_id, outcome, actual_result)
        .await
}

#[tauri::command]
pub async fn grade_pending_predictions(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<GradingSummary, String> {
    let pending = {
        let t = tracker.lock().await;
        let all = t.get_all_predictions().await;
        all.into_iter()
            .filter(|r| r.outcome == PredictionOutcome::Pending)
            .filter(|r| {
                r.prediction.player_name.is_some()
                    && r.prediction.stat_category.is_some()
                    && r.prediction.line.is_some()
                    && r.prediction.pick_type.is_some()
            })
            .map(|r| {
                (
                    r.prediction.id.clone(),
                    r.prediction.player_name.clone().unwrap_or_default(),
                    r.prediction.pick_type.clone().unwrap_or_default(),
                    r.prediction.line.unwrap_or(0.0),
                    r.prediction.stat_category.clone().unwrap_or_default(),
                    r.prediction.session_id.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    if pending.is_empty() {
        return Ok(GradingSummary {
            total_pending: 0,
            graded: 0,
            skipped: 0,
            wins: 0,
            losses: 0,
            pushes: 0,
            unresolved: 0,
            results: vec![],
            fetched_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    let summary = grading::grade_all_pending(&pending).await;

    let t = tracker.lock().await;
    for result in &summary.results {
        match result.outcome.as_str() {
            "Win" | "Loss" | "Push" => {
                let outcome = result
                    .outcome
                    .parse::<PredictionOutcome>()
                    .unwrap_or(PredictionOutcome::Pending);
                let _ = t
                    .update_outcome(&result.prediction_id, outcome, result.actual_result)
                    .await;
            }
            _ => {}
        }
    }

    Ok(summary)
}

#[tauri::command]
pub async fn get_grading_status(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<serde_json::Value, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;
    let pending_count = all
        .iter()
        .filter(|r| r.outcome == PredictionOutcome::Pending)
        .filter(|r| {
            r.prediction.player_name.is_some()
                && r.prediction.stat_category.is_some()
                && r.prediction.line.is_some()
                && r.prediction.pick_type.is_some()
        })
        .count();

    Ok(serde_json::json!({
        "total_predictions": all.len(),
        "pending_gradable": pending_count,
        "message": if pending_count > 0 {
            format!("{} pending predictions ready to grade", pending_count)
        } else {
            "No pending predictions to grade".to_string()
        }
    }))
}

#[tauri::command]
pub async fn export_predictions_csv(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<String, String> {
    let t = tracker.lock().await;
    let mut all = t.get_all_predictions().await;
    all.sort_by(|a, b| b.prediction.created_at.cmp(&a.prediction.created_at));

    csv_export(
        &["date", "player", "team", "pick_type", "line", "stat_category", "confidence", "confidence_score", "outcome", "actual_result"],
        |wtr| {
            for record in &all {
                let p = &record.prediction;
                wtr.write_record(&[
                    p.created_at.clone(),
                    p.player_name.clone().unwrap_or_default(),
                    String::new(),
                    p.pick_type.clone().unwrap_or_default(),
                    p.line.map(|l| l.to_string()).unwrap_or_default(),
                    p.stat_category.clone().unwrap_or_default(),
                    p.confidence.clone().unwrap_or_default(),
                    p.confidence_score.map(|s| s.to_string()).unwrap_or_default(),
                    record.outcome.to_string(),
                    record.actual_result.map(|r| r.to_string()).unwrap_or_default(),
                ])
                .map_err(|e| AppError::Io(format!("CSV row error: {e}")))?;
            }
            Ok(())
        },
    ).map_err(Into::into)
}
