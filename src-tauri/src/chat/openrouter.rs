#![allow(dead_code)]
use crate::analysis::context::AnalysisContext;
use crate::chat::decision_schema::PrizePicksTradeDecision;
use crate::config::AppConfig;
use crate::ml_predictor;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Pool, Sqlite};
use std::time::Duration;
use tokio::sync::mpsc;

// ═══════════════════════════════════════════════════════════════
// OpenRouter API Integration — PrizePicks-First DFS Prop AI
// Supports both streaming and non-streaming modes.
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Default)]
struct OpenRouterRequestReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenRouterRequestReasoning>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl ChatMessage {
    pub fn new(role: String, content: String) -> Self {
        Self {
            role,
            content,
            reasoning: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

fn model_supports_reasoning(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("claude")
        || m.contains("deepseek-r1")
        || m.contains("/r1")
        || m.contains("qwq")
        || m.contains("qvq")
        || m.contains("thinking")
        || m.contains("/o1")
        || m.contains("/o3")
        || m.contains("/o4")
        || m.contains("gemini-2.5")
}

async fn process_stream_line(
    line: &str,
    tx: &mpsc::Sender<String>,
    full_content: &mut String,
    full_reasoning: &mut String,
    chunk_count: &mut usize,
) -> bool {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return false;
    }

    let Some(data) = line.strip_prefix("data:") else {
        return false;
    };
    let data = data.trim_start();
    if data == "[DONE]" {
        return true;
    }

    match serde_json::from_str::<StreamChunk>(data) {
        Ok(chunk) => {
            let Some(choice) = chunk.choices.first() else {
                return false;
            };

            if let Some(r_content) = &choice.delta.reasoning_content {
                full_reasoning.push_str(r_content);
                let _ = tx.send(format!("__STREAM_THOUGHT__:{}", r_content)).await;
            } else if let Some(r_content) = &choice.delta.reasoning {
                full_reasoning.push_str(r_content);
                let _ = tx.send(format!("__STREAM_THOUGHT__:{}", r_content)).await;
            }

            if let Some(content) = &choice.delta.content {
                full_content.push_str(content);
                *chunk_count += 1;
                let _ = tx.send(content.to_string()).await;
            }
        }
        Err(e) => {
            tracing::debug!("Skipping unparseable stream data line: {}", e);
        }
    }

    false
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

/// Build the core system prompt for PrizePicks-first player prop analysis.
fn build_prizepicks_system_prompt(config: &AppConfig) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("# PRIZEPICKS MONSTER — DFS PLAYER PROP INTELLIGENCE ENGINE\n\n");
    prompt.push_str(
        "You are the PrizePicks Monster, an elite AI-driven DFS player prop intelligence system. ",
    );
    prompt.push_str("Your mission is to deliver mathematically rigorous, probability-weighted Over/Under prop research ");
    prompt.push_str("for PrizePicks player props.\n\n");

    if !config.system_prompt.is_empty() {
        prompt.push_str("## USER PREFERENCES\n");
        prompt.push_str(&config.system_prompt);
        prompt.push_str("\n\n");
    }

    prompt.push_str("GUIDING PRINCIPLES:\n");
    prompt.push_str(
        "- Never describe any pick, prop, or forecast as guaranteed, certain, or risk-free. ",
    );
    prompt.push_str("Always express outcomes in calibrated probabilities, expected value (EV), and downside risk controls.\n");
    prompt.push_str("- Prioritize player prop mechanics: line value, projection confidence, variance, injury uncertainty, weather/game script, and data quality.\n");
    prompt.push_str("- Default to PASS when the edge is unclear, the spread is too wide, or data quality is poor. ");
    prompt.push_str("A clean no-pick is often the best pick.\n");
    prompt.push_str(
        "- DFS player props are the primary domain. Only discuss non-prop prediction-market context ",
    );
    prompt.push_str("Only provide sports-focused detail when the user explicitly asks for it.\n\n");

    prompt
}

/// Send a message to OpenRouter with PrizePicks-first enriched context.
/// Sports prop context is only injected if the user explicitly requests it.
pub async fn send_message(
    config: &AppConfig,
    session_messages: &[ChatMessage],
    user_message: String,
    analysis_context: Option<&AnalysisContext>,
    db_pool: Option<&Pool<Sqlite>>,
    prizepicks_context: Option<&str>,
) -> Result<OpenRouterResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    // PrizePicks-first system prompt (replaces football-specific enriched prompt)
    let system_prompt = build_prizepicks_system_prompt(config);

    // PrizePicks decision framework context (always included)
    let decision_context = build_prizepicks_decision_context_message();

    // PrizePicks prop data context (always included — the core intelligence)
    let prizepicks_data_msg = prizepicks_context.unwrap_or("");

    // Sports prop context is injected ONLY when the user explicitly asks about sports props
    let sports_data = if is_sports_prop_query(&user_message) {
        build_sports_context(&user_message, config.max_context_players).await
    } else {
        String::new()
    };

    // Construct messages array: system + PrizePicks context + (optional sports) + history + user
    let mut messages = Vec::new();

    // System prompt (highest priority for identity and rules)
    messages.push(ChatMessage::new("system".to_string(), system_prompt));

    // PrizePicks decision framework
    messages.push(ChatMessage::new("system".to_string(), decision_context));

    // PrizePicks prop data (core intelligence)
    if !prizepicks_data_msg.is_empty() {
        messages.push(ChatMessage::new(
            "system".to_string(),
            prizepicks_data_msg.to_string(),
        ));
    }

    // Sports prop data (only if user asked for sports props)
    if !sports_data.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), sports_data));
    }

    // Analysis Engine computed context (edge, matchup, scoring, correlation)
    if let Some(analysis) = analysis_context {
        let analysis_prompt = analysis.to_prompt_context();
        if !analysis_prompt.is_empty() {
            messages.push(ChatMessage::new(
                "system".to_string(),
                format!("## ANALYSIS ENGINE COMPUTED CONTEXT\n{analysis_prompt}"),
            ));
        }
    }

    // ML Model Predictions — fetch from DB and inject as system context
    if let Some(pool) = db_pool {
        let ml_preds = ml_predictor::get_stored_ml_predictions(pool, 15)
            .await
            .unwrap_or_default();
        if !ml_preds.is_empty() {
            let ml_status = ml_predictor::get_model_status(pool, None).await.ok();
            let acc_str = ml_status
                .as_ref()
                .and_then(|s| s.cv_accuracy_mean)
                .map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
            let samples_str = ml_status
                .as_ref()
                .and_then(|s| s.samples)
                .map_or("N/A".to_string(), |s| s.to_string());
            let mut ml_ctx = format!(
                "## ML MODEL PREDICTIONS (trained on {} samples, CV accuracy: {})\n\n",
                samples_str, acc_str
            );
            ml_ctx.push_str("The following are machine-learning generated predictions from your trained model.\n");
            ml_ctx.push_str("Consider these alongside your own analysis — they may confirm or challenge your lean.\n\n");
            for pred in &ml_preds {
                let emoji = if pred.ml_win_probability >= 0.55 {
                    "✅"
                } else if pred.ml_win_probability >= 0.45 {
                    "⚠️"
                } else {
                    "❌"
                };
                let lean = if pred.ml_win_probability >= 0.5 {
                    "Lean OVER"
                } else {
                    "Lean UNDER"
                };
                let line_change_str = if pred.line_change.abs() > 0.01 {
                    format!(" | Line change: {:+.1}", pred.line_change)
                } else {
                    String::new()
                };
                ml_ctx.push_str(&format!(
                    "  {} {} — {} {} | Line: {:.1} | ML Win Prob: {:.1}% ({}){}\n",
                    emoji,
                    pred.player_name,
                    pred.ml_prediction,
                    pred.stat_category,
                    pred.line,
                    pred.ml_win_probability * 100.0,
                    lean,
                    line_change_str
                ));
            }
            messages.push(ChatMessage::new("system".to_string(), ml_ctx));
        }
    }

    // Previous conversation history, trimmed to keep the prompt bounded
    let mut history = session_messages.to_vec();
    if history.len() > 20 {
        history = history.split_off(history.len() - 20);
    }
    for msg in history {
        messages.push(msg);
    }

    // Current user message
    messages.push(ChatMessage::new("user".to_string(), user_message));

    let request = ChatRequest {
        model: config.selected_model.clone(),
        messages,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stream: false,
        reasoning: if model_supports_reasoning(&config.selected_model) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let response = client
        .post(format!("{}/chat/completions", config.openrouter_base_url))
        .header(
            "Authorization",
            format!("Bearer {}", config.openrouter_api_key),
        )
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://prizepicks-monster.app")
        .header("X-Title", "PrizePicks Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let json: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let content = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or("No content in response")?
        .to_string();

    let reasoning = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("reasoning").or_else(|| m.get("reasoning_content")))
        .and_then(|r| r.as_str())
        .map(|r| r.to_string());

    let usage = json.get("usage");
    let tokens_used = usage
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_u64());

    Ok(OpenRouterResponse {
        content,
        reasoning,
        tokens_used,
        model: config.selected_model.clone(),
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenRouterResponse {
    pub content: String,
    pub reasoning: Option<String>,
    pub tokens_used: Option<u64>,
    pub model: String,
}

impl OpenRouterResponse {
    pub fn new(content: String, model: String) -> Self {
        Self {
            content,
            reasoning: None,
            tokens_used: None,
            model,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Context Builders
// ═══════════════════════════════════════════════════════════════

/// Detects if the user is asking about sports props.
fn is_sports_prop_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    let sports_keywords = [
        "sports",
        "nba",
        "nfl",
        "mlb",
        "nhl",
        "ufc",
        "golf",
        "tennis",
        "player",
        "quarterback",
        "qb",
        "running back",
        "rb",
        "wide receiver",
        "wr",
        "passing",
        "rushing",
        "receiving",
        "yards",
        "touchdown",
        "basketball",
        "baseball",
        "football",
        "hockey",
        "playoff",
        "championship",
    ];
    sports_keywords.iter().any(|kw| lower.contains(kw))
}

/// Builds sports prop context ONLY when the user explicitly requests it.
/// This replaces the old behavior where sports data was injected by default.
async fn build_sports_context(user_message: &str, max_context_players: usize) -> String {
    use crate::football::data;
    use crate::football::live_data;

    let mut ctx = String::with_capacity(4096);

    // Detect the league from the user message
    if let Some(league) = live_data::detect_league_from_query(user_message) {
        let sport_prompt = data::build_multi_sport_system_prompt(league);
        if !sport_prompt.is_empty() {
            ctx.push_str("## SPORTS PROP CONTEXT (USER REQUESTED)\n");
            ctx.push_str(&sport_prompt);
            ctx.push('\n');
        }

        // Add live data context for the detected league
        let live = live_data::build_live_data_context(user_message, max_context_players).await;
        if !live.is_empty() {
            ctx.push_str("## LIVE SPORTS DATA\n");
            ctx.push_str(&live);
            ctx.push('\n');
        }
    } else {
        // Default: provide general sports prop overview if no specific league
        let live = live_data::build_live_data_context(user_message, max_context_players).await;
        if !live.is_empty() {
            ctx.push_str("## LIVE SPORTS PROP DATA (USER REQUESTED)\n");
            ctx.push_str(&live);
            ctx.push('\n');
        }
    }

    ctx
}

/// Build the premium PrizePicks-first player-prop decision context used by the chat model.
pub fn build_prizepicks_decision_context_message() -> String {
    PrizePicksTradeDecision::prompt_schema()
}

/// Stream a message to OpenRouter with PrizePicks-first context.
/// Sports prop context is only injected when the user explicitly asks about sports props.
pub async fn stream_message(
    config: &AppConfig,
    session_messages: &[ChatMessage],
    user_message: String,
    analysis_context: Option<&AnalysisContext>,
    db_pool: Option<&Pool<Sqlite>>,
    tx: mpsc::Sender<String>,
    prizepicks_context: Option<&str>,
) -> Result<OpenRouterResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    // PrizePicks-first system prompt
    let system_prompt = build_prizepicks_system_prompt(config);

    // PrizePicks decision framework
    let decision_context = build_prizepicks_decision_context_message();

    // PrizePicks prop data
    let prizepicks_data_msg = prizepicks_context.unwrap_or("");

    // Sports prop context only if user explicitly asks
    let sports_data = if is_sports_prop_query(&user_message) {
        build_sports_context(&user_message, config.max_context_players).await
    } else {
        String::new()
    };

    // Construct messages array
    let mut messages = Vec::new();

    // System prompt
    messages.push(ChatMessage::new("system".to_string(), system_prompt));

    // PrizePicks decision framework
    messages.push(ChatMessage::new("system".to_string(), decision_context));

    // PrizePicks prop data
    if !prizepicks_data_msg.is_empty() {
        messages.push(ChatMessage::new(
            "system".to_string(),
            prizepicks_data_msg.to_string(),
        ));
    }

    // Sports prop context (only if user asked)
    if !sports_data.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), sports_data));
    }

    // Analysis Engine
    if let Some(analysis) = analysis_context {
        let analysis_prompt = analysis.to_prompt_context();
        if !analysis_prompt.is_empty() {
            messages.push(ChatMessage::new(
                "system".to_string(),
                format!("## ANALYSIS ENGINE COMPUTED CONTEXT\n{analysis_prompt}"),
            ));
        }
    }

    // ML Model Predictions
    if let Some(pool) = db_pool {
        let ml_preds = ml_predictor::get_stored_ml_predictions(pool, 15)
            .await
            .unwrap_or_default();
        if !ml_preds.is_empty() {
            let ml_status = ml_predictor::get_model_status(pool, None).await.ok();
            let acc_str = ml_status
                .as_ref()
                .and_then(|s| s.cv_accuracy_mean)
                .map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
            let samples_str = ml_status
                .as_ref()
                .and_then(|s| s.samples)
                .map_or("N/A".to_string(), |s| s.to_string());
            let mut ml_ctx = format!(
                "## ML MODEL PREDICTIONS (trained on {} samples, CV accuracy: {})\n\n",
                samples_str, acc_str
            );
            ml_ctx.push_str("The following are machine-learning generated predictions from your trained model.\n");
            ml_ctx.push_str("Consider these alongside your own analysis — they may confirm or challenge your lean.\n\n");
            for pred in &ml_preds {
                let emoji = if pred.ml_win_probability >= 0.55 {
                    "✅"
                } else if pred.ml_win_probability >= 0.45 {
                    "⚠️"
                } else {
                    "❌"
                };
                let lean = if pred.ml_win_probability >= 0.5 {
                    "Lean OVER"
                } else {
                    "Lean UNDER"
                };
                ml_ctx.push_str(&format!(
                    "  {} {} — {} {} | Line: {:.1} | ML Win Prob: {:.1}% ({})\n",
                    emoji,
                    pred.player_name,
                    pred.ml_prediction,
                    pred.stat_category,
                    pred.line,
                    pred.ml_win_probability * 100.0,
                    lean
                ));
            }
            messages.push(ChatMessage::new("system".to_string(), ml_ctx));
        }
    }

    let mut history = session_messages.to_vec();
    if history.len() > 20 {
        history = history.split_off(history.len() - 20);
    }
    for msg in history {
        messages.push(msg);
    }
    messages.push(ChatMessage::new("user".to_string(), user_message));

    let request = ChatRequest {
        model: config.selected_model.clone(),
        messages,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stream: true,
        reasoning: if model_supports_reasoning(&config.selected_model) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let response = client
        .post(format!("{}/chat/completions", config.openrouter_base_url))
        .header(
            "Authorization",
            format!("Bearer {}", config.openrouter_api_key),
        )
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://prizepicks-monster.app")
        .header("X-Title", "PrizePicks Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        let _ = tx
            .send(format!(
                "__STREAM_ERROR__:API error ({}): {}",
                status, error_body
            ))
            .await;
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let mut stream = response.bytes_stream();
    let mut full_content = String::new();
    let mut full_reasoning = String::new();
    let mut tokens_used: Option<u64> = None;
    let mut chunk_count: usize = 0;
    let mut raw_data = String::new();
    let mut line_buffer: Vec<u8> = Vec::new();
    let mut done_received = false;

    'stream_loop: while let Some(chunk_result) = stream.next().await {
        let bytes = match chunk_result {
            Ok(b) => b,
            Err(e) => {
                if !full_content.is_empty() || !full_reasoning.is_empty() {
                    tracing::warn!(
                        "Stream read error after partial content; preserving streamed response: {}",
                        e
                    );
                    break 'stream_loop;
                }
                let _ = tx
                    .send(format!("__STREAM_ERROR__:Stream error: {}", e))
                    .await;
                return Err(format!("Stream error: {}", e));
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        raw_data.push_str(&text);
        line_buffer.extend_from_slice(&bytes);

        while let Some(newline_index) = line_buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = line_buffer.drain(..=newline_index).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(&['\r', '\n'][..]);
            if process_stream_line(
                line,
                &tx,
                &mut full_content,
                &mut full_reasoning,
                &mut chunk_count,
            )
            .await
            {
                done_received = true;
                break 'stream_loop;
            }
        }
    }

    if !done_received && !line_buffer.is_empty() {
        let line = String::from_utf8_lossy(&line_buffer);
        let line = line.trim_end_matches(&['\r', '\n'][..]);
        if process_stream_line(
            line,
            &tx,
            &mut full_content,
            &mut full_reasoning,
            &mut chunk_count,
        )
        .await
        {
            // done
        }
    }

    if tokens_used.is_none() {
        tokens_used = Some((full_content.len() / 4) as u64);
    }

    let reasoning_val = if full_reasoning.is_empty() {
        None
    } else {
        Some(full_reasoning)
    };

    Ok(OpenRouterResponse {
        content: full_content,
        reasoning: reasoning_val,
        tokens_used,
        model: config.selected_model.clone(),
    })
}

/// Send a message with pre-built context strings.
/// Used by the model comparison feature.
pub async fn send_message_with_context(
    config: &AppConfig,
    session_messages: &[ChatMessage],
    user_message: String,
    system_prompt: &str,
    prizepicks_context_msg: &str,
    sports_context_msg: &str,
) -> Result<OpenRouterResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut messages = Vec::new();

    messages.push(ChatMessage::new(
        "system".to_string(),
        system_prompt.to_string(),
    ));
    messages.push(ChatMessage::new(
        "system".to_string(),
        build_prizepicks_decision_context_message(),
    ));

    if !prizepicks_context_msg.is_empty() {
        messages.push(ChatMessage::new(
            "system".to_string(),
            prizepicks_context_msg.to_string(),
        ));
    }
    if !sports_context_msg.is_empty() {
        messages.push(ChatMessage::new(
            "system".to_string(),
            sports_context_msg.to_string(),
        ));
    }

    let mut history = session_messages.to_vec();
    if history.len() > 20 {
        history = history.split_off(history.len() - 20);
    }
    for msg in history {
        messages.push(msg);
    }

    messages.push(ChatMessage::new("user".to_string(), user_message));

    let request = ChatRequest {
        model: config.selected_model.clone(),
        messages,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stream: false,
        reasoning: if model_supports_reasoning(&config.selected_model) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let response = client
        .post(format!("{}/chat/completions", config.openrouter_base_url))
        .header(
            "Authorization",
            format!("Bearer {}", config.openrouter_api_key),
        )
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://prizepicks-monster.app")
        .header("X-Title", "PrizePicks Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let json: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let content = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or("No content in response")?
        .to_string();

    let usage = json.get("usage");
    let tokens_used = usage
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_u64());

    Ok(OpenRouterResponse {
        content,
        reasoning: None,
        tokens_used,
        model: config.selected_model.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_is_prizepicks_prop_first() {
        let prompt = build_prizepicks_system_prompt(&AppConfig::default());

        assert!(prompt.contains("DFS PLAYER PROP INTELLIGENCE ENGINE"));
        assert!(prompt.contains("Over/Under prop research"));
        assert!(!prompt.contains("PREDICTION MARKET INTELLIGENCE ENGINE"));
        assert!(!prompt.contains("event contracts"));
        assert!(!prompt.contains("wager, contract"));
        assert!(!prompt.contains("bid-ask spreads"));
        assert!(!prompt.contains("market microstructure"));
        assert!(!prompt.contains("Sports analysis is a subdomain"));
    }

    #[test]
    fn test_decision_context_uses_shared_prop_schema() {
        let prompt = build_prizepicks_decision_context_message();

        assert_eq!(prompt, PrizePicksTradeDecision::prompt_schema());
        assert!(prompt.contains("PRIZEPICKS PLAYER PROP DECISION SCHEMA"));
        assert!(!prompt.contains("KXEVENT"));
        assert!(!prompt.contains("Prediction Market Intelligence Framework"));
        assert!(!prompt.contains("orderbook"));
    }

    #[test]
    fn test_sports_prop_query_detection() {
        assert!(is_sports_prop_query("Analyze NBA player props"));
        assert!(!is_sports_prop_query("Federal Reserve decision analysis"));
    }
}
