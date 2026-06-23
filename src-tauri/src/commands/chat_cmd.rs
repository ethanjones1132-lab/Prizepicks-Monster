use crate::chat::openrouter::{self, OpenRouterResponse};
use crate::chat::session;
use crate::chat::prizepicks_context;
use crate::config::AppConfig;
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{mpsc, Mutex};

#[tauri::command]
pub async fn send_message(
    message: String,
    session_id: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
    _prizepicks: State<'_, Arc<Mutex<crate::prizepicks::PrizePicksClient>>>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<OpenRouterResponse, String> {
    let config = state.lock().await.clone();

    {
        let mut cs = chat_state.lock().await;
        if !cs.sessions.contains_key(&session_id) {
            cs.load_from_disk(&session_id);
        }
    }

    let session_messages = {
        let cs = chat_state.lock().await;
        let mut messages = cs.get_messages(&session_id);
        if messages.len() > 24 {
            messages = messages.split_off(messages.len() - 24);
        }
        messages
            .into_iter()
            .map(|m| openrouter::ChatMessage {
                role: m.role,
                content: m.content,
                reasoning: m.reasoning,
            })
            .collect::<Vec<_>>()
    };

    let prizepicks_context_val =
        { prizepicks_context::build_prizepicks_context(&message).await };

    let response = openrouter::send_message(
        &config,
        &session_messages,
        message.clone(),
        None,
        Some(&db_pool),
        Some(&prizepicks_context_val),
    )
    .await?;

    let user_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: message,
        reasoning: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: None,
    };

    let assistant_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: response.content.clone(),
        reasoning: response.reasoning.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: response.tokens_used,
    };

    {
        let mut cs = chat_state.lock().await;
        cs.add_message(&session_id, user_msg.clone());
        cs.add_message(&session_id, assistant_msg.clone());
    }

    let all_messages = {
        let cs = chat_state.lock().await;
        cs.get_messages(&session_id)
    };
    let _ = session::save_session_messages(&session_id, &all_messages);

    {
        let t = tracker.lock().await;
        let extracted = t.extract_predictions(&session_id, &response.content);
        for pred in extracted {
            let record = PredictionRecord {
                prediction: pred,
                outcome: PredictionOutcome::Pending,
                actual_result: None,
                notes: None,
                resolved_at: None,
            };
            let _ = t.save_prediction(record).await;
        }
    }

    Ok(response)
}

