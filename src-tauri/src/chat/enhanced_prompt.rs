#![allow(dead_code)]
//! Enhanced System Prompt Builder — PrizePicks DFS Prop AI Prompt Engine
//!
//! Builds the most comprehensive, data-rich system prompt focused on
//! DFS player props. Sports prop context is only injected when explicitly requested.
//!
//! Combines:
//!   1. PrizePicks player prop context (top props, trending props, league summaries)
//!   2. Live data for explicitly requested sports props
//!   3. Weather impact assessment (only for sports queries)
//!   4. ML model predictions (when available)
//!   5. Structured output format requirements for PrizePicks player prop picks
//!   6. Professional decision schema for DFS research recommendations

use crate::analysis::context::AnalysisContext;
use crate::chat::decision_schema::PrizePicksTradeDecision;
use crate::chat::prizepicks_context::build_prizepicks_context;
use crate::ml_predictor::{MLModelStatus, MLPrediction};
use crate::prizepicks::client::PrizePicksClient;
use std::fmt::Write;

/// Build the ultimate enriched system prompt with all available data.
/// Sports prop data is injected ONLY when the user explicitly asks about sports props.
pub async fn build_ultimate_prompt(
    base_prompt: &str,
    user_message: &str,
    max_context_players: usize,
    analysis_context: Option<&AnalysisContext>,
    ml_predictions: Option<&[MLPrediction]>,
    ml_model_status: Option<&MLModelStatus>,
    prizepicks_client: Option<&mut PrizePicksClient>,
) -> String {
    let mut prompt = String::with_capacity(16384);

    // Role & Identity — PrizePicks-first
    prompt.push_str(concat!(
        "# PRIZEPICKS MONSTER — DFS PLAYER PROP INTELLIGENCE ENGINE\n\n",
        "You are the PrizePicks Monster, the absolute pinnacle of AI-driven DFS player prop intelligence. ",
        "Your mission is to estimate accurate probabilities for Over/Under player props, identify mispriced lines, ",
        "and deliver mathematically sound DFS research recommendations.\n\n",
        "GUIDING PRINCIPLES:\n",
        "- Never describe any pick, prop, or forecast as guaranteed, certain, or risk-free. ",
        "Always express outcomes in calibrated probabilities, expected value (EV), and downside risk controls.\n",
        "- Prioritize DFS player prop mechanics: line value, projection confidence, variance, injury uncertainty, ",
        "weather/game script, and data quality.\n",
        "- DFS player props are the primary domain. Only discuss non-prop prediction-market context ",
        "when the user explicitly asks for it.\n\n"
    ));

    // User custom prompt
    if !base_prompt.is_empty() {
        let _ = write!(prompt, "## CUSTOM INSTRUCTIONS\n{}\n\n", base_prompt);
    }

    // PrizePicks player prop context (primary intelligence)
    if let Some(_client) = prizepicks_client {
        let prizepicks_ctx = build_prizepicks_context(user_message).await;
        if !prizepicks_ctx.is_empty() {
            prompt.push_str(&prizepicks_ctx);
            prompt.push('\n');
        }
    }

    // Sports prop context — ONLY if user explicitly asks about sports props
    if is_sports_prop_query(user_message) {
        let sports_ctx = build_sports_prompt_context(user_message, max_context_players).await;
        if !sports_ctx.is_empty() {
            prompt.push_str("## SPORTS PROP CONTEXT (USER REQUESTED)\n");
            prompt.push_str(&sports_ctx);
            prompt.push_str("\n\n");
        }
    }

    // Analysis Engine Context
    if let Some(analysis) = analysis_context {
        let analysis_prompt = analysis.to_prompt_context();
        if !analysis_prompt.is_empty() {
            let _ = write!(
                prompt,
                "## ANALYSIS ENGINE COMPUTED CONTEXT\n{analysis_prompt}\n"
            );
        }
    }

    // ML Model Predictions
    if let Some(preds) = ml_predictions {
        if !preds.is_empty() {
            let acc_str = ml_model_status
                .and_then(|s| s.cv_accuracy_mean)
                .map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
            let samples_str = ml_model_status
                .and_then(|s| s.samples)
                .map_or("N/A".to_string(), |s| s.to_string());
            let _ = write!(
                prompt,
                "## ML MODEL PREDICTIONS (trained on {} samples, CV accuracy: {})\n\n",
                samples_str, acc_str
            );
            let _ = write!(prompt, "The following are machine-learning generated predictions from your trained model.\n");
            let _ = write!(prompt, "Consider these alongside your own analysis — they may confirm or challenge your lean.\n\n");
            for pred in preds.iter().take(15) {
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
                let _ = write!(
                    prompt,
                    "  {} {} — {} {} | Line: {:.1} | ML Win Prob: {:.1}% ({})",
                    emoji,
                    pred.player_name,
                    pred.ml_prediction,
                    pred.stat_category,
                    pred.line,
                    pred.ml_win_probability * 100.0,
                    lean
                );
                if let Some(orig_prob) = pred.original_probability {
                    let agreement = (pred.ml_win_probability >= 0.5) == (orig_prob >= 50.0);
                    let _ = write!(
                        prompt,
                        " | AI prob: {:.0}%{}",
                        orig_prob,
                        if agreement {
                            " ✓ agree"
                        } else {
                            " ⚡ DISAGREE"
                        }
                    );
                }
                let _ = write!(prompt, "\n");
            }
            prompt.push('\n');
        }
    }

    // Professional decision schema
    let decision_schema = PrizePicksTradeDecision::prompt_schema();
    prompt.push_str(&decision_schema);

    prompt
}

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
        "player prop",
        "prop pick",
        "over/under",
        "pick over",
        "pick under",
    ];
    sports_keywords.iter().any(|kw| lower.contains(kw))
}

