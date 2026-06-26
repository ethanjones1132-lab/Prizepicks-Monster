#![allow(dead_code)]
// ═══════════════════════════════════════════════════════════════
// ML Predictor — Python-interop ML training & inference engine
//
// Bridges the SQLite prediction store with a Python scikit-learn
// model (GradientBoosting classifier) for prop outcome prediction.
//
// Flow:
//   1. Rust extracts features from SQLite (predictions + line movements)
//   2. Shells out to ml_predictor.py for training and inference
//   3. Stores ML predictions back in SQLite for frontend display
//   4. Injects ML context into the AI chat prompt
//
// The Python script lives at:
//   src-tauri/src/ml_predictor.py
// ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;
use std::process::Command;

// ═══════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════

/// ML model training result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLTrainingResult {
    pub status: String,
    pub samples: Option<i64>,
    pub cv_accuracy_mean: Option<f64>,
    pub cv_accuracy_std: Option<f64>,
    pub win_rate: Option<f64>,
    pub model_path: Option<String>,
    pub feature_importance: Option<Vec<MLFeatureImportance>>,
    pub message: String,
}

/// Feature importance from the trained model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLFeatureImportance {
    pub feature: String,
    pub importance: f64,
}

/// A single ML prediction for a pending prop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLPrediction {
    pub prediction_id: String,
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub ml_win_probability: f64,
    pub ml_prediction: String,
    pub original_confidence: i64,
    pub original_probability: Option<f64>,
    pub line_change: f64,
}

/// Batch prediction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLPredictionBatch {
    pub status: String,
    pub model_path: Option<String>,
    pub predictions_count: i64,
    pub predictions: Vec<MLPrediction>,
    pub message: String,
}

/// ML model status for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLModelStatus {
    pub model_exists: bool,
    pub model_path: String,
    pub trained_at: Option<String>,
    pub samples: Option<i64>,
    pub cv_accuracy_mean: Option<f64>,
    pub cv_accuracy_std: Option<f64>,
    pub win_rate: Option<f64>,
    pub feature_importance: Option<Vec<MLFeatureImportance>>,
    pub pending_predictions: i64,
    pub resolved_predictions: i64,
    pub message: String,
}

/// Result of training a single per-stat-category model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLCategoryModelResult {
    pub category: String,
    pub token: String,
    pub status: String,
    pub samples: i64,
    pub win_rate: f64,
    pub model_path: Option<String>,
    pub cv_accuracy_mean: Option<f64>,
    pub cv_accuracy_std: Option<f64>,
    pub feature_importance: Vec<MLFeatureImportance>,
    pub message: String,
}

/// Outcome of training one model per stat_category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLCategoryTrainResult {
    pub status: String,
    pub message: String,
    pub output_dir: String,
    pub trained_count: i64,
    pub skipped_count: i64,
    pub min_samples: i64,
    pub categories: Vec<MLCategoryModelResult>,
}

/// Summary of a single per-category model on disk (read from its _meta.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLCategoryModelInfo {
    pub category: String,
    pub token: String,
    pub model_path: String,
    pub meta_path: String,
    pub trained_at: Option<String>,
    pub samples: Option<i64>,
    pub cv_accuracy_mean: Option<f64>,
    pub cv_accuracy_std: Option<f64>,
    pub win_rate: Option<f64>,
    pub feature_importance: Vec<MLFeatureImportance>,
}

/// Envelope returned from `list_category_models` — one entry per model file
/// on disk plus an aggregate status / message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLCategoryModelList {
    pub status: String,
    pub model_dir: String,
    pub message: String,
    pub models: Vec<MLCategoryModelInfo>,
}

/// ML-enhanced analysis context — extends the existing AnalysisContext
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLAnalysisContext {
    pub ml_predictions: Vec<MLPrediction>,
    pub model_accuracy: Option<f64>,
    pub model_samples: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════
// Paths
// ═══════════════════════════════════════════════════════════════

/// Path to the Python ML script
fn ml_script_path() -> PathBuf {
    // In production, this is relative to the app bundle.
    // For development, use the source directory.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        PathBuf::from(manifest).join("src/ml_predictor.py")
    } else {
        PathBuf::from("src/ml_predictor.py")
    }
}

/// Default model output path
fn default_model_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".openclaw/prizepicks-monster/ml_model.joblib")
}

/// Default predictions db path
pub fn default_db_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".openclaw/prizepicks-monster/predictions.db")
}

