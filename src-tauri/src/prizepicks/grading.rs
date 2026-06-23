//! PrizePicks grading engine — Over/Under player prop grading with multiplier-based PnL.
//!
//! Two grading paths:
//! 1. **Over/Under (player props)**: Compares actual_stat_value vs line, uses multiplier tables.
//! 2. **Binary contract (legacy)**: Yes/No market outcome, binary PnL (kept for compatibility).
//!
//! PrizePicks payout multipliers (Power Play — all picks must be correct):
//!   6-pick: 37.5x  |  5-pick: 20x  |  4-pick: 10x  |  3-pick: 6x  |  2-pick: 3x
//!
//! Flex Play (can still win with 1-2 incorrect):
//!   6 of 6: 25x  |  5 of 6: 2x  |  4 of 6: 0.4x
//!   5 of 5: 10x  |  4 of 5: 2x  |  3 of 5: 0.4x
//!   4 of 4: 6x   |  3 of 4: 1.5x
//!   3 of 3: 3x   |  2 of 3: 1x

use super::models::PrizePicksPrediction;
use crate::predictions::tracker::PredictionTracker;

// ═══════════════════════════════════════════════════════════════
// Over/Under Prop Grading
// ═══════════════════════════════════════════════════════════════

/// Result of grading a single Over/Under prop pick
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropGrade {
    Win,
    Loss,
    Push, // Exact match on the line (tie)
    DNP,  // Player did not play
}

/// Evaluation of a single Over/Under prop bet
#[derive(Debug, Clone)]
pub struct PropBetEvaluation {
    pub grade: PropGrade,
    pub pnl: f64,
    pub line: f64,
    pub actual_value: Option<f64>,
    pub pick_type: String, // "Over" or "Under"
    pub multiplier: f64,
}

/// Determine Over/Under grade by comparing actual stat value vs prop line.
///
/// - Over wins if actual > line
/// - Under wins if actual < line
/// - Exact match = Push (tie)
/// - DNP = player didn't play (handled separately)
fn is_dnp_outcome(outcome: &str) -> bool {
    matches!(
        outcome.trim().to_uppercase().as_str(),
        "DNP" | "DID NOT PLAY" | "DID NOT PARTICIPATE"
    )
}

fn prop_prediction_has_gradeable_result(pred: &PrizePicksPrediction) -> bool {
    pred.actual_stat_value.is_some() || pred.actual_outcome.as_deref().is_some_and(is_dnp_outcome)
}

fn is_supported_prop_pick_type(pick_type: &str) -> bool {
    matches!(
        pick_type.trim().to_lowercase().as_str(),
        "over" | "more" | "o" | "under" | "less" | "u"
    )
}

pub fn grade_over_under(pick_type: &str, line: f64, actual_value: f64) -> PropGrade {
    let diff = actual_value - line;
    // Use a small epsilon for floating-point comparison
    if diff.abs() < 0.001 {
        return PropGrade::Push;
    }
    match pick_type.to_lowercase().as_str() {
        "over" | "more" | "o" => {
            if actual_value > line {
                PropGrade::Win
            } else {
                PropGrade::Loss
            }
        }
        "under" | "less" | "u" => {
            if actual_value < line {
                PropGrade::Win
            } else {
                PropGrade::Loss
            }
        }
        _ => PropGrade::Push, // Direct comparison fallback; evaluate_prop_bet rejects unsupported pick types before grading.
    }
}

/// Calculate PnL for a single Over/Under prop pick using multiplier.
///
/// Win:  stake * (multiplier - 1)  [net profit, not including stake return]
/// Loss: -stake
/// Push: 0 (stake returned, no profit/loss)
/// DNP:  0 (stake returned)
pub fn prop_pnl(stake: f64, grade: &PropGrade, multiplier: f64) -> f64 {
    match grade {
        PropGrade::Win => stake * (multiplier - 1.0),
        PropGrade::Loss => -stake,
        PropGrade::Push | PropGrade::DNP => 0.0,
    }
}

/// Evaluate a single Over/Under prop prediction.
///
/// Returns `None` if the prediction cannot be graded (missing data).
pub fn evaluate_prop_bet(pred: &PrizePicksPrediction) -> Option<PropBetEvaluation> {
    let pick_type = pred.pick_type.as_deref().unwrap_or("Unknown");
    let line = pred.line.unwrap_or(0.0);
    let multiplier = pred.multiplier.unwrap_or(3.0); // Default to 2-pick Power Play

    if pred.actual_outcome.as_deref().is_some_and(is_dnp_outcome) {
        return Some(PropBetEvaluation {
            grade: PropGrade::DNP,
            pnl: 0.0,
            line,
            actual_value: None,
            pick_type: pick_type.to_string(),
            multiplier,
        });
    }

    let actual_value = pred.actual_stat_value?;
    if !is_supported_prop_pick_type(pick_type) {
        return None;
    }

    let stake = pred.stake_amount;
    let grade = grade_over_under(pick_type, line, actual_value);
    let pnl = prop_pnl(stake, &grade, multiplier);

    Some(PropBetEvaluation {
        grade: grade.clone(),
        pnl,
        line,
        actual_value: Some(actual_value),
        pick_type: pick_type.to_string(),
        multiplier,
    })
}

