use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn create_prediction_from_ocr(
    session_id: String,
    player_name: String,
    stat_category: String,
    line: f64,
    pick_type: String,
    source: String,
    stake: Option<f64>,
    potential_payout: Option<f64>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<String, String> {
    let prediction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let raw_text = format!(
        "[Bet Slip OCR - {}] {} {} {} {}",
        source, player_name, pick_type, line, stat_category
    );

    let notes = match (stake, potential_payout) {
        (Some(s), Some(p)) => Some(format!("Stake: ${:.2}, Potential Payout: ${:.2}", s, p)),
        (Some(s), None) => Some(format!("Stake: ${:.2}", s)),
        (None, Some(p)) => Some(format!("Potential Payout: ${:.2}", p)),
        (None, None) => None,
    };

    let prediction = crate::predictions::tracker::Prediction {
        id: prediction_id.clone(),
        session_id: session_id.clone(),
        raw_text,
        player_name: if player_name.is_empty() { None } else { Some(player_name) },
        pick_type: if pick_type.is_empty() { None } else { Some(pick_type) },
        line: if line > 0.0 { Some(line) } else { None },
        stat_category: if stat_category.is_empty() { None } else { Some(stat_category) },
        confidence: None,
        confidence_score: None,
        probability: None,
        reasoning: None,
        risk: None,
        created_at: now,
        full_decision_json: None,
    };

    let record = PredictionRecord {
        prediction,
        outcome: PredictionOutcome::Pending,
        actual_result: None,
        notes,
        resolved_at: None,
    };

    let t = tracker.lock().await;
    t.save_prediction(record).await?;

    Ok(prediction_id)
}