/// Default per-category model directory.
///
/// Returns ``<config_dir>/ml_models`` where ``<config_dir>`` is the same root
/// used for the single-model path. Per-category training writes
/// ``ml_model_<token>.joblib`` files here.
pub fn default_category_model_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".openclaw/prizepicks-monster/ml_models")
}

/// Convert a stat_category string into a filesystem-safe token.
///
/// Mirrors :func:`ml_predictor._safe_filename_token` in the Python side. We
/// keep the implementation here so the Rust side can also reason about file
/// paths without shelling out to Python (used by the listing helper).
pub fn safe_category_token(category: &str) -> String {
    if category.trim().is_empty() {
        return "uncategorized".to_string();
    }
    let mut out = String::with_capacity(category.len());
    let mut last_was_underscore = false;
    for ch in category.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            out.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    let trimmed = out.trim_matches(|c: char| c == '_' || c == '.' || c == '-').to_string();
    if trimmed.is_empty() {
        "uncategorized".to_string()
    } else {
        trimmed
    }
}

/// Model metadata path
fn model_meta_path(model_path: &PathBuf) -> PathBuf {
    model_path.with_file_name(format!(
        "{}_meta.json",
        model_path.file_stem().unwrap_or_default().to_string_lossy()
    ))
}

/// Public wrapper for tests — exposes the same path-derivation logic
/// that `get_model_status` and `predict_batch` use internally.
pub fn model_meta_path_for(model_path: &PathBuf) -> PathBuf {
    model_meta_path(model_path)
}

// ═══════════════════════════════════════════════════════════════
// Core Operations
// ═══════════════════════════════════════════════════════════════

/// Train one GradientBoosting model per stat_category.
///
/// Shells out to ``ml_predictor.py train-per-category`` and parses its JSON
/// output into an :struct:`MLCategoryTrainResult`. ``min_samples`` defaults
/// to 10 to match the single-model gate, and the output directory defaults
/// to the same per-category directory used by the rest of the per-category
/// surface.
pub async fn train_per_category(
    db_path: Option<&str>,
    output_dir: Option<&str>,
    min_samples: Option<i64>,
) -> Result<MLCategoryTrainResult, String> {
    let db = db_path.map(PathBuf::from).unwrap_or_else(default_db_path);
    let output = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_category_model_dir);
    let script = ml_script_path();

    if !script.exists() {
        return Err(format!(
            "ML script not found at {}. Ensure ml_predictor.py is in the src/ directory.",
            script.display()
        ));
    }

    let min_samples = min_samples.unwrap_or(10);
    let output_str = output.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("train-per-category")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--output-dir")
            .arg(&output_str)
            .arg("--min-samples")
            .arg(min_samples.to_string())
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if !out.status.success() {
            return Err(format!(
                "ml_predictor.py train-per-category failed (exit {}): {}",
                out.status, stderr
            ));
        }

        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        let result: MLCategoryTrainResult = serde_json::from_str(json_line).map_err(|e| {
            format!(
                "Failed to parse ml_predictor per-category output: {}\nRaw: {}",
                e, json_line
            )
        })?;

        Ok::<_, String>(result)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result)
}

