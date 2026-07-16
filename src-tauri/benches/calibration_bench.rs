use criterion::{black_box, criterion_group, criterion_main, Criterion};
use edge_eval::Calibrator;

/// Load the calibrator that ships alongside the analysis module.
///
/// The calibrator was checked into `src-tauri/src/analysis/calibrator.json`
/// after the shared `monster-edge-core` crate was inlined into the project.
/// Update the path below if the file moves.
fn load_calibrator() -> Calibrator {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/analysis/calibrator.json"
    );
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "failed to read calibrator at {}: {} \
             (hint: calibrator.json must be present in src/analysis/)",
            path, e
        )
    });
    serde_json::from_str(&raw).expect("calibrator.json must deserialize into Calibrator")
}

fn calibration_bench(c: &mut Criterion) {
    let calibrator = load_calibrator();

    // Single calibration - hot path: each paper-lot render touches the calibrator
    // once to convert a raw model probability into a calibrated edge estimate.
    c.bench_function("calibrator_apply_single", |b| {
        b.iter(|| {
            let result = calibrator.apply(black_box(0.55));
            black_box(result);
        })
    });

    // Batch calibration - 100 probabilities: represents a single refresh of the
    // predictions panel where every visible pick is recalibrated in one pass.
    c.bench_function("calibrator_apply_batch_100", |b| {
        b.iter(|| {
            for i in 0..100 {
                let p = i as f64 / 100.0;
                let result = calibrator.apply(black_box(p));
                black_box(result);
            }
        })
    });

    // Batch calibration - 1000 probabilities: stress test for the calibrator's
    // hot path. Useful when the calibration metrics view recomputes the
    // reliability scatter across the full 0..1 grid.
    c.bench_function("calibrator_apply_batch_1000", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let p = i as f64 / 1000.0;
                let result = calibrator.apply(black_box(p));
                black_box(result);
            }
        })
    });
}

criterion_group!(calibration_benches, calibration_bench);
criterion_main!(calibration_benches);