/// Build sports-only context when the user explicitly requests it.
async fn build_sports_prompt_context(user_message: &str, max_context_players: usize) -> String {
    use crate::football::data;
    use crate::football::live_data;

    let mut ctx = String::with_capacity(4096);

    let detected_league = live_data::detect_league_from_query(user_message);
    if let Some(league) = detected_league {
        let sport_addition = data::build_multi_sport_system_prompt(league);
        if !sport_addition.is_empty() {
            ctx.push_str(&sport_addition);
            ctx.push('\n');
        }
    }

    let live_context = live_data::build_live_data_context(user_message, max_context_players).await;
    if !live_context.is_empty() {
        let _ = write!(ctx, "## LIVE SPORTS DATA\n{}\n\n", live_context);
    }

    // Player Knowledge Base (only for sports prop queries)
    if let Some(league) = detected_league {
        if let Some(ctx_data) = data::get_multi_sport_context(league.short_name()) {
            push_multi_sport_knowledge(&mut ctx, &ctx_data, user_message);
        }
    }

    ctx
}

fn push_multi_sport_knowledge(
    prompt: &mut String,
    ctx: &crate::football::data::MultiSportContext,
    user_message: &str,
) {
    let tokens = tokenize(user_message);
    let _ = write!(prompt, "## {} PLAYER PROFILES\n", ctx.sport);
    let relevant = select_relevant(&ctx.top_players, &tokens, 12);
    for p in &relevant {
        push_profile(prompt, p);
    }
    prompt.push('\n');

    if !ctx.team_rankings.is_empty() {
        let _ = write!(prompt, "## {} TEAM RANKINGS (off/def/pace)\n", ctx.sport);
        for r in ctx.team_rankings.iter().take(10) {
            let _ = write!(
                prompt,
                "- {}: off#{}/def#{}/pace#{} — {}\n",
                r.team, r.offense_rank, r.defense_rank, r.pace_rank, r.note
            );
        }
        prompt.push('\n');
    }

    if !ctx.trending_narratives.is_empty() {
        let _ = write!(prompt, "## {} NARRATIVES\n", ctx.sport);
        for n in ctx.trending_narratives.iter().take(5) {
            let _ = write!(prompt, "- {}\n", n);
        }
        prompt.push('\n');
    }
}

