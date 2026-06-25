//! Portfolio correlation detection and Kelly stake scaling for PrizePicks markets.

use super::models::{parse_bet_side, PrizePicksBetSide};
use super::models::{PrizePicksPosition, PrizePicksPrediction};
use crate::analysis::kelly_shrinkage::KellyShrinkageReport;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CorrelationStrength {
    None,
    Category,
    Series,
    Event,
}

impl CorrelationStrength {
    /// Kelly multiplier applied when this correlation is detected.
    pub fn kelly_multiplier(self) -> f64 {
        match self {
            CorrelationStrength::None => 1.0,
            CorrelationStrength::Category => 0.90,
            CorrelationStrength::Series => 0.75,
            CorrelationStrength::Event => 0.50,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CorrelationStrength::None => "independent",
            CorrelationStrength::Category => "same category",
            CorrelationStrength::Series => "same series",
            CorrelationStrength::Event => "same event",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioExposure {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub contract_side: String,
    pub stake_amount: f64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationConflict {
    pub exposure_ticker: String,
    pub exposure_title: String,
    pub strength: String,
    pub kelly_multiplier: f64,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeAdjustment {
    pub kelly_scale: f64,
    pub raw_recommended_stake: f64,
    pub adjusted_recommended_stake: f64,
    pub conflicts: Vec<CorrelationConflict>,
    pub warnings: Vec<String>,
    /// Optional Brier-driven shrinkage layer. `None` when the caller did not
    /// compute one (e.g. legacy paths). When present, the shrinkage multiplier
    /// is folded into `kelly_scale` after correlation scaling.
    pub kelly_shrinkage: Option<KellyShrinkageReport>,
}

/// Parse player/game keys from a PrizePicks prop ticker.
pub fn ticker_keys(ticker: &str) -> (String, String) {
    let parts: Vec<&str> = ticker.split('-').collect();
    let series = parts.first().unwrap_or(&ticker).to_string();
    let event = if parts.len() >= 2 {
        format!("{}-{}", parts[0], parts[1])
    } else {
        series.clone()
    };
    (series, event)
}

pub fn correlation_strength(
    target_ticker: &str,
    target_category: &str,
    exposure_ticker: &str,
    exposure_category: &str,
) -> CorrelationStrength {
    if target_ticker == exposure_ticker {
        return CorrelationStrength::Event;
    }
    let (t_series, t_event) = ticker_keys(target_ticker);
    let (e_series, e_event) = ticker_keys(exposure_ticker);
    if t_event == e_event {
        return CorrelationStrength::Event;
    }
    if t_series == e_series {
        return CorrelationStrength::Series;
    }
    if !target_category.is_empty()
        && !exposure_category.is_empty()
        && target_category.eq_ignore_ascii_case(exposure_category)
    {
        return CorrelationStrength::Category;
    }
    CorrelationStrength::None
}

/// Build exposures from pending paper/chat predictions plus live portfolio positions.
/// Build exposures from authenticated PrizePicks portfolio positions.
pub fn exposures_from_positions(positions: &[PrizePicksPosition]) -> Vec<PortfolioExposure> {
    positions
        .iter()
        .filter(|p| p.position != 0)
        .map(|p| {
            let side = if p.position > 0 { "Yes" } else { "No" };
            let stake = (p.market_exposure.unsigned_abs() as f64) / 100.0;
            PortfolioExposure {
                ticker: p.ticker.clone(),
                title: p.ticker.clone(),
                category: String::new(),
                contract_side: side.to_string(),
                stake_amount: stake.max(0.01),
                source: "portfolio".to_string(),
            }
        })
        .collect()
}

pub fn exposures_from_predictions(pending: &[PrizePicksPrediction]) -> Vec<PortfolioExposure> {
    pending
        .iter()
        .filter(|p| p.actual_outcome.is_none())
        .filter_map(|p| {
            let side = parse_bet_side(p.contract_side.as_deref(), p.pick_type.as_deref());
            if side == PrizePicksBetSide::Pass || side == PrizePicksBetSide::Unknown {
                return None;
            }
            Some(PortfolioExposure {
                ticker: p.ticker.clone(),
                title: p.title.clone(),
                category: p.category.clone(),
                contract_side: format!("{:?}", side),
                stake_amount: p.stake_amount,
                source: "prediction".to_string(),
            })
        })
        .collect()
}

pub fn compute_stake_adjustment(
    target_ticker: &str,
    target_category: &str,
    target_side: Option<&str>,
    recommended_stake: f64,
    exposures: &[PortfolioExposure],
) -> StakeAdjustment {
    compute_stake_adjustment_with_shrinkage(
        target_ticker,
        target_category,
        target_side,
        recommended_stake,
        exposures,
        None,
    )
}

/// Like `compute_stake_adjustment` but applies a Brier-driven shrinkage layer
/// on top of the correlation-aware Kelly scale. The shrinkage multiplier is
/// folded into the final `kelly_scale`; `adjusted_recommended_stake` is the
/// raw stake scaled by the combined factor.
pub fn compute_stake_adjustment_with_shrinkage(
    target_ticker: &str,
    target_category: &str,
    target_side: Option<&str>,
    recommended_stake: f64,
    exposures: &[PortfolioExposure],
    shrinkage: Option<KellyShrinkageReport>,
) -> StakeAdjustment {
    let mut conflicts = Vec::new();
    let mut min_scale = 1.0_f64;
    let mut warnings = Vec::new();

    let target_bet_side = parse_bet_side(target_side, None);

    for exp in exposures {
        if exp.ticker == target_ticker {
            warnings.push(format!(
                "Existing exposure on {} (${:.2} {}) — adding size increases concentration.",
                exp.ticker, exp.stake_amount, exp.contract_side
            ));
            min_scale = min_scale.min(0.85);
            continue;
        }

        let strength =
            correlation_strength(target_ticker, target_category, &exp.ticker, &exp.category);
        if strength == CorrelationStrength::None {
            continue;
        }

        let mult = strength.kelly_multiplier();
        min_scale = min_scale.min(mult);

        let same_direction = exp
            .contract_side
            .eq_ignore_ascii_case(&format!("{:?}", target_bet_side));
        let direction_note = if same_direction {
            "same direction"
        } else {
            "opposite direction (partial hedge)"
        };

        conflicts.push(CorrelationConflict {
            exposure_ticker: exp.ticker.clone(),
            exposure_title: exp.title.clone(),
            strength: strength.label().to_string(),
            kelly_multiplier: mult,
            explanation: format!(
                "Correlated with active {} position (${:.2} {}) — {}",
                exp.source, exp.stake_amount, exp.contract_side, direction_note
            ),
        });
    }

    if min_scale < 1.0 {
        warnings.push(format!(
            "Kelly stake scaled to {:.0}% due to portfolio correlation (raw ${:.2} → ${:.2}).",
            min_scale * 100.0,
            recommended_stake,
            recommended_stake * min_scale
        ));
    }

    // Apply Brier-driven shrinkage on top of the correlation scale. The
    // shrinkage multiplier is always in [MIN_MULT, 1.0] (see
    // analysis::kelly_shrinkage), so combining it can only ever reduce the
    // stake further, never amplify it.
    let (combined_scale, shrinkage_note) = match shrinkage.as_ref() {
        Some(s) => {
            let combined = (min_scale * s.multiplier).clamp(0.0, 1.0);
            if s.multiplier < 1.0 {
                let note = format!(
                    "Volatility-adjusted Kelly: {:.0}% of raw (Brier-shrunk from observed history).",
                    s.multiplier * 100.0
                );
                (combined, Some(note))
            } else {
                (combined, None)
            }
        }
        None => (min_scale, None),
    };
    if let Some(note) = shrinkage_note {
        warnings.push(note);
    }

    StakeAdjustment {
        kelly_scale: round4(combined_scale),
        raw_recommended_stake: recommended_stake,
        adjusted_recommended_stake: recommended_stake * combined_scale,
        conflicts,
        warnings,
        kelly_shrinkage: shrinkage,
    }
}

fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_correlation_scales_to_half() {
        let adj = compute_stake_adjustment(
            "NFL-JoshAllen-O-275.5",
            "NFL",
            Some("Over"),
            100.0,
            &[PortfolioExposure {
                ticker: "NFL-JoshAllen-U-275.5".into(),
                title: "Josh Allen passing yards Under 275.5".into(),
                category: "NFL".into(),
                contract_side: "Under".into(),
                stake_amount: 50.0,
                source: "prediction".into(),
            }],
        );
        assert!((adj.kelly_scale - 0.5).abs() < 0.01);
        assert!((adj.adjusted_recommended_stake - 50.0).abs() < 0.01);
    }

    #[test]
    fn pending_over_under_predictions_build_exposures() {
        let exposure = exposures_from_predictions(&[PrizePicksPrediction {
            id: "pending-prop".into(),
            ticker: "NFL-JoshAllen-O-275.5".into(),
            title: "Josh Allen passing yards Over 275.5".into(),
            category: "NFL".into(),
            predicted_probability: 58.0,
            actual_outcome: None,
            confidence_score: None,
            reasoning: None,
            created_at: String::new(),
            resolved_at: None,
            stake_amount: 40.0,
            pnl: None,
            pick_type: None,
            price_to_enter: Some(0.62),
            market_price_at_entry: None,
            contract_side: Some("Over".into()),
            edge_points: None,
            fractional_kelly_pct: None,
            recommended_stake_dollars: None,
            risk_flags: None,
            thesis: None,
            data_quality: None,
            decision: None,
            line: Some(275.5),
            actual_stat_value: None,
            multiplier: None,
        }]);

        assert_eq!(exposure.len(), 1);
        assert_eq!(exposure[0].contract_side, "Over");
        assert_eq!(exposure[0].stake_amount, 40.0);
    }

    #[test]
    fn shrinkage_folds_into_kelly_scale() {
        use crate::analysis::kelly_shrinkage::KellyShrinkageReport;
        // Event correlation alone would give 0.5; with a 0.8 shrinkage the
        // combined scale should be 0.4, never amplifying above 0.5.
        let shrinkage = KellyShrinkageReport {
            multiplier: 0.8,
            n: 50,
            brier: Some(0.10),
            base_rate: Some(0.6),
            climatology_brier: Some(0.24),
            brier_skill_score: Some(0.5833),
            sample_factor: 1.0,
            calibration_factor: 0.7638,
            reason: "test fixture".to_string(),
        };
        let adj = compute_stake_adjustment_with_shrinkage(
            "NFL-JoshAllen-O-275.5",
            "NFL",
            Some("Over"),
            100.0,
            &[PortfolioExposure {
                ticker: "NFL-JoshAllen-U-275.5".into(),
                title: "Josh Allen passing yards Under 275.5".into(),
                category: "NFL".into(),
                contract_side: "Under".into(),
                stake_amount: 50.0,
                source: "prediction".into(),
            }],
            Some(shrinkage.clone()),
        );
        assert!((adj.kelly_scale - 0.4).abs() < 1e-4, "got {}", adj.kelly_scale);
        assert!((adj.adjusted_recommended_stake - 40.0).abs() < 1e-4);
        assert!(adj.kelly_shrinkage.is_some());
        assert!(adj.warnings.iter().any(|w| w.contains("Volatility-adjusted")));
    }

    #[test]
    fn shrinkage_unity_keeps_legacy_behavior() {
        // When the shrinkage multiplier is 1.0 (cold start), the result must
        // match the legacy compute_stake_adjustment output.
        let shrinkage = KellyShrinkageReport::cold_start();
        let adj_with = compute_stake_adjustment_with_shrinkage(
            "NFL-JoshAllen-O-275.5",
            "NFL",
            Some("Over"),
            100.0,
            &[],
            Some(shrinkage),
        );
        let adj_legacy = compute_stake_adjustment(
            "NFL-JoshAllen-O-275.5",
            "NFL",
            Some("Over"),
            100.0,
            &[],
        );
        assert!((adj_with.kelly_scale - adj_legacy.kelly_scale).abs() < 1e-9);
        assert!(!adj_with.warnings.iter().any(|w| w.contains("Volatility-adjusted")));
    }

    #[test]
    fn shrinkage_warms_to_full_kelly() {
        use crate::analysis::kelly_shrinkage::KellyShrinkageReport;
        // Multiplier 1.0 must NOT add a volatility warning even if there is
        // an explicit report.
        let shrinkage = KellyShrinkageReport {
            multiplier: 1.0,
            n: 100,
            brier: Some(0.0),
            base_rate: Some(0.5),
            climatology_brier: Some(0.25),
            brier_skill_score: Some(1.0),
            sample_factor: 1.0,
            calibration_factor: 1.0,
            reason: "fully calibrated".to_string(),
        };
        let adj = compute_stake_adjustment_with_shrinkage(
            "NFL-Mahomes-O-300.5",
            "NFL",
            Some("Over"),
            50.0,
            &[],
            Some(shrinkage),
        );
        assert!((adj.kelly_scale - 1.0).abs() < 1e-9);
        assert!(!adj.warnings.iter().any(|w| w.contains("Volatility-adjusted")));
    }
}
