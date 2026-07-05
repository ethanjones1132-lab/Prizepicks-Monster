use criterion::{black_box, criterion_group, criterion_main, Criterion};
use prizepicks_monster_lib::analysis::kelly_shrinkage::KellyShrinkageReport;
use prizepicks_monster_lib::prizepicks::models::PrizePicksPrediction;
use prizepicks_monster_lib::prizepicks::portfolio_risk::{
    compute_stake_adjustment, compute_stake_adjustment_with_shrinkage, correlation_strength,
    exposures_from_predictions, PortfolioExposure, StakeAdjustment,
};

fn make_prediction(id: &str, ticker: &str, category: &str, side: &str) -> PrizePicksPrediction {
    PrizePicksPrediction {
        id: id.to_string(),
        ticker: ticker.to_string(),
        title: format!("Test Player {} {}", category, side),
        category: category.to_string(),
        predicted_probability: 0.55,
        actual_outcome: None,
        confidence_score: Some(80),
        reasoning: Some("Bench fixture".to_string()),
        created_at: "2024-01-15T20:00:00Z".to_string(),
        resolved_at: None,
        stake_amount: 10.0,
        pnl: None,
        pick_type: Some(side.to_string()),
        price_to_enter: Some(0.55),
        market_price_at_entry: Some(55.0),
        contract_side: Some(side.to_string()),
        edge_points: Some(5.0),
        fractional_kelly_pct: Some(0.25),
        recommended_stake_dollars: Some(10.0),
        risk_flags: Some(vec![]),
        thesis: Some("Bench thesis".to_string()),
        data_quality: Some("high".to_string()),
        decision: Some("Bench decision".to_string()),
        line: Some(275.5),
        actual_stat_value: None,
        multiplier: Some(3.0),
    }
}

fn make_predictions(n: usize) -> Vec<PrizePicksPrediction> {
    let categories = ["Passing Yards", "Rushing Yards", "Receiving Yards"];
    let sides = ["Over", "Under"];
    (0..n)
        .map(|i| {
            let category = categories[i % categories.len()];
            let side = sides[i % sides.len()];
            make_prediction(
                &format!("bench-pred-{}", i),
                &format!("PP-NFL-{}-PASS-275.5", i),
                category,
                side,
            )
        })
        .collect()
}

fn make_shrinkage() -> KellyShrinkageReport {
    KellyShrinkageReport {
        multiplier: 0.75,
        n: 100,
        brier: Some(0.22),
        base_rate: Some(0.5),
        climatology_brier: Some(0.25),
        brier_skill_score: Some(0.12),
        sample_factor: 1.0,
        calibration_factor: 0.75,
        reason: "warm".to_string(),
    }
}

fn portfolio_risk_bench(c: &mut Criterion) {
    let preds = make_predictions(10);
    let exposures: Vec<PortfolioExposure> = exposures_from_predictions(&preds);
    let shrinkage = make_shrinkage();

    // Tier-1: classify a single (target, exposure) pair.
    c.bench_function("correlation_strength_single", |b| {
        b.iter(|| {
            let result = correlation_strength(
                black_box("PP-NFL-QB-PASS-275.5"),
                black_box("Passing Yards"),
                black_box("PP-NFL-QB-PASS-275.5"),
                black_box("Passing Yards"),
            );
            black_box(result);
        })
    });

    // Tier-2: classify every (target, exposure) pair in the portfolio.
    // With 10 predictions the inner loop is 10 targets × up to 10 exposures.
    c.bench_function("correlation_strength_all_pairs_10", |b| {
        b.iter(|| {
            for target in &preds {
                for exp in &exposures {
                    let result = correlation_strength(
                        black_box(&target.ticker),
                        black_box(&target.category),
                        black_box(&exp.ticker),
                        black_box(&exp.category),
                    );
                    black_box(result);
                }
            }
        })
    });

    // Tier-3: full stake adjustment (correlation + Kelly) for a single pick
    // against a 10-exposure portfolio, without shrinkage.
    c.bench_function("compute_stake_adjustment_single", |b| {
        b.iter(|| {
            let result = compute_stake_adjustment(
                black_box(&preds[0].ticker),
                black_box(&preds[0].category),
                black_box(Some("Over")),
                black_box(10.0_f64),
                black_box(&exposures),
            );
            black_box(result);
        })
    });

    // Tier-3 with the Brier-driven shrinkage layer folded in.
    c.bench_function("compute_stake_adjustment_with_shrinkage_single", |b| {
        b.iter(|| {
            let result = compute_stake_adjustment_with_shrinkage(
                black_box(&preds[0].ticker),
                black_box(&preds[0].category),
                black_box(Some("Over")),
                black_box(10.0_f64),
                black_box(&exposures),
                black_box(Some(shrinkage.clone())),
            );
            black_box(result);
        })
    });

    // Tier-4: full portfolio adjustment — one stake call per pending pick.
    // Mirrors the load pattern when a batch of fresh predictions is sized
    // against the current paper-lot portfolio.
    c.bench_function("compute_stake_adjustment_portfolio_10", |b| {
        b.iter(|| {
            let results: Vec<StakeAdjustment> = preds
                .iter()
                .map(|p| {
                    compute_stake_adjustment(
                        black_box(&p.ticker),
                        black_box(&p.category),
                        black_box(Some("Over")),
                        black_box(10.0_f64),
                        black_box(&exposures),
                    )
                })
                .collect();
            black_box(results);
        })
    });
}

criterion_group!(portfolio_risk_benches, portfolio_risk_bench);
criterion_main!(portfolio_risk_benches);