// ═══════════════════════════════════════════════════════════════
// Multiplier Tables (PrizePicks standard payouts)
// ═══════════════════════════════════════════════════════════════

/// Power Play multipliers — all picks must be correct to win.
/// PrizePicks standard: 6-pick=37.5x, 5-pick=20x, 4-pick=10x, 3-pick=6x, 2-pick=3x
pub fn power_play_multiplier(num_picks: usize) -> f64 {
    match num_picks {
        6 => 37.5,
        5 => 20.0,
        4 => 10.0,
        3 => 6.0,
        2 => 3.0,
        _ => 3.0, // Default to 2-pick rate
    }
}

/// Flex Play multipliers — can still win with 1-2 incorrect picks.
/// Returns (correct_picks, total_picks) → multiplier.
/// Non-listed Flex outcomes have no payout.
pub fn flex_play_multiplier(correct: usize, total: usize) -> f64 {
    match (correct, total) {
        (6, 6) => 25.0,
        (5, 6) => 2.0,
        (4, 6) => 0.4,
        (5, 5) => 10.0,
        (4, 5) => 2.0,
        (3, 5) => 0.4,
        (4, 4) => 6.0,
        (3, 4) => 1.5,
        (3, 3) => 3.0,
        (2, 3) => 1.0,
        // Reverted lineup (DNP reduces pick count) — use Power Play rate for reduced count.
        (c, t) if c == t && c >= 2 => power_play_multiplier(c),
        _ => 0.0, // No payout
    }
}

/// Calculate lineup PnL for a set of prop picks.
///
/// For Power Play: all must be correct. If any loss, entire lineup loses.
/// For Flex Play: partial wins allowed per flex_play_multiplier table.
///
/// Returns (total_pnl, wins, losses, pushes, dnps)
pub fn calculate_lineup_pnl(
    picks: &[&PrizePicksPrediction],
    is_flex: bool,
) -> (f64, usize, usize, usize, usize) {
    let num_picks = picks.len();
    if num_picks == 0 || num_picks < 2 {
        return (0.0, 0, 0, 0, 0);
    }

    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut pushes = 0usize;
    let mut dnps = 0usize;
    let mut pending = 0usize;
    let mut total_stake = 0.0;

    for pick in picks {
        let stake = pick.stake_amount;
        total_stake += stake;

        if let Some(eval) = evaluate_prop_bet(pick) {
            match eval.grade {
                PropGrade::Win => wins += 1,
                PropGrade::Loss => losses += 1,
                PropGrade::Push => pushes += 1,
                PropGrade::DNP => dnps += 1,
            }
        } else {
            // Can't grade yet — do not apply lineup payout until all non-DNP legs have results.
            pending += 1;
        }
    }

    let graded = wins + losses + pushes + dnps;
    if pending > 0 || graded == 0 || wins + losses + pushes == 0 {
        return (0.0, wins, losses, pushes, dnps);
    }

    let total_pnl = if is_flex {
        // Flex Play: DNP legs revert the lineup; wins and pushes count as correct.
        let correct = wins + pushes;
        let effective_total = num_picks.saturating_sub(dnps);
        if effective_total < 2 {
            0.0
        } else {
            let multiplier = flex_play_multiplier(correct, effective_total);
            if multiplier > 0.0 {
                total_stake * (multiplier - 1.0)
            } else {
                -total_stake
            }
        }
    } else {
        // Power Play: all must win; pushes and DNPs reduce the effective pick count.
        if losses > 0 {
            -total_stake
        } else {
            let effective_picks = num_picks.saturating_sub(pushes).saturating_sub(dnps);
            if effective_picks < 2 {
                // Reverted below 2-pick minimum = refund
                0.0
            } else {
                let multiplier = power_play_multiplier(effective_picks);
                total_stake * (multiplier - 1.0)
            }
        }
    };

    (total_pnl, wins, losses, pushes, dnps)
}