/// List per-category model files on disk by globbing for ``ml_model_*_meta.json``.
///
/// Pure filesystem operation — does not shell out to Python. Used by the
/// frontend to render the per-category metrics table without re-training.
pub fn list_category_models(model_dir: Option<&str>) -> MLCategoryModelList {
    let dir = model_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_category_model_dir);
    let dir_str = dir.display().to_string();

    if !dir.exists() {
        return MLCategoryModelList {
            status: "no_models".to_string(),
            model_dir: dir_str.clone(),
            message: format!(
                "Model directory {} does not exist. Train first.",
                dir_str
            ),
            models: vec![],
        };
    }

    let mut models: Vec<MLCategoryModelInfo> = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(err) => {
            return MLCategoryModelList {
                status: "error".to_string(),
                model_dir: dir_str.clone(),
                message: format!("Failed to read_dir {}: {}", dir_str, err),
                models: vec![],
            };
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Only consume <token>_meta.json files. We skip the single-model
        // `ml_model_meta.json` (no underscore-suffixed category token).
        if !name.starts_with("ml_model_") || !name.ends_with("_meta.json") {
            continue;
        }
        // The category token lives between the prefix and the suffix.
        let token = name
            .trim_start_matches("ml_model_")
            .trim_end_matches("_meta.json")
            .to_string();

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_err) => {
                let joblib_name = format!("ml_model_{}.joblib", token);
                models.push(MLCategoryModelInfo {
                    category: token.clone(),
                    token: token.clone(),
                    model_path: path.with_file_name(joblib_name).display().to_string(),
                    meta_path: path.display().to_string(),
                    trained_at: None,
                    samples: None,
                    cv_accuracy_mean: None,
                    cv_accuracy_std: None,
                    win_rate: None,
                    feature_importance: vec![],
                });
                continue;
            }
        };
        #[derive(Deserialize)]
        struct Meta {
            category: String,
            token: Option<String>,
            trained_at: String,
            samples: i64,
            cv_accuracy_mean: f64,
            cv_accuracy_std: f64,
            win_rate: f64,
            feature_importance: Vec<MLFeatureImportance>,
        }
        match serde_json::from_str::<Meta>(&content) {
            Ok(m) => {
                let token_for_path = m.token.clone().unwrap_or_else(|| token.clone());
                let joblib_name = format!("ml_model_{}.joblib", token_for_path);
                models.push(MLCategoryModelInfo {
                    category: m.category,
                    token: m.token.unwrap_or(token),
                    model_path: path.with_file_name(joblib_name).display().to_string(),
                    meta_path: path.display().to_string(),
                    trained_at: Some(m.trained_at),
                    samples: Some(m.samples),
                    cv_accuracy_mean: Some(m.cv_accuracy_mean),
                    cv_accuracy_std: Some(m.cv_accuracy_std),
                    win_rate: Some(m.win_rate),
                    feature_importance: m.feature_importance,
                });
            }
            Err(_) => continue, // silently skip unparseable meta
        }
    }

    let status = if models.is_empty() {
        "no_models"
    } else {
        "ok"
    };
    let message = format!("Found {} per-category model(s) on disk.", models.len());
    MLCategoryModelList {
        status: status.to_string(),
        model_dir: dir_str,
        message,
        models,
    }
}

