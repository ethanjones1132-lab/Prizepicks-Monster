use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// PrizePicks Player Prop Context Builder
// Builds a data-rich context for AI chat focused on DFS player props.
// ═══════════════════════════════════════════════════════════════

/// Structured context for a single prop pick decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrizePicksPropDecision {
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub pick_type: String, // Over or Under
    pub projection: f64,
    pub win_probability_pct: f64,
    pub edge_points: f64,
    pub expected_value_pct: f64,
    pub kelly_stake_pct: f64,
    pub confidence_tier: String,
    pub thesis: String,
    pub evidence: Vec<String>,
    pub risk_flags: Vec<String>,
    pub data_quality: String,
}

/// Complete snapshot of the current PrizePicks prop environment for the AI
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrizePicksContextSnapshot {
    pub top_props: Vec<PropBrief>,
    pub trending_props: Vec<PropBrief>,
    pub league_summaries: Vec<LeagueSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropBrief {
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub stat_category: String,
    pub line: f64,
    pub projection: f64,
    pub edge_pct: f64,
    pub league: String,
    pub game_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeagueSnapshot {
    pub league: String,
    pub prop_count: usize,
    pub top_edge_pct: f64,
}

/// Build a rich PrizePicks prop context string for the AI prompt.
pub async fn build_prizepicks_context(_user_message: &str) -> String {
    let mut ctx = String::with_capacity(8192);

    ctx.push_str("# PRIZEPICKS PLAYER PROP INTELLIGENCE CONTEXT\n\n");

    // Note: actual prop data is injected by the command handler
    // This provides the structural context for the AI

    ctx.push_str("## AVAILABLE DATA SOURCES\n");
    ctx.push_str("- Player stats: season averages, last-3 splits, home/away splits\n");
    ctx.push_str("- Matchup data: defensive rankings, pace, usage rates\n");
    ctx.push_str("- Weather: game-day weather conditions for outdoor games\n");
    ctx.push_str("- Injuries: current injury reports from Sleeper API\n");
    ctx.push_str("- Live scores: real-time game data from ESPN\n\n");

    ctx.push_str("## ANALYSIS FRAMEWORK\n");
    ctx.push_str("For each prop analysis, consider:\n");
    ctx.push_str("1. Statistical baseline (season avg, last 3, splits)\n");
    ctx.push_str("2. Matchup context (defensive rank, pace, usage)\n");
    ctx.push_str("3. Situational factors (weather, injuries, game script)\n");
    ctx.push_str("4. Line value (projection vs. line = edge)\n");
    ctx.push_str("5. Risk factors (variance, injury uncertainty, game flow)\n\n");

    ctx.push_str("## DFS SAFETY REMINDERS\n");
    ctx.push_str("- This is an analysis tool. No bets are placed automatically.\n");
    ctx.push_str("- Always verify lines on prizepicks.com before playing.\n");
    ctx.push_str("- Never stake more than you can afford to lose.\n");
    ctx.push_str("- Outcomes are probabilistic, never guaranteed.\n\n");

    ctx
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_builds() {
        // Basic smoke test — context should always build without error
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = rt.block_on(async { build_prizepicks_context("test query").await });
        assert!(ctx.contains("PRIZEPICKS PLAYER PROP INTELLIGENCE CONTEXT"));
        assert!(ctx.contains("ANALYSIS FRAMEWORK"));
    }
}
