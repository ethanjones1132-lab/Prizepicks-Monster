use crate::analysis::{context, edge_calculator, prop_scorer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EdgeAnalysisInput {
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub pick_type: String,
    pub projection: f64,
    pub season_avg: f64,
    pub last3_avg: f64,
    pub home_avg: Option<f64>,
    pub away_avg: Option<f64>,
    pub is_home: bool,
    pub defense_rank: Option<u32>,
    pub pace_rank: Option<u32>,
    pub usage_rate: Option<f64>,
    pub opponent_pace_rank: Option<u32>,
    pub park_factor: Option<f64>,
    pub goalie_quality_rank: Option<u32>,
    pub consistency_score: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PropAnalysisResult {
    pub edge: edge_calculator::EdgeScore,
    pub scored: prop_scorer::ScoredProp,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParlayLegInput {
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: Option<f64>,
    pub confidence_score: Option<u8>,
}

fn to_analysis_input(input: &EdgeAnalysisInput) -> context::AnalysisInput {
    context::AnalysisInput {
        player_name: input.player_name.clone(),
        stat_category: input.stat_category.clone(),
        line: input.line,
        pick_type: input.pick_type.clone(),
        projection: input.projection,
        season_avg: input.season_avg,
        last3_avg: input.last3_avg,
        home_avg: input.home_avg,
        away_avg: input.away_avg,
        is_home: input.is_home,
        defense_rank: input.defense_rank,
        pace_rank: input.pace_rank,
        usage_rate: input.usage_rate,
        opponent_pace_rank: input.opponent_pace_rank,
        park_factor: input.park_factor,
        goalie_quality_rank: input.goalie_quality_rank,
        consistency_score: input.consistency_score,
    }
}

#[tauri::command]
pub async fn analyze_prop(input: EdgeAnalysisInput) -> Result<PropAnalysisResult, String> {
    let analysis_input = to_analysis_input(&input);
    let (edge, scored) = context::analyze_single_prop(&analysis_input);
    Ok(PropAnalysisResult { edge, scored })
}

#[tauri::command]
pub async fn analyze_multiple_props(
    inputs: Vec<EdgeAnalysisInput>,
) -> Result<context::AnalysisContext, String> {
    let analysis_inputs: Vec<context::AnalysisInput> = inputs.iter().map(to_analysis_input).collect();
    Ok(context::analyze_multiple_props(&analysis_inputs))
}

#[tauri::command]
pub async fn analyze_parlay_correlation(
    legs: Vec<ParlayLegInput>,
) -> Result<crate::analysis::parlay_correlation::ParlayAnalysis, String> {
    let picks: Vec<crate::correlation::CorrelationPick> = legs
        .into_iter()
        .map(|leg| crate::correlation::CorrelationPick {
            player_name: leg.player_name,
            team: leg.team,
            opponent: leg.opponent,
            prop_category: leg.prop_category,
            line: leg.line,
            pick_type: leg.pick_type,
            win_probability: leg.win_probability,
            confidence_score: leg.confidence_score,
        })
        .collect();
    Ok(context::analyze_parlay(&picks))
}

#[tauri::command]
pub async fn generate_analysis_context(inputs: Vec<EdgeAnalysisInput>) -> Result<String, String> {
    let analysis_inputs: Vec<context::AnalysisInput> = inputs.iter().map(to_analysis_input).collect();
    let ctx = context::analyze_multiple_props(&analysis_inputs);
    Ok(ctx.to_prompt_context())
}

#[tauri::command]
pub async fn get_scored_props_by_tier(
    inputs: Vec<EdgeAnalysisInput>,
    min_tier: String,
) -> Result<Vec<prop_scorer::ScoredProp>, String> {
    let analysis_inputs: Vec<context::AnalysisInput> = inputs.iter().map(to_analysis_input).collect();
    let ctx = context::analyze_multiple_props(&analysis_inputs);

    let min_tier_enum = match min_tier.as_str() {
        "Elite" => prop_scorer::PropTier::Elite,
        "Strong" => prop_scorer::PropTier::Strong,
        "Playable" => prop_scorer::PropTier::Playable,
        "Marginal" => prop_scorer::PropTier::Marginal,
        _ => prop_scorer::PropTier::Avoid,
    };

    let filtered: Vec<prop_scorer::ScoredProp> = ctx
        .scored_props
        .into_iter()
        .filter(|p| {
            let score = p.composite_score;
            match min_tier_enum {
                prop_scorer::PropTier::Elite => score >= 80.0,
                prop_scorer::PropTier::Strong => score >= 65.0,
                prop_scorer::PropTier::Playable => score >= 50.0,
                prop_scorer::PropTier::Marginal => score >= 35.0,
                prop_scorer::PropTier::Avoid => true,
            }
        })
        .collect();

    Ok(filtered)
}