/// Generate ML predictions for all pending props using per-category models.
pub async fn predict_batch_per_category(
    db_path: Option<&str>,
    model_dir: Option<&str>,
) -> Result<MLPredictionBatch, String> {
    let db = db_path.map(PathBuf::from).unwrap_or_else(default_db_path);
    let dir = model_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_category_model_dir);
    let script = ml_script_path();

    if !script.exists() {
        return Err(format!("ML script not found at {}", script.display()));
    }

    if !dir.exists() {
        return Ok(MLPredictionBatch {
            status: "no_model".to_string(),
            model_path: Some(dir.display().to_string()),
            predictions_count: 0,
            predictions: vec![],
            message: format!(
                "Per-category model directory {} not found. Train first.",
                dir.display()
            ),
        });
    }

    let dir_str = dir.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("predict-per-category")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--model-dir")
            .arg(&dir_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if !out.status.success() {
            return Err(format!(
                "ml_predictor.py predict-per-category failed (exit {}): {}",
                out.status, stderr
            ));
        }

        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        #[derive(Deserialize)]
        struct RawBatch {
            status: String,
            predictions_count: i64,
            predictions: Vec<MLPrediction>,
            message: Option<String>,
            model_dir: Option<String>,
        }
        let raw: RawBatch = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse per-category predict output: {}", e))?;

        let model_path = raw
            .model_dir
            .unwrap_or_else(|| dir.display().to_string());
        let message = raw.message.unwrap_or_else(|| match raw.status.as_str() {
            "ok" => format!("Generated {} ML predictions", raw.predictions_count),
            "no_pending" => "No pending predictions to score".to_string(),
            "no_models" => "No per-category models matched the pending props.".to_string(),
            _ => format!("Status: {}", raw.status),
        });

        Ok::<_, String>(MLPredictionBatch {
            status: raw.status,
            model_path: Some(model_path),
            predictions_count: raw.predictions_count,
            predictions: raw.predictions,
            message,
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result)
}

/// Train the ML model on historical prediction data
pub async fn train_model(
    db_path: Option<&str>,
    output_path: Option<&str>,
) -> Result<MLTrainingResult, String> {
    let db = db_path.map(PathBuf::from).unwrap_or_else(default_db_path);
    let output = output_path
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let script = ml_script_path();

    if !script.exists() {
        return Err(format!(
            "ML script not found at {}. Ensure ml_predictor.py is in the src/ directory.",
            script.display()
        ));
    }

    let output_str = output.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("train")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--output")
            .arg(&output_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if !out.status.success() {
            return Err(format!(
                "ml_predictor.py failed (exit {}): {}",
                out.status, stderr
            ));
        }

        // Parse JSON output (last line)
        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        let result: MLTrainingResult = serde_json::from_str(json_line).map_err(|e| {
            format!(
                "Failed to parse ml_predictor output: {}\nRaw: {}",
                e, json_line
            )
        })?;

        Ok::<_, String>(result)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result)
}

/// Generate ML predictions for all pending props
pub async fn predict_batch(
    db_path: Option<&str>,
    model_path: Option<&str>,
) -> Result<MLPredictionBatch, String> {
    let db = db_path.map(PathBuf::from).unwrap_or_else(default_db_path);
    let model = model_path
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let script = ml_script_path();

    if !script.exists() {
        return Err(format!("ML script not found at {}", script.display()));
    }

    if !model.exists() {
        return Ok(MLPredictionBatch {
            status: "no_model".to_string(),
            model_path: None,
            predictions_count: 0,
            predictions: vec![],
            message: "Model not found. Train first using ml_train.".to_string(),
        });
    }

    let model_str = model.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("predict")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--model")
            .arg(&model_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if !out.status.success() {
            return Err(format!(
                "ml_predictor.py failed (exit {}): {}",
                out.status, stderr
            ));
        }

        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        #[derive(Deserialize)]
        struct RawBatch {
            status: String,
            model_path: Option<String>,
            predictions_count: i64,
            predictions: Vec<MLPrediction>,
        }

        let raw: RawBatch = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse prediction output: {}", e))?;

        let message = match raw.status.as_str() {
            "ok" => format!("Generated {} ML predictions", raw.predictions_count),
            "no_pending" => "No pending predictions to score".to_string(),
            "no_model" => "Model not found. Train first.".to_string(),
            _ => format!("Status: {}", raw.status),
        };

        Ok::<_, String>(MLPredictionBatch {
            status: raw.status,
            model_path: raw.model_path,
            predictions_count: raw.predictions_count,
            predictions: raw.predictions,
            message,
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result)
}

/// Get ML model status including training metadata
pub async fn get_model_status(
    pool: &Pool<Sqlite>,
    model_path: Option<&str>,
) -> Result<MLModelStatus, String> {
    let model = model_path
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let meta_path = model_meta_path(&model);

    let model_exists = model.exists();

    // Load metadata if available
    let (trained_at, samples, cv_mean, cv_std, win_rate, feature_importance) = if meta_path.exists()
    {
        let content = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("Failed to read model meta: {}", e))?;
        #[derive(Deserialize)]
        struct Meta {
            trained_at: String,
            samples: i64,
            cv_accuracy_mean: f64,
            cv_accuracy_std: f64,
            win_rate: f64,
            feature_importance: Vec<MLFeatureImportance>,
        }
        match serde_json::from_str::<Meta>(&content) {
            Ok(m) => (
                Some(m.trained_at),
                Some(m.samples),
                Some(m.cv_accuracy_mean),
                Some(m.cv_accuracy_std),
                Some(m.win_rate),
                Some(m.feature_importance),
            ),
            Err(_) => (None, None, None, None, None, None),
        }
    } else {
        (None, None, None, None, None, None)
    };

    // Count predictions
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM predictions WHERE outcome = 'Pending'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    let resolved: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM predictions WHERE outcome IN ('Win', 'Loss', 'Push')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let message = if !model_exists {
        "No model trained yet. Need at least 10 resolved predictions to train.".to_string()
    } else if let Some(s) = samples {
        format!(
            "Model trained on {} samples. CV accuracy: {:.1}%",
            s,
            cv_mean.unwrap_or(0.0) * 100.0
        )
    } else {
        "Model file exists but metadata is missing. Retrain for best results.".to_string()
    };

    Ok(MLModelStatus {
        model_exists,
        model_path: model.display().to_string(),
        trained_at,
        samples,
        cv_accuracy_mean: cv_mean,
        cv_accuracy_std: cv_std,
        win_rate,
        feature_importance,
        pending_predictions: pending,
        resolved_predictions: resolved,
        message,
    })
}

/// Store ML predictions in the database for frontend display
/// Adds ml_win_probability to a dedicated table
pub async fn init_ml_tables(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ml_predictions (
            id TEXT PRIMARY KEY,
            prediction_id TEXT NOT NULL,
            ml_win_probability REAL NOT NULL,
            ml_prediction TEXT NOT NULL,
            ml_model_version TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (prediction_id) REFERENCES predictions(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create ml_predictions table: {}", e))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ml_pred_prediction ON ml_predictions(prediction_id)",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_ml_pred_ticker ON ml_predictions(ticker)")
        .execute(pool)
        .await
        .ok();

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_ml_pred_created ON ml_predictions(created_at)")
        .execute(pool)
        .await
        .ok();

    Ok(())
}

/// Save a batch of ML predictions to the database
pub async fn save_ml_predictions(
    pool: &Pool<Sqlite>,
    predictions: &[MLPrediction],
    model_version: &str,
) -> Result<usize, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut saved = 0;

    for pred in predictions {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO ml_predictions
                (id, prediction_id, ml_win_probability, ml_prediction, ml_model_version, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(&id)
        .bind(&pred.prediction_id)
        .bind(pred.ml_win_probability)
        .bind(&pred.ml_prediction)
        .bind(model_version)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to save ML prediction: {}", e))?;
        saved += 1;
    }

    Ok(saved)
}

/// Get stored ML predictions for display
pub async fn get_stored_ml_predictions(
    pool: &Pool<Sqlite>,
    limit: i64,
) -> Result<Vec<MLPrediction>, String> {
    let rows = sqlx::query(
        r#"
        SELECT mp.prediction_id, p.player_name, p.stat_category, p.line,
               mp.ml_win_probability, mp.ml_prediction,
               p.confidence_score, p.probability
        FROM ml_predictions mp
        JOIN predictions p ON mp.prediction_id = p.id
        ORDER BY mp.created_at DESC
        LIMIT ?1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch ML predictions: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| MLPrediction {
            prediction_id: r.get("prediction_id"),
            player_name: r.get("player_name"),
            stat_category: r.get("stat_category"),
            line: r.get("line"),
            ml_win_probability: r.get("ml_win_probability"),
            ml_prediction: r.get("ml_prediction"),
            original_confidence: r.get::<Option<i64>, _>("confidence_score").unwrap_or(50),
            original_probability: r.get("probability"),
            line_change: 0.0, // not stored in ml_predictions table
        })
        .collect())
}

/// Generate ML context string for AI prompt injection
pub fn generate_ml_context(predictions: &[MLPrediction], accuracy: Option<f64>) -> String {
    if predictions.is_empty() {
        return String::new();
    }

    let acc_str = accuracy.map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
    let mut ctx = format!("🤖 ML MODEL PREDICTIONS (accuracy: {}):\n", acc_str);

    for pred in predictions.iter().take(10) {
        let emoji = if pred.ml_win_probability >= 0.6 {
            "✅"
        } else if pred.ml_win_probability >= 0.45 {
            "⚠️"
        } else {
            "❌"
        };
        ctx.push_str(&format!(
            "  {} {} {} {} — ML Win Prob: {:.1}% ({}), Line: {:.1}\n",
            emoji,
            pred.player_name,
            pred.ml_prediction,
            pred.stat_category,
            pred.ml_win_probability * 100.0,
            if pred.ml_win_probability >= 0.5 {
                "Lean Over"
            } else {
                "Lean Under"
            },
            pred.line
        ));
    }

    ctx.push('\n');
    ctx
}

/// Export features as CSV for external analysis
pub async fn export_features_csv(output_path: Option<&str>) -> Result<String, String> {
    let db = default_db_path();
    let output = output_path.map(PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".openclaw/prizepicks-monster/ml_features.csv")
    });
    let script = ml_script_path();

    let output_str = output.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("export-features")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--output")
            .arg(&output_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();

        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        #[derive(Deserialize)]
        struct ExportResult {
            status: String,
            samples: Option<i64>,
            output_path: String,
        }

        let r: ExportResult = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse export output: {}", e))?;

        Ok::<_, String>(r)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result.output_path)
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Compare two path strings ignoring separator differences (Windows uses
    /// `\` while Unix uses `/`). Splits on either separator and rejoins with `/`.
    fn paths_eq(a: &str, b: &str) -> bool {
        a.split(|c| c == '/' || c == '\\')
            .collect::<Vec<_>>()
            == b.split(|c| c == '/' || c == '\\').collect::<Vec<_>>()
    }

    #[test]
    fn model_meta_path_strips_joblib_and_appends_meta_json() {
        let p = PathBuf::from("/home/user/.openclaw/prizepicks-monster/ml_model.joblib");
        let meta = model_meta_path_for(&p);
        assert!(
            paths_eq(
                &meta.to_string_lossy(),
                "/home/user/.openclaw/prizepicks-monster/ml_model_meta.json"
            ),
            "got {:?}",
            meta
        );
    }

    #[test]
    fn model_meta_path_handles_alternate_filename() {
        let p = PathBuf::from("C:/models/prop_v2.joblib");
        let meta = model_meta_path_for(&p);
        assert!(
            paths_eq(&meta.to_string_lossy(), "C:/models/prop_v2_meta.json"),
            "got {:?}",
            meta
        );
    }

    #[test]
    fn model_meta_path_preserves_directory() {
        let p = PathBuf::from("models/subdir/foo.joblib");
        let meta = model_meta_path_for(&p);
        // PathBuf::with_file_name replaces only the file portion
        assert!(
            paths_eq(&meta.to_string_lossy(), "models/subdir/foo_meta.json"),
            "got {:?}",
            meta
        );
    }

    #[test]
    fn ml_context_with_empty_predictions_returns_empty_string() {
        let ctx = generate_ml_context(&[], Some(0.65));
        assert!(ctx.is_empty());
    }

    #[test]
    fn ml_context_includes_accuracy_when_provided() {
        let preds = vec![MLPrediction {
            prediction_id: "p1".to_string(),
            player_name: "Test Player".to_string(),
            stat_category: "Points".to_string(),
            line: 25.5,
            ml_win_probability: 0.72,
            ml_prediction: "Win".to_string(),
            original_confidence: 70,
            original_probability: Some(68.0),
            line_change: 0.0,
        }];
        let ctx = generate_ml_context(&preds, Some(0.78));
        assert!(ctx.contains("78.0%"));
        assert!(ctx.contains("Test Player"));
        assert!(ctx.contains("Points"));
        assert!(ctx.contains("72.0%"));
        assert!(ctx.contains("Lean Over"));
    }

    #[test]
    fn ml_context_uses_na_when_accuracy_missing() {
        let preds = vec![MLPrediction {
            prediction_id: "p1".to_string(),
            player_name: "P2".to_string(),
            stat_category: "Rebounds".to_string(),
            line: 8.0,
            ml_win_probability: 0.35,
            ml_prediction: "Loss".to_string(),
            original_confidence: 50,
            original_probability: None,
            line_change: -0.2,
        }];
        let ctx = generate_ml_context(&preds, None);
        assert!(ctx.contains("N/A"));
        assert!(ctx.contains("Lean Under"));
    }

    #[test]
    fn ml_context_caps_at_ten_predictions() {
        let preds: Vec<MLPrediction> = (0..15)
            .map(|i| MLPrediction {
                prediction_id: format!("p{}", i),
                player_name: format!("Player {}", i),
                stat_category: "Points".to_string(),
                line: 20.0,
                ml_win_probability: 0.5,
                ml_prediction: "Win".to_string(),
                original_confidence: 50,
                original_probability: None,
                line_change: 0.0,
            })
            .collect();
        let ctx = generate_ml_context(&preds, Some(0.6));
        // Should include first 10 only
        assert!(ctx.contains("Player 0"));
        assert!(ctx.contains("Player 9"));
        assert!(!ctx.contains("Player 10"));
        assert!(!ctx.contains("Player 14"));
    }

    // ── Per-category helpers ──

    #[test]
    fn safe_category_token_keeps_alphanumeric() {
        assert_eq!(safe_category_token("Points"), "Points");
        assert_eq!(safe_category_token("3-Pt Made"), "3-Pt_Made");
        assert_eq!(safe_category_token("FG%"), "FG");
    }

    #[test]
    fn safe_category_token_collapses_special_chars() {
        assert_eq!(safe_category_token("Pts/Rebs"), "Pts_Rebs");
        assert_eq!(safe_category_token("a   b"), "a_b");
    }

    #[test]
    fn safe_category_token_handles_empty_and_whitespace() {
        assert_eq!(safe_category_token(""), "uncategorized");
        assert_eq!(safe_category_token("   "), "uncategorized");
        assert_eq!(safe_category_token("___"), "uncategorized");
        assert_eq!(safe_category_token("..."), "uncategorized");
    }

    #[test]
    fn safe_category_token_strips_edge_punctuation() {
        // Leading/trailing punctuation is trimmed before falling back to default.
        assert_eq!(safe_category_token("__Points__"), "Points");
        assert_eq!(safe_category_token("...Assists..."), "Assists");
    }

    #[test]
    fn default_category_model_dir_lives_under_openclaw_prizepicks() {
        let dir = default_category_model_dir();
        let s = dir.to_string_lossy();
        assert!(
            s.contains("prizepicks-monster"),
            "expected prizepicks-monster in path, got {}",
            s
        );
        assert!(s.ends_with("ml_models"), "expected ml_models suffix, got {}", s);
    }

    #[test]
    fn list_category_models_returns_no_models_for_missing_dir() {
        let result = list_category_models(Some("C:/path/that/should/never/exist/here"));
        assert_eq!(result.status, "no_models");
        assert_eq!(result.models.len(), 0);
        assert!(result.message.contains("does not exist"));
    }

    #[test]
    fn list_category_models_returns_no_models_for_empty_dir() {
        // Use a unique temp directory that we know is empty.
        let dir = std::env::temp_dir().join(format!(
            "ppml_list_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let result = list_category_models(Some(&dir.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(result.status, "no_models");
        assert_eq!(result.models.len(), 0);
    }

    #[test]
    fn list_category_models_reads_meta_files_in_dir() {
        let dir = std::env::temp_dir().join(format!(
            "ppml_list_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Write two per-category meta files with valid JSON.
        let meta_a = r#"{
            "trained_at": "2026-06-26T10:00:00+00:00",
            "category": "Points",
            "token": "Points",
            "samples": 42,
            "cv_accuracy_mean": 0.66,
            "cv_accuracy_std": 0.04,
            "feature_names": ["line","conf"],
            "feature_importance": [{"feature":"line","importance":0.7},{"feature":"conf","importance":0.3}],
            "win_rate": 0.58,
            "num_features": 2
        }"#;
        let meta_b = r#"{
            "trained_at": "2026-06-26T10:00:00+00:00",
            "category": "Rebounds",
            "token": "Rebounds",
            "samples": 31,
            "cv_accuracy_mean": 0.61,
            "cv_accuracy_std": 0.05,
            "feature_names": ["line","conf"],
            "feature_importance": [{"feature":"line","importance":0.4},{"feature":"conf","importance":0.6}],
            "win_rate": 0.55,
            "num_features": 2
        }"#;
        let path_a = dir.join("ml_model_Points_meta.json");
        let path_b = dir.join("ml_model_Rebounds_meta.json");
        std::fs::write(&path_a, meta_a).unwrap();
        std::fs::write(&path_b, meta_b).unwrap();
        // Also drop a file that should be ignored: the single-model meta
        // (no underscore-suffixed category token between `ml_model_` and
        // `_meta.json`).
        std::fs::write(
            dir.join("ml_model_meta.json"),
            r#"{"trained_at":"x","samples":1,"cv_accuracy_mean":0.5,"cv_accuracy_std":0.1,"win_rate":0.5,"feature_importance":[]}"#,
        )
        .unwrap();

        let result = list_category_models(Some(&dir.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(result.status, "ok");
        assert_eq!(result.models.len(), 2, "expected 2 per-category models");
        let names: Vec<&str> = result.models.iter().map(|m| m.category.as_str()).collect();
        assert!(names.contains(&"Points"));
        assert!(names.contains(&"Rebounds"));
        // Feature importance should be parsed for both.
        for m in &result.models {
            assert_eq!(m.feature_importance.len(), 2);
        }
        // The single-model meta (ml_model_meta.json) should be ignored.
        for m in &result.models {
            assert!(m.model_path.contains("ml_model_"));
            assert!(m.model_path.ends_with(".joblib"));
        }
    }

    #[test]
    fn list_category_models_skips_unparseable_meta() {
        let dir = std::env::temp_dir().join(format!(
            "ppml_list_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ml_model_Points_meta.json"), b"not valid json").unwrap();
        let result = list_category_models(Some(&dir.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&dir);
        // Garbage meta is silently dropped.
        assert_eq!(result.models.len(), 0);
        assert_eq!(result.status, "no_models");
    }
}
