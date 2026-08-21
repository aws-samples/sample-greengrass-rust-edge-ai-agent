//! Criterion benchmark: inference latency (NFR-2).
//!
//! Run with `cargo bench` (uses the tiny fixture) or point at the real
//! model via MODEL_PATH/MODEL_SHA256 env vars:
//!   MODEL_PATH=../model/sample_model.onnx \
//!   MODEL_SHA256=$(cat ../model/model_hash.sha256) cargo bench
//!
//! Criterion reports p50/p95/p99 in the HTML report (target/criterion).
//! Peak-RSS measurement lives in scripts/run_benchmarks.sh, not here —
//! Criterion measures time, not memory.

use criterion::{criterion_group, criterion_main, Criterion};
use edge_ai_classifier::inference::classifier::Classifier;
use edge_ai_classifier::inference::model_loader::{load_verified_model, sha256_hex};
use edge_ai_classifier::inference::types::WINDOW_SIZE;
use std::hint::black_box;
use std::path::PathBuf;

fn model_under_test() -> (PathBuf, String) {
    if let Ok(path) = std::env::var("MODEL_PATH") {
        let hash = std::env::var("MODEL_SHA256")
            .map(|h| h.trim().to_string())
            .unwrap_or_else(|_| sha256_hex(&std::fs::read(&path).expect("MODEL_PATH readable")));
        return (PathBuf::from(path), hash);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny_model.onnx");
    let hash = sha256_hex(&std::fs::read(&path).expect("fixture present"));
    (path, hash)
}

fn bench_inference(c: &mut Criterion) {
    let (path, hash) = model_under_test();
    let session = load_verified_model(&path, &hash).expect("model loads");
    let mut classifier = Classifier::new(session);

    // Realistic-looking window: baselines + small ripple.
    let mut window = [0.0f32; WINDOW_SIZE];
    let baselines = [100.0f32, 50.0, 1.0, 20.0];
    for (i, slot) in window.iter_mut().enumerate() {
        let channel = i / 60;
        let t = (i % 60) as f32;
        *slot = baselines[channel] + (t * 0.7).sin() * 0.1 * baselines[channel].max(1.0) * 0.02;
    }

    c.bench_function("classify_window", |b| {
        b.iter(|| classifier.classify(black_box(&window)).unwrap())
    });
}

criterion_group!(benches, bench_inference);
criterion_main!(benches);
