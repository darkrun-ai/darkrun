//! Criterion microbench smoke for the bench backend.
//!
//! This is the *library*-surface verification path: a criterion harness that
//! measures the pure percentile reduction ([`darkrun_verify::summarize`]) — the
//! hot path that turns raw latency samples into a `BenchProof`. It doubles as a
//! smoke test that the criterion harness wires up and runs.

use std::time::Duration;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use darkrun_verify::summarize;

fn bench_summarize(c: &mut Criterion) {
    // A representative latency sample set.
    let samples: Vec<f64> = (0..1_000).map(|i| (i % 50) as f64 + 0.5).collect();
    c.bench_function("summarize_1k_samples", |b| {
        b.iter(|| summarize(black_box(&samples), black_box(Duration::from_secs(1))))
    });
}

criterion_group!(benches, bench_summarize);
criterion_main!(benches);