// ═══════════════════════════════════════════════════════════════
// Batch Grading — Grade all pending predictions
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct PropGradingResult {
    pub prediction_id: String,
    pub player_name: String,
    pub stat_category: String,
    pub pick_type: String,
    pub line: f64,
    pub actual_value: Option<f64>,
    pub grade: String, // "Win" | "Loss" | "Push" | "DNP"
    pub pnl: f64,
    pub stake_amount: f64,
    pub multiplier: f64,
    pub resolved_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct PropGradingSummary {
    pub total_predictions: u32,
    pub gradable: u32,
    pub graded: u32,
    pub wins: u32,
    pub losses: u32,
    pub pushes: u32,
    pub dnps: u32,
    pub total_pnl: f64,
    pub results: Vec<PropGradingResult>,
    pub fetched_at: String,
}

/// Grade all pending Over/Under predictions that have actual_stat_value set.
///
/// This is the main entry point for the grading engine. It:
/// 1. Fetches all pending predictions from the tracker
/// 2. Filters to those with actual_stat_value or explicit DNP outcome
/// 3. Grades each one using grade_over_under or DNP handling
/// 4. Updates the tracker with results
/// 5. Returns a summary
pub async fn grade_pending_prop_predictions(
    tracker: &PredictionTracker,
) -> Result<PropGradingSummary, String> {
    let pending: Vec<PrizePicksPrediction> = tracker
        .get_prizepicks_predictions()
        .await
        .into_iter()
        .filter(|p| {
            // Grade predictions that have actual_stat_value or an explicit DNP outcome,
            // but haven't been graded yet.
            prop_prediction_has_gradeable_result(p)
                && p.pnl.is_none()
                && p.pick_type.is_some()
                && p.line.is_some()
        })
        .collect();

    if pending.is_empty() {
        return Ok(PropGradingSummary {
            fetched_at: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        });
    }

    let mut results = Vec::new();
    let mut wins = 0u32;
    let mut losses = 0u32;
    let mut pushes = 0u32;
    let mut dnps = 0u32;
    let mut total_pnl = 0.0;

    let resolved_at = chrono::Utc::now().to_rfc3339();

    for pred in &pending {
        let Some(eval) = evaluate_prop_bet(pred) else {
            continue;
        };

        let grade_str = match &eval.grade {
            PropGrade::Win => {
                wins += 1;
                "Win"
            }
            PropGrade::Loss => {
                losses += 1;
                "Loss"
            }
            PropGrade::Push => {
                pushes += 1;
                "Push"
            }
            PropGrade::DNP => {
                dnps += 1;
                "DNP"
            }
        };

        total_pnl += eval.pnl;

        // Update the tracker with the grade result
        tracker
            .update_prizepicks_outcome(&pred.id, grade_str, eval.pnl)
            .await?;

        results.push(PropGradingResult {
            prediction_id: pred.id.clone(),
            player_name: pred.title.clone(), // title contains player name for prop picks
            stat_category: pred.category.clone(),
            pick_type: eval.pick_type,
            line: eval.line,
            actual_value: eval.actual_value,
            grade: grade_str.to_string(),
            pnl: eval.pnl,
            stake_amount: pred.stake_amount,
            multiplier: eval.multiplier,
            resolved_at: resolved_at.clone(),
        });
    }

    Ok(PropGradingSummary {
        total_predictions: pending.len() as u32,
        gradable: pending.len() as u32,
        graded: results.len() as u32,
        wins,
        losses,
        pushes,
        dnps,
        total_pnl,
        results,
        fetched_at: resolved_at,
    })
}

// ═══════════════════════════════════════════════════════════════
// Binary Contract Grading (legacy — kept for compatibility)
// ═══════════════════════════════════════════════════════════════

use super::market_data_provider::MarketDataProvider;
use crate::prizepicks::models::{
    parse_bet_side, PrizePicksBetSide, PrizePicksGradingResult, PrizePicksGradingSummary,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PrizePicksBetEvaluation {
    pub side: PrizePicksBetSide,
    pub won: bool,
    pub pnl: f64,
    pub entry_price: f64,
    pub market_price_at_entry_pct: Option<f64>,
}

pub fn infer_market_price_at_entry(
    stored_market_price: Option<f64>,
    price_to_enter: Option<f64>,
    contract_side: Option<&str>,
) -> Option<f64> {
    if let Some(m) = stored_market_price {
        return Some(m);
    }
    let entry = price_to_enter?;
    let entry_dec = if entry > 0.0 && entry < 1.0 {
        entry
    } else if entry > 1.0 && entry <= 100.0 {
        entry / 100.0
    } else {
        return None;
    };
    match parse_bet_side(contract_side, None) {
        PrizePicksBetSide::Yes => Some(entry_dec * 100.0),
        PrizePicksBetSide::No => Some((1.0 - entry_dec) * 100.0),
        PrizePicksBetSide::Over | PrizePicksBetSide::Under => Some(entry_dec * 100.0),
        _ => None,
    }
}

pub fn market_price_at_entry_pct(pred: &PrizePicksPrediction) -> Option<f64> {
    infer_market_price_at_entry(
        pred.market_price_at_entry,
        pred.price_to_enter,
        pred.contract_side.as_deref(),
    )
}

fn entry_price_decimal(pred: &PrizePicksPrediction, side: PrizePicksBetSide) -> f64 {
    if let Some(p) = pred.price_to_enter {
        if p > 0.0 && p < 1.0 {
            return p;
        }
        if p > 1.0 && p <= 100.0 {
            return p / 100.0;
        }
    }
    if let Some(m) = pred.market_price_at_entry {
        let yes = m / 100.0;
        return match side {
            PrizePicksBetSide::Yes => yes,
            PrizePicksBetSide::No => 1.0 - yes,
            PrizePicksBetSide::Over | PrizePicksBetSide::Under => yes,
            _ => 0.5,
        };
    }
    0.5
}

pub fn bet_won(side: PrizePicksBetSide, actual_outcome: &str) -> Option<bool> {
    let actual = actual_outcome.trim().to_uppercase();
    match side {
        PrizePicksBetSide::Yes => Some(matches!(actual.as_str(), "YES" | "Y")),
        PrizePicksBetSide::No => Some(matches!(actual.as_str(), "NO" | "N")),
        PrizePicksBetSide::Over => Some(matches!(actual.as_str(), "OVER" | "O" | "MORE")),
        PrizePicksBetSide::Under => Some(matches!(actual.as_str(), "UNDER" | "U" | "LESS")),
        PrizePicksBetSide::Pass | PrizePicksBetSide::Unknown => None,
    }
}

pub fn contract_pnl(stake: f64, entry_price: f64, won: bool) -> f64 {
    if stake <= 0.0 {
        return 0.0;
    }
    if !won {
        return -stake;
    }
    let p = entry_price.clamp(0.01, 0.99);
    (stake / p) - stake
}

pub fn evaluate_bet(
    pred: &PrizePicksPrediction,
    actual_outcome: &str,
) -> Option<PrizePicksBetEvaluation> {
    let side = parse_bet_side(pred.contract_side.as_deref(), pred.pick_type.as_deref());
    let won = bet_won(side, actual_outcome)?;
    let entry_price = entry_price_decimal(pred, side);
    let pnl = contract_pnl(pred.stake_amount, entry_price, won);
    Some(PrizePicksBetEvaluation {
        side,
        won,
        pnl,
        entry_price,
        market_price_at_entry_pct: market_price_at_entry_pct(pred),
    })
}

/// Grade pending binary contract predictions (legacy path).
/// This uses the trading API market result (Yes/No) for grading.
pub async fn grade_pending_predictions(
    tracker: &PredictionTracker,
    client: &dyn MarketDataProvider,
) -> Result<PrizePicksGradingSummary, String> {
    let pending: Vec<PrizePicksPrediction> = tracker
        .get_prizepicks_predictions()
        .await
        .into_iter()
        .filter(|p| p.actual_outcome.is_none())
        .collect();

    if pending.is_empty() {
        return Ok(empty_binary_summary());
    }

    let mut by_ticker: HashMap<String, Vec<&PrizePicksPrediction>> = HashMap::new();
    for pred in &pending {
        by_ticker.entry(pred.ticker.clone()).or_default().push(pred);
    }

    let mut results = Vec::new();
    let mut wins = 0u32;
    let mut losses = 0u32;
    let mut total_pnl = 0.0;

    for (ticker, preds) in by_ticker {
        let market = match client.get_market(&ticker).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("prizepicks grade: skip {} — {}", ticker, e);
                continue;
            }
        };
        if market.result.is_empty() {
            continue;
        }

        let actual = market.result.clone();
        let resolved_at = chrono::Utc::now().to_rfc3339();

        for pred in preds {
            let Some(eval) = evaluate_bet(pred, &actual) else {
                continue;
            };
            if eval.won {
                wins += 1;
            } else {
                losses += 1;
            }
            total_pnl += eval.pnl;
            tracker
                .update_prizepicks_outcome(&pred.id, &actual, eval.pnl)
                .await?;
            results.push(PrizePicksGradingResult {
                prediction_id: pred.id.clone(),
                ticker: pred.ticker.clone(),
                title: pred.title.clone(),
                category: pred.category.clone(),
                predicted_probability: pred.predicted_probability,
                actual_outcome: actual.clone(),
                outcome: if eval.won {
                    "Win".to_string()
                } else {
                    "Loss".to_string()
                },
                pnl: eval.pnl,
                stake_amount: pred.stake_amount,
                contract_side: Some(side_label(eval.side)),
                market_price_at_entry: eval.market_price_at_entry_pct,
                notes: None,
                resolved_at: resolved_at.clone(),
            });
        }
    }

    Ok(PrizePicksGradingSummary {
        total_predictions: pending.len() as u32,
        pending_gradable: pending.len() as u32,
        graded: results.len() as u32,
        wins,
        losses,
        total_pnl,
        results,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn empty_binary_summary() -> PrizePicksGradingSummary {
    PrizePicksGradingSummary {
        total_predictions: 0,
        pending_gradable: 0,
        graded: 0,
        wins: 0,
        losses: 0,
        total_pnl: 0.0,
        results: vec![],
        fetched_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub fn spawn_auto_grade_task<T: MarketDataProvider + Send + 'static>(
    prizepicks: std::sync::Arc<tokio::sync::Mutex<T>>,
    tracker: std::sync::Arc<tokio::sync::Mutex<PredictionTracker>>,
    poll_interval_secs: u64,
) {
    let interval_secs = poll_interval_secs.max(60);
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let pending_count = {
                let t = tracker.lock().await;
                t.get_prizepicks_predictions()
                    .await
                    .into_iter()
                    .filter(|p| p.actual_outcome.is_none())
                    .count()
            };
            if pending_count == 0 {
                continue;
            }
            let summary = {
                let t = tracker.lock().await;
                let client = prizepicks.lock().await;
                match grade_pending_predictions(&t, &*client).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("prizepicks auto-grade: {}", e);
                        continue;
                    }
                }
            };
            if summary.graded > 0 {
                tracing::info!(
                    "prizepicks auto-grade: {} graded ({}W/{}L, ${:.2})",
                    summary.graded,
                    summary.wins,
                    summary.losses,
                    summary.total_pnl
                );
            }
        }
    });
}

