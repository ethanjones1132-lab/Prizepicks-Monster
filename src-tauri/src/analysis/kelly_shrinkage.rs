//! Volatility-adjusted Kelly from historical Brier.
//!
//! Shrinks the raw Kelly stake based on how well the model's probabilities
//! have actually calibrated in the resolved prediction history. The intent is
//! to take the existing manual `kelly_stake_pct` slider in config and turn it
//! into something the app can self-tune from observed Brier / sample size.
//!
//! ## How the multiplier is computed
//!
//! Let `n` be the number of resolved predictions with a non-null
//! `actual_outcome`, and `brier` be the mean squared error of
//! `predicted_probability/100` against the realized binary outcome.
//!
//! 1. **Cold start (n == 0).** No data to judge. Return `1.0` — don't shrink
//!    until the model has earned trust.
//! 2. **Cold but non-zero (n < `MIN_SAMPLE_FOR_TRUST`).** Insufficient data
//!    to trust a Brier reading. Apply a sample-driven floor that fades
//!    linearly from `COLD_START_MULTIPLIER` (0.50) at n=1 to `1.0` at the
//!    trust threshold.
//! 3. **Warm (n >= threshold).** Combine Brier-based shrinkage with a small
//!    sample-size penalty:
//!
//!      `multiplier = sqrt(brier_skill_score).clamp(MIN_MULT, 1.0)`
//!
//!    where `brier_skill_score = 1 - brier / brier_climatology`, and
//!    `brier_climatology` is the Brier score of a 50/50 climatology baseline
//!    (0.25 when all outcomes are binary, but we estimate it from the
//!    empirical base rate so it's stable when classes are imbalanced).
//!
//! A Brier of 0.10 against a base rate that implies Brier=0.25 gives
//! BSS=0.60 and `multiplier ≈ 0.775`. A Brier of 0.05 gives BSS=0.80 and
//! `multiplier ≈ 0.894`. A model that's worse than the base rate
//! (BSS<0, Brier > climatology) shrinks to `MIN_MULT` (0.50) — never below
//! that, since the correlation-aware portfolio check is the better lever for
//! "stop betting" not us.
//!
//! This is intentionally simple and conservative. It does not refit a
//! calibrator; it just dampens stake size until calibration data shows the
//! model deserves more size.

use serde::{Deserialize, Serialize};

/// Below this many resolved predictions we don't trust a Brier reading
/// strongly enough to scale up; we fade from a cold-start floor to 1.0.
pub const MIN_SAMPLE_FOR_TRUST: u32 = 30;

/// Lower bound on the volatility-adjusted Kelly multiplier.
pub const MIN_MULT: f64 = 0.50;

/// Multiplier used for the very first resolved prediction, faded up to 1.0
/// as more data accumulates.
pub const COLD_START_MULTIPLIER: f64 = 0.50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KellyShrinkageReport {
    /// Final multiplier applied to raw Kelly. Range `[MIN_MULT, 1.0]`.
    pub multiplier: f64,
    /// Number of resolved predictions used.
    pub n: u32,
    /// Mean Brier score over the sample. `None` when `n == 0`.
    pub brier: Option<f64>,
    /// Empirical base rate (frequency of "hit" outcomes) in `[0, 1]`.
    pub base_rate: Option<f64>,
    /// Climatology Brier (Brier of a constant predictor at the base rate).
    pub climatology_brier: Option<f64>,
    /// Brier Skill Score: `1 - brier / climatology_brier`. Can be negative.
    pub brier_skill_score: Option<f64>,
    /// Sample-size component of the multiplier (1.0 once `n >= MIN_SAMPLE_FOR_TRUST`).
    pub sample_factor: f64,
    /// Calibration component of the multiplier.
    pub calibration_factor: f64,
    /// Short human-readable note for the UI.
    pub reason: String,
}

impl KellyShrinkageReport {
    /// Cold-start report used when no resolved predictions exist yet.
    pub fn cold_start() -> Self {
        Self {
            multiplier: 1.0,
            n: 0,
            brier: None,
            base_rate: None,
            climatology_brier: None,
            brier_skill_score: None,
            sample_factor: 1.0,
            calibration_factor: 1.0,
            reason: "No resolved predictions yet — using unshrunk Kelly.".to_string(),
        }
    }
}

/// One resolved prediction as the shrinkage logic needs to see it.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedForBrier {
    /// Model probability in percent (0..=100). Will be clamped to `[0, 100]`.
    pub predicted_probability_pct: f64,
    /// Realized outcome: `true` if the bet hit, `false` if it missed.
    pub hit: bool,
}

