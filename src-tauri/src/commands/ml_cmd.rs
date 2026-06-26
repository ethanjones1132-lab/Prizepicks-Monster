use sqlx::{Pool, Sqlite};
use tauri::State;

#[tauri::command]
pub async fn ml_train_model(
    db_path: Option<String>,
    output_path: Option<String>,
) -> Result<crate::ml_predictor::MLTrainingResult, String> {
    crate::ml_predictor::train_model(db_path.as_deref(), output_path.as_deref()).await
}

#[tauri::command]
pub async fn ml_predict_batch(
    db_path: Option<String>,
    model_path: Option<String>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::ml_predictor::MLPredictionBatch, String> {
    let batch =
        crate::ml_predictor::predict_batch(db_path.as_deref(), model_path.as_deref()).await?;

    if batch.status == "ok" && !batch.predictions.is_empty() {
        let model_ver = batch
            .model_path
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let _ = crate::ml_predictor::save_ml_predictions(&db_pool, &batch.predictions, &model_ver)
            .await;
    }

    Ok(batch)
}

#[tauri::command]
pub async fn ml_get_model_status(
    model_path: Option<String>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::ml_predictor::MLModelStatus, String> {
    crate::ml_predictor::get_model_status(&db_pool, model_path.as_deref()).await
}

#[tauri::command]
pub async fn ml_get_predictions(
    limit: Option<i64>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<crate::ml_predictor::MLPrediction>, String> {
    let limit = limit.unwrap_or(50);
    crate::ml_predictor::get_stored_ml_predictions(&db_pool, limit).await
}

#[tauri::command]
pub async fn ml_export_features(
    output_path: Option<String>,
    _db_pool: State<'_, Pool<Sqlite>>,
) -> Result<String, String> {
    crate::ml_predictor::export_features_csv(output_path.as_deref()).await
}

#[tauri::command]
pub async fn ml_train_per_category(
    db_path: Option<String>,
    output_dir: Option<String>,
    min_samples: Option<i64>,
) -> Result<crate::ml_predictor::MLCategoryTrainResult, String> {
    crate::ml_predictor::train_per_category(
        db_path.as_deref(),
        output_dir.as_deref(),
        min_samples,
    )
    .await
}

#[tauri::command]
pub async fn ml_predict_batch_per_category(
    db_path: Option<String>,
    model_dir: Option<String>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::ml_predictor::MLPredictionBatch, String> {
    let batch = crate::ml_predictor::predict_batch_per_category(
        db_path.as_deref(),
        model_dir.as_deref(),
    )
    .await?;
    if batch.status == "ok" && !batch.predictions.is_empty() {
        let model_ver = batch
            .model_path
            .clone()
            .unwrap_or_else(|| "per_category_unknown".to_string());
        let _ = crate::ml_predictor::save_ml_predictions(&db_pool, &batch.predictions, &model_ver)
            .await;
    }
    Ok(batch)
}

#[tauri::command]
pub async fn ml_get_category_models(
    model_dir: Option<String>,
) -> Result<crate::ml_predictor::MLCategoryModelList, String> {
    Ok(crate::ml_predictor::list_category_models(model_dir.as_deref()))
}