fn side_label(side: PrizePicksBetSide) -> String {
    match side {
        PrizePicksBetSide::Yes => "YES".to_string(),
        PrizePicksBetSide::No => "NO".to_string(),
        PrizePicksBetSide::Over => "OVER".to_string(),
        PrizePicksBetSide::Under => "UNDER".to_string(),
        PrizePicksBetSide::Pass => "PASS".to_string(),
        PrizePicksBetSide::Unknown => "UNKNOWN".to_string(),
    }
}

pub fn resolved_bet_won(pred: &PrizePicksPrediction) -> Option<bool> {
    let actual = pred.actual_outcome.as_deref()?;
    let side = parse_bet_side(pred.contract_side.as_deref(), pred.pick_type.as_deref());
    bet_won(side, actual)
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Over/Under grading tests ──

    #[test]
    fn over_wins_when_actual_above_line() {
        assert_eq!(grade_over_under("Over", 275.5, 289.0), PropGrade::Win);
    }

    #[test]
    fn over_losses_when_actual_below_line() {
        assert_eq!(grade_over_under("Over", 275.5, 260.0), PropGrade::Loss);
    }

    #[test]
    fn under_wins_when_actual_below_line() {
        assert_eq!(grade_over_under("Under", 275.5, 260.0), PropGrade::Win);
    }

    #[test]
    fn under_losses_when_actual_above_line() {
        assert_eq!(grade_over_under("Under", 275.5, 289.0), PropGrade::Loss);
    }

    #[test]
    fn exact_match_is_push() {
        assert_eq!(grade_over_under("Over", 275.5, 275.5), PropGrade::Push);
        assert_eq!(grade_over_under("Under", 6.5, 6.5), PropGrade::Push);
    }

    #[test]
    fn unknown_pick_type_with_stat_value_is_not_graded_as_push() {
        let mut pred = make_prop_pred("bad-pick", "Garbage", 275.5, Some(289.0), 10.0, 3.0);
        pred.pick_type = Some("Garbage".into());

        assert!(evaluate_prop_bet(&pred).is_none());
    }

    #[test]
    fn dnp_without_supported_pick_type_is_still_graded() {
        let mut pred = make_prop_pred("dnp", "Garbage", 275.5, None, 10.0, 3.0);
        pred.actual_outcome = Some("DNP".into());

        let eval = evaluate_prop_bet(&pred).unwrap();
        assert_eq!(eval.grade, PropGrade::DNP);
        assert!((eval.pnl - 0.0).abs() < 0.001);
    }

    #[test]
    fn over_with_more_alias() {
        assert_eq!(grade_over_under("More", 24.5, 26.0), PropGrade::Win);
    }

    #[test]
    fn under_with_less_alias() {
        assert_eq!(grade_over_under("Less", 24.5, 22.0), PropGrade::Win);
    }

    // ── PnL calculation tests ──

    #[test]
    fn prop_pnl_win_3x_multiplier() {
        // $10 stake, 3x multiplier (2-pick Power Play) → $20 profit
        assert!((prop_pnl(10.0, &PropGrade::Win, 3.0) - 20.0).abs() < 0.001);
    }

    #[test]
    fn prop_pnl_loss() {
        // $10 stake, loss → -$10
        assert!((prop_pnl(10.0, &PropGrade::Loss, 3.0) - (-10.0)).abs() < 0.001);
    }

    #[test]
    fn prop_pnl_push() {
        // Push → $0 (stake returned)
        assert!((prop_pnl(10.0, &PropGrade::Push, 3.0)).abs() < 0.001);
    }

    #[test]
    fn prop_pnl_dnp() {
        // DNP → $0 (stake returned)
        assert!((prop_pnl(10.0, &PropGrade::DNP, 3.0)).abs() < 0.001);
    }

    #[test]
    fn dnp_outcome_aliases_are_recognized() {
        assert!(is_dnp_outcome("DNP"));
        assert!(is_dnp_outcome("did not play"));
        assert!(is_dnp_outcome("did not participate"));
        assert!(!is_dnp_outcome("injured"));
    }

    #[test]
    fn evaluate_prop_bet_marks_dnp_without_stat_value() {
        let mut pred = make_prop_pred("dnp", "Over", 275.5, None, 10.0, 3.0);
        pred.actual_outcome = Some("DNP".into());

        let eval = evaluate_prop_bet(&pred).unwrap();

        assert_eq!(eval.grade, PropGrade::DNP);
        assert_eq!(eval.actual_value, None);
        assert!((eval.pnl - 0.0).abs() < 0.001);
    }

    #[test]
    fn pending_prop_filter_accepts_dnp_without_stat_value() {
        let mut dnp_pred = make_prop_pred("dnp", "Over", 275.5, None, 10.0, 3.0);
        dnp_pred.actual_outcome = Some("did not play".into());
        let pending_pred = make_prop_pred("pending", "Over", 275.5, None, 10.0, 3.0);

        assert!(prop_prediction_has_gradeable_result(&dnp_pred));
        assert!(!prop_prediction_has_gradeable_result(&pending_pred));
    }

    #[test]
    fn prop_pnl_win_10x_multiplier() {
        // $10 stake, 10x multiplier (4-pick Power Play) → $90 profit
        assert!((prop_pnl(10.0, &PropGrade::Win, 10.0) - 90.0).abs() < 0.001);
    }

    // ── Multiplier table tests ──

    #[test]
    fn power_play_multipliers() {
        assert!((power_play_multiplier(2) - 3.0).abs() < 0.001);
        assert!((power_play_multiplier(3) - 6.0).abs() < 0.001);
        assert!((power_play_multiplier(4) - 10.0).abs() < 0.001);
        assert!((power_play_multiplier(5) - 20.0).abs() < 0.001);
        assert!((power_play_multiplier(6) - 37.5).abs() < 0.001);
    }

    #[test]
    fn flex_play_multipliers() {
        assert!((flex_play_multiplier(6, 6) - 25.0).abs() < 0.001);
        assert!((flex_play_multiplier(5, 6) - 2.0).abs() < 0.001);
        assert!((flex_play_multiplier(4, 6) - 0.4).abs() < 0.001);
        assert!((flex_play_multiplier(5, 5) - 10.0).abs() < 0.001);
        assert!((flex_play_multiplier(4, 5) - 2.0).abs() < 0.001);
        assert!((flex_play_multiplier(3, 3) - 3.0).abs() < 0.001);
        assert!((flex_play_multiplier(2, 3) - 1.0).abs() < 0.001);
    }

    #[test]
    fn flex_play_partial_six_pack_without_payout_is_zero() {
        assert!((flex_play_multiplier(3, 6) - 0.0).abs() < 0.001);
        assert!((flex_play_multiplier(2, 6) - 0.0).abs() < 0.001);
    }

    // ── Lineup PnL tests ──

    fn make_prop_pred(
        id: &str,
        pick_type: &str,
        line: f64,
        actual: Option<f64>,
        stake: f64,
        multiplier: f64,
    ) -> PrizePicksPrediction {
        PrizePicksPrediction {
            id: id.into(),
            ticker: "PROP".into(),
            title: "Test Player".into(),
            category: "Passing Yards".into(),
            predicted_probability: 55.0,
            actual_outcome: None,
            confidence_score: Some(75),
            reasoning: None,
            created_at: String::new(),
            resolved_at: None,
            stake_amount: stake,
            pnl: None,
            pick_type: Some(pick_type.into()),
            price_to_enter: None,
            market_price_at_entry: None,
            contract_side: None,
            edge_points: None,
            fractional_kelly_pct: None,
            recommended_stake_dollars: None,
            risk_flags: None,
            thesis: None,
            data_quality: None,
            decision: None,
            line: Some(line),
            actual_stat_value: actual,
            multiplier: Some(multiplier),
        }
    }

    #[test]
    fn lineup_all_win_power_play() {
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 3.0);
        let p2 = make_prop_pred("2", "Under", 6.5, Some(5.0), 10.0, 3.0);
        let picks = vec![&p1, &p2];
        let (pnl, wins, losses, _pushes, _dnps) = calculate_lineup_pnl(&picks, false);
        assert_eq!(wins, 2);
        assert_eq!(losses, 0);
        assert!((pnl - 40.0).abs() < 0.001); // $20 stake * (3x - 1) = $40
    }

    #[test]
    fn lineup_one_loss_power_play_loses_all() {
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 3.0);
        let p2 = make_prop_pred("2", "Under", 6.5, Some(8.0), 10.0, 3.0); // Lost
        let picks = vec![&p1, &p2];
        let (pnl, wins, losses, _pushes, _dnps) = calculate_lineup_pnl(&picks, false);
        assert_eq!(wins, 1);
        assert_eq!(losses, 1);
        assert!((pnl - (-20.0)).abs() < 0.001); // Lost entire $20 stake
    }

    #[test]
    fn lineup_push_reduces_multiplier() {
        // 3-pick Power Play with 1 push → reverts to 2-pick rate
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 6.0);
        let p2 = make_prop_pred("2", "Under", 6.5, Some(5.0), 10.0, 6.0);
        let p3 = make_prop_pred("3", "Over", 20.5, Some(20.5), 10.0, 6.0); // Push
        let picks = vec![&p1, &p2, &p3];
        let (pnl, wins, _losses, pushes, _dnps) = calculate_lineup_pnl(&picks, false);
        assert_eq!(wins, 2);
        assert_eq!(pushes, 1);
        // 3-pick with 1 push → reverts to 2-pick rate (3x)
        // $30 stake * (3x - 1) = $60
        assert!((pnl - 60.0).abs() < 0.001);
    }

    #[test]
    fn lineup_pending_without_actuals_refunds_power_play() {
        let p1 = make_prop_pred("1", "Over", 275.5, None, 10.0, 3.0);
        let p2 = make_prop_pred("2", "Under", 6.5, None, 10.0, 3.0);
        let picks = vec![&p1, &p2];
        let (pnl, wins, losses, pushes, dnps) = calculate_lineup_pnl(&picks, false);
        assert_eq!(wins, 0);
        assert_eq!(losses, 0);
        assert_eq!(pushes, 0);
        assert_eq!(dnps, 0);
        assert!((pnl - 0.0).abs() < 0.001);
    }

    #[test]
    fn lineup_with_pending_pick_does_not_payout() {
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 3.0);
        let p2 = make_prop_pred("2", "Under", 6.5, None, 10.0, 3.0);
        let picks = vec![&p1, &p2];
        let (pnl, wins, losses, pushes, dnps) = calculate_lineup_pnl(&picks, false);
        assert_eq!(wins, 1);
        assert_eq!(losses, 0);
        assert_eq!(pushes, 0);
        assert_eq!(dnps, 0);
        assert!((pnl - 0.0).abs() < 0.001);
    }

    #[test]
    fn lineup_power_play_dnp_reduces_multiplier() {
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 6.0);
        let p2 = make_prop_pred("2", "Under", 6.5, Some(5.0), 10.0, 6.0);
        let mut p3 = make_prop_pred("3", "Over", 20.5, None, 10.0, 6.0);
        p3.actual_outcome = Some("DNP".into());

        let picks = vec![&p1, &p2, &p3];
        let (pnl, wins, losses, pushes, dnps) = calculate_lineup_pnl(&picks, false);

        assert_eq!(wins, 2);
        assert_eq!(losses, 0);
        assert_eq!(pushes, 0);
        assert_eq!(dnps, 1);
        assert!((pnl - 60.0).abs() < 0.001); // 3-pick Power Play with 1 DNP reverts to 2-pick rate: $30 * (3x - 1)
    }

    #[test]
    fn lineup_flex_play_dnp_reduces_pick_count() {
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 6.0);
        let p2 = make_prop_pred("2", "Under", 6.5, Some(5.0), 10.0, 6.0);
        let p3 = make_prop_pred("3", "Over", 20.5, Some(21.0), 10.0, 6.0);
        let mut p4 = make_prop_pred("4", "Under", 8.5, None, 10.0, 6.0);
        p4.actual_outcome = Some("DNP".into());

        let picks = vec![&p1, &p2, &p3, &p4];
        let (pnl, wins, losses, pushes, dnps) = calculate_lineup_pnl(&picks, true);

        assert_eq!(wins, 3);
        assert_eq!(losses, 0);
        assert_eq!(pushes, 0);
        assert_eq!(dnps, 1);
        assert!((pnl - 80.0).abs() < 0.001); // 4-pick Flex with 1 DNP pays as 3-pick Flex: $40 * (3x - 1)
    }

    #[test]
    fn lineup_flex_play_with_three_losses_loses_stake() {
        let p1 = make_prop_pred("1", "Over", 275.5, Some(289.0), 10.0, 6.0);
        let p2 = make_prop_pred("2", "Under", 6.5, Some(5.0), 10.0, 6.0);
        let p3 = make_prop_pred("3", "Over", 20.5, Some(21.0), 10.0, 6.0);
        let p4 = make_prop_pred("4", "Under", 8.5, Some(10.0), 10.0, 6.0);
        let p5 = make_prop_pred("5", "Over", 12.5, Some(11.0), 10.0, 6.0);
        let p6 = make_prop_pred("6", "Under", 1.5, Some(2.0), 10.0, 6.0);

        let picks = vec![&p1, &p2, &p3, &p4, &p5, &p6];
        let (pnl, wins, losses, pushes, dnps) = calculate_lineup_pnl(&picks, true);

        assert_eq!(wins, 3);
        assert_eq!(losses, 3);
        assert_eq!(pushes, 0);
        assert_eq!(dnps, 0);
        assert!((pnl - (-60.0)).abs() < 0.001); // 3-for-6 Flex has no payout: lose $60 stake
    }

    // ── Binary contract grading tests (legacy) ──

    fn pred(side: &str, fair: f64, stake: f64, entry: f64) -> PrizePicksPrediction {
        PrizePicksPrediction {
            id: "t".into(),
            ticker: "KXTEST".into(),
            title: "T".into(),
            category: "Economics".into(),
            predicted_probability: fair,
            actual_outcome: None,
            confidence_score: None,
            reasoning: None,
            created_at: String::new(),
            resolved_at: None,
            stake_amount: stake,
            pnl: None,
            pick_type: None,
            price_to_enter: Some(entry),
            market_price_at_entry: None,
            contract_side: Some(side.to_string()),
            edge_points: None,
            fractional_kelly_pct: None,
            recommended_stake_dollars: None,
            risk_flags: None,
            thesis: None,
            data_quality: None,
            decision: None,
            line: None,
            actual_stat_value: None,
            multiplier: None,
        }
    }

    #[test]
    fn yes_below_fifty_wins() {
        assert!(
            evaluate_bet(&pred("YES", 48.0, 100.0, 0.52), "Yes")
                .unwrap()
                .won
        );
    }

    #[test]
    fn no_wins_on_no() {
        assert!(
            evaluate_bet(&pred("NO", 40.0, 100.0, 0.40), "No")
                .unwrap()
                .won
        );
    }

    #[test]
    fn over_under_contract_side_is_recognized_for_prop_tracking() {
        assert_eq!(parse_bet_side(Some("OVER"), None), PrizePicksBetSide::Over);
        assert_eq!(parse_bet_side(Some("Under"), None), PrizePicksBetSide::Under);
        assert_eq!(parse_bet_side(None, Some("OVER")), PrizePicksBetSide::Over);
        assert_eq!(
            parse_bet_side(None, Some("under")),
            PrizePicksBetSide::Under
        );
    }

    #[test]
    fn infer_market_price_at_entry_preserves_selected_over_under_probability() {
        assert_eq!(
            infer_market_price_at_entry(None, Some(62.0), Some("OVER")),
            Some(62.0)
        );
        assert_eq!(
            infer_market_price_at_entry(None, Some(0.62), Some("UNDER")),
            Some(62.0)
        );
        assert_eq!(
            infer_market_price_at_entry(Some(70.0), Some(0.62), Some("UNDER")),
            Some(70.0)
        );
    }

    #[test]
    fn over_under_bet_won_matches_actual_over_under_result() {
        assert_eq!(bet_won(PrizePicksBetSide::Over, "Over"), Some(true));
        assert_eq!(bet_won(PrizePicksBetSide::Over, "under"), Some(false));
        assert_eq!(bet_won(PrizePicksBetSide::Under, "UNDER"), Some(true));
        assert_eq!(bet_won(PrizePicksBetSide::Under, "Over"), Some(false));
    }
}