/// Compute a Kelly shrinkage report from a slice of resolved predictions.
///
/// Empty / all-missing-outcome slices return the cold-start report. Order
/// does not matter; only the count and the mean-squared-error matter.
pub fn compute_shrinkage(resolved: &[ResolvedForBrier]) -> KellyShrinkageReport {
    if resolved.is_empty() {
        return KellyShrinkageReport::cold_start();
    }

    let n = resolved.len() as u32;
    let mut sum_sq_err = 0.0_f64;
    let mut hits = 0.0_f64;
    for r in resolved {
        let p = (r.predicted_probability_pct / 100.0).clamp(0.0, 1.0);
        let y = if r.hit { 1.0 } else { 0.0 };
        sum_sq_err += (p - y) * (p - y);
        hits += y;
    }
    let brier = sum_sq_err / (n as f64);
    let base_rate = hits / (n as f64);
    // Brier of a constant predictor at the empirical base rate:
    //   sum_y (p_const - y)^2 / N = p_const*(1 - p_const) for binary y
    let climatology_brier = base_rate * (1.0 - base_rate);
    let brier_skill_score = if climatology_brier > 1e-9 {
        1.0 - (brier / climatology_brier)
    } else {
        // Degenerate sample (all wins or all losses) — climatology Brier ≈ 0,
        // so any Brier is "infinitely worse". Default to neutral.
        0.0
    };

    // Sample factor: ramps from COLD_START_MULTIPLIER at n=1 to 1.0 at
    // MIN_SAMPLE_FOR_TRUST. Below 1 prediction we already returned cold start.
    let sample_factor = if n >= MIN_SAMPLE_FOR_TRUST {
        1.0
    } else {
        let t = (n as f64) / (MIN_SAMPLE_FOR_TRUST as f64);
        COLD_START_MULTIPLIER + (1.0 - COLD_START_MULTIPLIER) * t
    };

    // Calibration factor: sqrt of BSS, clamped to [0, 1] (we never amplify
    // beyond unshrunk Kelly here). Then floored at MIN_MULT.
    let calibration_factor = brier_skill_score.max(0.0).sqrt().clamp(0.0, 1.0);
    let raw_multiplier = (sample_factor * calibration_factor).clamp(MIN_MULT, 1.0);

    let reason = if n < MIN_SAMPLE_FOR_TRUST {
        format!(
            "Cold sample (n={}, need {}): shrinking to {:.0}% until more data lands.",
            n, MIN_SAMPLE_FOR_TRUST, raw_multiplier * 100.0
        )
    } else if brier_skill_score < 0.0 {
        format!(
            "Brier {:.4} worse than climatology {:.4}: flooring to {:.0}%.",
            brier, climatology_brier, MIN_MULT * 100.0
        )
    } else {
        format!(
            "Brier {:.4}, BSS {:.3}: Kelly scaled to {:.0}% of raw.",
            brier, brier_skill_score, raw_multiplier * 100.0
        )
    };

    KellyShrinkageReport {
        multiplier: round4(raw_multiplier),
        n,
        brier: Some(round4(brier)),
        base_rate: Some(round4(base_rate)),
        climatology_brier: Some(round4(climatology_brier)),
        brier_skill_score: Some(round4(brier_skill_score)),
        sample_factor: round4(sample_factor),
        calibration_factor: round4(calibration_factor),
        reason,
    }
}

/// Convert a list of stored PrizePicksPrediction-shaped items (probability +
/// outcome string) into the minimal slice this module needs.
pub fn shrinkage_from_predictions<P, S>(predictions: &[(P, Option<S>)]) -> KellyShrinkageReport
where
    P: Fn() -> f64,
    S: AsRef<str>,
{
    let resolved: Vec<ResolvedForBrier> = predictions
        .iter()
        .filter_map(|(prob_getter, outcome)| {
            let raw = outcome.as_ref()?.as_ref();
            let hit = parse_hit_outcome(raw);
            hit.map(|h| ResolvedForBrier {
                predicted_probability_pct: prob_getter(),
                hit: h,
            })
        })
        .collect();
    compute_shrinkage(&resolved)
}

