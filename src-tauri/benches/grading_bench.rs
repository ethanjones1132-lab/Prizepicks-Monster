use criterion::{black_box, criterion_group, criterion_main, Criterion};
use prizepicks_monster_lib::prizepicks::grading::{
    evaluate_prop_bet, grade_over_under, prop_pnl, power_play_multiplier, PropGrade,
    calculate_lineup_pnl,
};
use prizepicks_monster_lib::prizepicks::models::PrizePicksPrediction;

fn make_prediction() -> PrizePicksPrediction {
    PrizePicksPrediction {
        id: "bench-pred-1".to_string(),
        ticker: "PP-NFL-QB-PASS-275.5".to_string(),
        title: "Patrick Mahomes Over 275.5 Passing Yards".to_string(),
        category: "Passing Yards".to_string(),
        predicted_probability: 0.55,
        actual_outcome: Some("Over".to_string()),
        confidence_score: Some(80),
        reasoning: Some("Strong matchup".to_string()),
        created_at: "2024-01-15T20:00:00Z".to_string(),
        resolved_at: Some("2024-01-15T23:00:00Z".to_string()),
        stake_amount: 10.0,
        pnl: None,
        pick_type: Some("Over".to_string()),
        price_to_enter: Some(0.55),
        market_price_at_entry: Some(55.0),
        contract_side: Some("Over".to_string()),
        edge_points: Some(5.0),
        fractional_kelly_pct: Some(0.25),
        recommended_stake_dollars: Some(10.0),
        risk_flags: Some(vec![]),
        thesis: Some("Test thesis".to_string()),
        data_quality: Some("high".to_string()),
        decision: Some("Test decision".to_string()),
        line: Some(275.5),
        actual_stat_value: Some(280.0),
        multiplier: Some(3.0),
    }
}

fn make_predictions(n: usize) -> Vec<PrizePicksPrediction> {
    (0..n).map(|i| {
        let mut p = make_prediction();
        p.id = format!("bench-pred-{}", i);
        p.actual_stat_value = Some(275.5 + (i as f64 * 10.0) % 50.0);
        p.pick_type = if i % 2 == 0 { Some("Over".to_string()) } else { Some("Under".to_string()) };
        p
    }).collect()
}

fn grading_bench(c: &mut Criterion) {
    let pred = make_prediction();
    let preds = make_predictions(100);

    // Core grading function - single pick
    c.bench_function("grade_over_under_single", |b| {
        b.iter(|| {
            let result = grade_over_under(black_box("Over"), black_box(275.5), black_box(280.0));
            black_box(result);
        })
    });

    // Full evaluation of a single prop bet
    c.bench_function("evaluate_prop_bet_single", |b| {
        b.iter(|| {
            let result = evaluate_prop_bet(black_box(&pred));
            black_box(result);
        })
    });

    // Batch evaluation - 100 predictions
    c.bench_function("evaluate_prop_bet_batch_100", |b| {
        b.iter(|| {
            for pred in &preds {
                let result = evaluate_prop_bet(black_box(pred));
                black_box(result);
            }
        })
    });

    // PnL calculation
    c.bench_function("prop_pnl_single", |b| {
        b.iter(|| {
            let result = prop_pnl(black_box(10.0), black_box(&PropGrade::Win), black_box(3.0));
            black_box(result);
        })
    });

    // Multiplier lookup
    c.bench_function("power_play_multiplier", |b| {
        b.iter(|| {
            let result = power_play_multiplier(black_box(4));
            black_box(result);
        })
    });

    // Lineup PnL calculation - Power Play
    let pick_refs: Vec<&PrizePicksPrediction> = preds.iter().take(4).collect();
    c.bench_function("calculate_lineup_pnl_power_play_4", |b| {
        b.iter(|| {
            let result = calculate_lineup_pnl(black_box(&pick_refs), black_box(false));
            black_box(result);
        })
    });

    // Lineup PnL calculation - Flex Play
    c.bench_function("calculate_lineup_pnl_flex_6", |b| {
        b.iter(|| {
            let pick_refs: Vec<&PrizePicksPrediction> = preds.iter().take(6).collect();
            let result = calculate_lineup_pnl(black_box(&pick_refs), black_box(true));
            black_box(result);
        })
    });
}

criterion_group!(grading_benches, grading_bench);
criterion_main!(grading_benches);