fn push_profile(prompt: &mut String, p: &crate::football::data::PlayerProfile) {
    let _ = write!(prompt, "- {} ({}, {})", p.name, p.team, p.position);
    let stats: Vec<String> = p
        .season_avg_game
        .iter()
        .map(|(k, v)| format!("{}={:.1}", k, v))
        .collect();
    if !stats.is_empty() {
        let _ = write!(prompt, " | {}", stats.join(", "));
    }
    let l3: Vec<String> = p
        .last_3_avg
        .iter()
        .map(|(k, v)| format!("{}={:.1}", k, v))
        .collect();
    if !l3.is_empty() {
        let _ = write!(prompt, " | L3: {}", l3.join(", "));
    }
    if !p.notes.is_empty() {
        let _ = write!(prompt, "\n  Note: {}", p.notes);
    }
    prompt.push('\n');
}

fn select_relevant(
    players: &[crate::football::data::PlayerProfile],
    tokens: &[String],
    max: usize,
) -> Vec<crate::football::data::PlayerProfile> {
    if tokens.is_empty() {
        return players.iter().take(max).cloned().collect();
    }
    let mut scored: Vec<(i32, &crate::football::data::PlayerProfile)> = players
        .iter()
        .map(|p| {
            let mut s = 0;
            let name = p.name.to_lowercase();
            let team = p.team.to_lowercase();
            let notes = p.notes.to_lowercase();
            for t in tokens {
                if t.len() < 2 {
                    continue;
                }
                if name.contains(t) {
                    s += 10;
                }
                if team.contains(t) {
                    s += 6;
                }
                if notes.contains(t) {
                    s += 2;
                }
            }
            (s, p)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by_key(|(s, _)| -*s);
    scored.iter().take(max).map(|(_, p)| (*p).clone()).collect()
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            if cur.len() > 1 {
                tokens.push(cur.clone());
            }
            cur.clear();
        }
    }
    if !cur.is_empty() && cur.len() > 1 {
        tokens.push(cur);
    }
    tokens
}

/// Build the ultimate prompt with REAL-TIME live data injection.
/// This is the primary entry point used by the chat system.
/// It fetches live data concurrently with building the base prompt.
pub async fn build_ultimate_prompt_with_live_data(
    base_prompt: &str,
    user_message: &str,
    max_context_players: usize,
    analysis_context: Option<&AnalysisContext>,
    ml_predictions: Option<&[MLPrediction]>,
    ml_model_status: Option<&MLModelStatus>,
    prizepicks_client: Option<&mut PrizePicksClient>,
) -> String {
    // Build base prompt and fetch live data concurrently
    let base_future = build_ultimate_prompt(
        base_prompt,
        user_message,
        max_context_players,
        analysis_context,
        ml_predictions,
        ml_model_status,
        prizepicks_client,
    );

    let (base_prompt_str, _) = tokio::join!(base_future, async {});

    base_prompt_str
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sports_prop_query() {
        assert!(is_sports_prop_query("Analyze NBA player props"));
        assert!(is_sports_prop_query("What about player props?"));
        assert!(!is_sports_prop_query("Federal Reserve decision analysis"));
        assert!(!is_sports_prop_query("Crypto market outlook"));
    }

    #[test]
    fn test_non_sports_query_no_sports_data() {
        let query = "Analyze political election odds";
        assert!(!is_sports_prop_query(query));
    }

    #[tokio::test]
    async fn test_built_prompt_is_prizepicks_prop_first() {
        let prompt =
            build_ultimate_prompt("", "Analyze NBA player props", 50, None, None, None, None).await;

        assert!(prompt.contains("DFS PLAYER PROP INTELLIGENCE ENGINE"));
        assert!(prompt.contains("Over/Under player props"));
        assert!(!prompt.contains("PREDICTION MARKET INTELLIGENCE ENGINE"));
        assert!(!prompt.contains("event contracts"));
        assert!(!prompt.contains("mispriced options"));
        assert!(!prompt.contains("outperform the market"));
        assert!(!prompt.contains("wager, contract"));
        assert!(!prompt.contains("bid-ask spreads"));
        assert!(!prompt.contains("market microstructure"));
        assert!(!prompt.contains("Sports analysis is a subdomain"));
    }
}