#[tauri::command]
pub async fn send_message_stream(
    message: String,
    session_id: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
    _prizepicks: State<'_, Arc<Mutex<crate::prizepicks::PrizePicksClient>>>,
    db_pool: State<'_, Pool<Sqlite>>,
    app: tauri::AppHandle<tauri::Wry>,
) -> Result<(), String> {
    let config = state.lock().await.clone();

    {
        let mut cs = chat_state.lock().await;
        if !cs.sessions.contains_key(&session_id) {
            cs.load_from_disk(&session_id);
        }
    }

    let session_messages = {
        let cs = chat_state.lock().await;
        let mut messages = cs.get_messages(&session_id);
        if messages.len() > 24 {
            messages = messages.split_off(messages.len() - 24);
        }
        messages
            .into_iter()
            .map(|m| openrouter::ChatMessage {
                role: m.role,
                content: m.content,
                reasoning: m.reasoning,
            })
            .collect::<Vec<_>>()
    };

    let (tx, mut rx) = mpsc::channel::<String>(256);
    let session_id_clone = session_id.clone();
    let app_clone = app.clone();

    let forward_handle = tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            if chunk == "__STREAM_DONE__" {
                let _ = app_clone.emit("stream-done", &session_id_clone);
                break;
            }
            if chunk.starts_with("__STREAM_ERROR__:") {
                let error_msg = &chunk["__STREAM_ERROR__:".len()..];
                let _ = app_clone.emit(
                    "stream-error",
                    serde_json::json!({
                        "session_id": session_id_clone,
                        "error": error_msg,
                    }),
                );
                break;
            }
            if chunk.starts_with("__STREAM_THOUGHT__:") {
                let thought = &chunk["__STREAM_THOUGHT__:".len()..];
                let _ = app_clone.emit(
                    "stream-thought",
                    serde_json::json!({
                        "session_id": session_id_clone,
                        "thought": thought,
                    }),
                );
                continue;
            }
            let _ = app_clone.emit(
                "stream-chunk",
                serde_json::json!({
                    "session_id": session_id_clone,
                    "chunk": chunk,
                }),
            );
        }
    });

    let prizepicks_context_val =
        { prizepicks_context::build_prizepicks_context(&message).await };

    let tx_after_stream = tx.clone();
    let response = match openrouter::stream_message(
        &config,
        &session_messages,
        message.clone(),
        None,
        Some(&db_pool),
        tx,
        Some(&prizepicks_context_val),
    )
    .await
    {
        Ok(response) => response,
        Err(_error) => {
            let _ = forward_handle.await;
            return Ok(());
        }
    };

    let _ = tx_after_stream.send("__STREAM_DONE__".to_string()).await;
    let _ = forward_handle.await;

    let user_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: message,
        reasoning: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: None,
    };

    let assistant_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: response.content.clone(),
        reasoning: response.reasoning.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: response.tokens_used,
    };

    {
        let mut cs = chat_state.lock().await;
        cs.add_message(&session_id, user_msg.clone());
        cs.add_message(&session_id, assistant_msg.clone());
    }

    let all_messages = {
        let cs = chat_state.lock().await;
        cs.get_messages(&session_id)
    };
    let _ = session::save_session_messages(&session_id, &all_messages);

    {
        let t = tracker.lock().await;
        let extracted = t.extract_predictions(&session_id, &response.content);
        for pred in extracted {
            let record = PredictionRecord {
                prediction: pred,
                outcome: PredictionOutcome::Pending,
                actual_result: None,
                notes: None,
                resolved_at: None,
            };
            let _ = t.save_prediction(record).await;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn new_chat_session(
    name: Option<String>,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<session::ChatSession, String> {
    let config = state.lock().await;
    let session = session::create_session(name, &config.selected_model)?;
    Ok(session)
}

#[tauri::command]
pub async fn list_chat_sessions() -> Result<Vec<session::ChatSession>, String> {
    session::list_sessions()
}

#[tauri::command]
pub async fn delete_chat_session(session_id: String) -> Result<(), String> {
    session::delete_session(&session_id)
}

#[tauri::command]
pub async fn get_session_messages(
    session_id: String,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
) -> Result<Vec<session::ChatMessage>, String> {
    {
        let mut cs = chat_state.lock().await;
        if !cs.sessions.contains_key(&session_id) {
            cs.load_from_disk(&session_id);
        }
    }
    let cs = chat_state.lock().await;
    Ok(cs.get_messages(&session_id))
}

#[tauri::command]
pub async fn compare_models(
    message: String,
    models: Vec<String>,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    _chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
) -> Result<Vec<openrouter::OpenRouterResponse>, String> {
    let config = state.lock().await.clone();
    let session_messages: Vec<openrouter::ChatMessage> = Vec::new();
    let system_prompt = config.system_prompt.clone();

    let futures = models.into_iter().map(|model| {
        let mut model_config = config.clone();
        model_config.selected_model = model.clone();
        let session_messages = session_messages.clone();
        let message = message.clone();
        let system_prompt = system_prompt.clone();

        async move {
            let result = openrouter::send_message_with_context(
                &model_config,
                &session_messages,
                message,
                &system_prompt,
                "",
                "",
            )
            .await;

            match result {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::warn!("Model {} failed: {}", model, e);
                    openrouter::OpenRouterResponse {
                        content: format!("Error: {}", e),
                        reasoning: None,
                        tokens_used: None,
                        model: model.clone(),
                    }
                }
            }
        }
    });

    let results: Vec<openrouter::OpenRouterResponse> = futures::future::join_all(futures).await;
    Ok(results)
}