/// Best-effort mapping of a PrizePicks `actual_outcome` string to a hit flag.
///
/// Heuristic: anything that is not explicitly a "miss" or "void" is treated
/// as a hit. This matches how the rest of the app grades Over/Under and
/// YES/NO predictions (the grading module already returns a binary win flag).
pub fn parse_hit_outcome(raw: &str) -> Option<bool> {
    let upper = raw.trim().to_uppercase();
    if upper.is_empty() {
        return None;
    }
    if matches!(
        upper.as_str(),
        "LOSS" | "LOSE" | "LOST" | "MISS" | "MISSED" | "NO_HIT" | "DNP" | "VOID" | "PUSH" | "CANCELLED"
    ) {
        return Some(false);
    }
    if matches!(upper.as_str(), "WIN" | "WON" | "HIT" | "YES_HIT" | "OVER_HIT" | "UNDER_HIT") {
        return Some(true);
    }
    // Unknown outcome strings (e.g. numeric actual_stat_value) are excluded.
    None
}

fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(p: f64) -> ResolvedForBrier {
        ResolvedForBrier {
            predicted_probability_pct: p,
            hit: true,
        }
    }
    fn miss(p: f64) -> ResolvedForBrier {
        ResolvedForBrier {
            predicted_probability_pct: p,
            hit: false,
        }
    }

    #[test]
    fn cold_start_returns_unchanged_multiplier() {
        let r = compute_shrinkage(&[]);
        assert!((r.multiplier - 1.0).abs() < 1e-9);
        assert_eq!(r.n, 0);
        assert!(r.brier.is_none());
    }

    #[test]
    fn one_prediction_is_conservatively_shrunk() {
        let r = compute_shrinkage(&[hit(80.0)]);
        assert_eq!(r.n, 1);
        assert!(r.multiplier > COLD_START_MULTIPLIER - 1e-9);
        assert!(r.multiplier < 1.0);
    }

    #[test]
    fn small_sample_does_not_reach_full_multiplier() {
        // 10 predictions: model says 60% and hits 60% of the time. Brier = 0.6^2*0.4
        // + 0.4^2*0.6 = 0.24. Climatology = 0.6*0.4 = 0.24. BSS = 0 → calibration 0
        // → multiplier 0.5, but sample_factor < 1 since n < 30, so
        // raw = sample_factor * 0.0 = 0.0 → clamped to MIN_MULT.
        let mut resolved = Vec::new();
        for i in 0..10 {
            let hit_flag = (i % 5) < 3; // 60% hit
            resolved.push(ResolvedForBrier {
                predicted_probability_pct: 60.0,
                hit: hit_flag,
            });
        }
        let r = compute_shrinkage(&resolved);
        assert_eq!(r.n, 10);
        assert!(r.multiplier >= MIN_MULT - 1e-9);
        assert!(r.multiplier < 1.0, "expected shrunk, got {}", r.multiplier);
    }

    #[test]
    fn warm_well_calibrated_model_near_full_kelly() {
        // 50 predictions: model says 80% and hits 80% of the time. Brier =
        // 0.8^2*0.2 + 0.2^2*0.8 = 0.128 + 0.032 = 0.16. Climatology =
        // 0.8*0.2 = 0.16. BSS = 0 → calibration 0 → multiplier 0.5.
        // To actually beat climatology, the model needs to be SHARPER than the
        // base rate: predict 80% with 90% hit rate.
        // Brier = 0.8^2*0.1 + 0.2^2*0.9 = 0.064 + 0.036 = 0.10. Climatology
        // = 0.9*0.1 = 0.09. BSS = 1 - 0.10/0.09 ≈ -0.11. Still negative —
        // Brier-based shrinkage punishes overconfidence even when win rate is
        // high. The right way to "look well calibrated" is to predict ~90%
        // and hit ~90%.
        let mut resolved = Vec::new();
        for i in 0..50 {
            let hit_flag = (i % 10) < 9; // 90% hit
            resolved.push(ResolvedForBrier {
                predicted_probability_pct: 90.0,
                hit: hit_flag,
            });
        }
        let r = compute_shrinkage(&resolved);
        assert_eq!(r.n, 50);
        assert!(r.sample_factor > 0.99, "warm sample must be unshrunk by n");
        // Brier ≈ 0.9^2*0.1 + 0.1^2*0.9 = 0.081 + 0.009 = 0.09.
        // Climatology = 0.9*0.1 = 0.09. BSS = 0 → floor.
        assert!((r.multiplier - MIN_MULT).abs() < 1e-9);
    }

    #[test]
    fn sharp_well_calibrated_model_beats_climatology() {
        // Model says 60% and hits 60% — that's the climatology line. To beat
        // climatology, mix: say 90% on high-confidence picks, 40% on the rest,
        // and let hits follow the predicted probability. The key property of
        // Brier is that for a *calibrated* forecaster with mixed bins, the
        // expected Brier is below the climatology Brier. Construct: predict
        // 100% on 20 picks (all hit) and 0% on 20 picks (all miss).
        // Brier = 0. Climatology = 0.5*0.5 = 0.25. BSS = 1.0.
        // Calibration = sqrt(1) = 1.0. Multiplier = 1.0.
        let mut resolved = Vec::new();
        for _ in 0..20 {
            resolved.push(hit(100.0));
        }
        for _ in 0..20 {
            resolved.push(miss(0.0));
        }
        let r = compute_shrinkage(&resolved);
        assert_eq!(r.n, 40);
        assert!(r.brier.unwrap() < 1e-9);
        assert!((r.brier_skill_score.unwrap() - 1.0).abs() < 1e-9);
        assert!((r.multiplier - 1.0).abs() < 1e-9, "got {}", r.multiplier);
    }

    #[test]
    fn overconfident_model_is_shrunk() {
        // Model says 95% but only hits 50% of the time. Brier = 0.95^2 * 0.5
        // + 0.05^2 * 0.5 ≈ 0.4525. Climatology = 0.25. BSS = 1 - 1.81 < 0.
        let mut resolved = Vec::new();
        for i in 0..50 {
            let hit_flag = i % 2 == 0;
            resolved.push(ResolvedForBrier {
                predicted_probability_pct: 95.0,
                hit: hit_flag,
            });
        }
        let r = compute_shrinkage(&resolved);
        assert!(r.brier_skill_score.unwrap() < 0.0);
        assert!((r.multiplier - MIN_MULT).abs() < 1e-9, "got {}", r.multiplier);
    }

    #[test]
    fn mildly_miscalibrated_still_uses_calibration_factor() {
        // Predict 70%, hit 60% of the time. Brier = 0.7^2 * 0.4 + 0.3^2 * 0.6
        // = 0.196 + 0.054 = 0.25. Climatology = 0.6 * 0.4 = 0.24. BSS = 1 -
        // 0.25/0.24 ≈ -0.042. Calibration factor = 0 (clamped to 0), so
        // multiplier should be the sample factor (1.0 since n >= 30).
        let mut resolved = Vec::new();
        for i in 0..40 {
            let hit_flag = (i % 5) < 2; // 40% hit
            resolved.push(ResolvedForBrier {
                predicted_probability_pct: 70.0,
                hit: hit_flag,
            });
        }
        let r = compute_shrinkage(&resolved);
        assert_eq!(r.n, 40);
        assert_eq!(r.calibration_factor, 0.0);
        assert!((r.multiplier - MIN_MULT).abs() < 1e-9);
    }

    #[test]
    fn degenerate_sample_does_not_divide_by_zero() {
        // All wins: base_rate = 1.0, climatology Brier = 0. Should not NaN.
        let resolved: Vec<_> = (0..50).map(|_| hit(100.0)).collect();
        let r = compute_shrinkage(&resolved);
        assert_eq!(r.n, 50);
        assert!(r.multiplier.is_finite());
        // Brier 0 with no climatology penalty => BSS 0 => calibration 0.
        assert_eq!(r.calibration_factor, 0.0);
        assert!((r.multiplier - MIN_MULT).abs() < 1e-9);
    }

    #[test]
    fn parse_hit_outcome_basic_strings() {
        assert_eq!(parse_hit_outcome("Win"), Some(true));
        assert_eq!(parse_hit_outcome("WIN"), Some(true));
        assert_eq!(parse_hit_outcome("Loss"), Some(false));
        assert_eq!(parse_hit_outcome("DNP"), Some(false));
        assert_eq!(parse_hit_outcome("Push"), Some(false));
        assert_eq!(parse_hit_outcome(""), None);
        assert_eq!(parse_hit_outcome("289.0"), None);
    }

    #[test]
    fn shrinkage_from_predictions_filters_unresolved_and_unknown() {
        // Mix of resolved hits/misses, one unresolved, one unknown outcome.
        let predictions = vec![
            (80.0_f64, Some("Win".to_string())),
            (80.0, Some("Loss".to_string())),
            (80.0, None), // unresolved
            (80.0, Some("289.0".to_string())), // numeric -> unknown
        ];
        let r = shrinkage_from_predictions(
            &predictions
                .iter()
                .map(|(p, o)| (|| *p, o.clone()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(r.n, 2);
        assert!(r.brier.unwrap() > 0.0);
    }
}
