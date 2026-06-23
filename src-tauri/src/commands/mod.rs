pub mod analysis_cmd;
pub mod bankroll_cmd;
pub mod bot_cmd;
pub mod chat_cmd;
pub mod config_cmd;
pub mod file_upload_cmd;
pub mod football_cmd;
pub mod line_tracker_cmd;
pub mod ml_cmd;
pub mod notification_cmd;
pub mod ocr_cmd;
pub mod paper_cmd;
pub mod prediction_cmd;
pub mod prizepicks_cmd;
pub mod weather_cmd;

pub use analysis_cmd::*;
pub use bankroll_cmd::*;
pub use bot_cmd::*;
pub use chat_cmd::*;
pub use config_cmd::*;
pub use file_upload_cmd::*;
pub use football_cmd::*;
pub use line_tracker_cmd::*;
pub use ml_cmd::*;
pub use notification_cmd::*;
pub use ocr_cmd::*;
pub use paper_cmd::*;
pub use prediction_cmd::*;
pub use prizepicks_cmd::*;
pub use weather_cmd::*;

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared type alias for PrizePicks client state used across command modules.
pub type PrizePicksState = Arc<Mutex<crate::prizepicks::PrizePicksClient>>;

/// Shared CSV export — writes headers + rows via a write closure, returns UTF-8 string.
pub fn csv_export(
    headers: &[&str],
    write_rows: impl FnOnce(&mut csv::Writer<Vec<u8>>) -> Result<(), AppError>,
) -> Result<String, AppError> {
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(headers)
        .map_err(|e| AppError::Io(format!("CSV header error: {e}")))?;
    write_rows(&mut wtr)?;
    wtr.flush()
        .map_err(|e| AppError::Io(format!("CSV flush error: {e}")))?;
    let bytes = wtr
        .into_inner()
        .map_err(|e| AppError::Io(format!("CSV inner error: {e}")))?;
    String::from_utf8(bytes)
        .map_err(|e| AppError::Serialization(format!("CSV encoding error: {e}")))
}

/// Input for generating bet recommendations from the frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PickInput {
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: f64,
    pub confidence_score: Option<u8>,
}

/// A parlay leg derived from a prediction, formatted for the frontend parlay builder.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParlayLeg {
    pub id: String,
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub confidence: String,
    pub confidence_score: Option<u8>,
    pub win_probability: Option<f64>,
    pub reasoning: Option<String>,
    pub risk: Option<String>,
}

impl From<&crate::predictions::tracker::PredictionRecord> for ParlayLeg {
    fn from(record: &crate::predictions::tracker::PredictionRecord) -> Self {
        let p = &record.prediction;
        ParlayLeg {
            id: p.id.clone(),
            player_name: p.player_name.clone().unwrap_or_default(),
            team: String::new(),
            opponent: String::new(),
            prop_category: p.stat_category.clone().unwrap_or_default(),
            line: p.line.unwrap_or(0.0),
            pick_type: p.pick_type.clone().unwrap_or_default(),
            confidence: p.confidence.clone().unwrap_or_default(),
            confidence_score: p.confidence_score,
            win_probability: p.probability,
            reasoning: p.reasoning.clone(),
            risk: p.risk.clone(),
        }
    }
}

/// Detect the league from message content
#[allow(dead_code)]
pub fn detect_league_from_message(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    if lower.contains("nfl") || lower.contains("football") {
        Some("football".to_string())
    } else if lower.contains("nba") || lower.contains("basketball") {
        Some("basketball".to_string())
    } else if lower.contains("mlb") || lower.contains("baseball") {
        Some("baseball".to_string())
    } else if lower.contains("nhl") || lower.contains("hockey") {
        Some("hockey".to_string())
    } else {
        None
    }
}
