//! The bench backend — objective NUMBERS for the bench surfaces
//! (library / api / data).
//!
//! Two pieces:
//!
//! - A tiny **HTTP load harness** ([`load_http`]): fire `N` requests at a target
//!   with a bounded concurrency, time each one, and reduce the samples into
//!   p50/p95/p99 + throughput — a [`BenchProof`](darkrun_api::BenchProof).
//! - Pure **percentile reduction** ([`summarize`]): turns a slice of latency
//!   samples into a `BenchProof`. No network, no clock — unit-tested directly.
//!
//! A criterion microbench harness for *library* surfaces lives in
//! `benches/load_harness.rs`; this module supplies the load side.

use std::time::{Duration, Instant};

use darkrun_api::{BenchProof, Proof, Surface};

use crate::error::{Result, VerifyError};

/// Options for an HTTP load run.
#[derive(Debug, Clone)]
pub struct LoadOpts {
    /// Total number of requests to issue.
    pub requests: u64,
    /// Maximum in-flight requests at once.
    pub concurrency: usize,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl Default for LoadOpts {
    fn default() -> Self {
        LoadOpts {
            requests: 100,
            concurrency: 8,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Reduce a set of latency samples (milliseconds) plus the wall-clock window
/// they were gathered over into a [`BenchProof`].
///
/// Percentiles use the nearest-rank method on the sorted samples. Throughput is
/// `samples / wall_secs` (operations per second). **Pure** — given the same
/// inputs it always returns the same proof.
pub fn summarize(samples_ms: &[f64], wall: Duration) -> BenchProof {
    if samples_ms.is_empty() {
        return BenchProof::default();
    }
    let mut sorted: Vec<f64> = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n = sorted.len();
    let throughput = {
        let secs = wall.as_secs_f64();
        if secs > 0.0 {
            Some(n as f64 / secs)
        } else {
            None
        }
    };

    BenchProof {
        p50: Some(percentile(&sorted, 50.0)),
        p95: Some(percentile(&sorted, 95.0)),
        p99: Some(percentile(&sorted, 99.0)),
        throughput,
        samples: Some(n as u64),
    }
}

/// Nearest-rank percentile over an already-sorted, non-empty slice.
fn percentile(sorted: &[f64], pct: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    // Nearest-rank: rank = ceil(pct/100 * n), clamped to [1, n].
    let rank = ((pct / 100.0) * n as f64).ceil() as usize;
    let idx = rank.clamp(1, n) - 1;
    sorted[idx]
}

/// Wrap a [`BenchProof`] in a surface-tagged [`Proof`] for a bench surface.
pub fn bench_proof_into(proof: BenchProof, surface: Surface) -> Proof {
    Proof::bench(surface, proof)
}

/// Fire an HTTP load run against `url` and reduce it into a [`BenchProof`].
///
/// Issues `opts.requests` GETs with at most `opts.concurrency` in flight, times
/// each, and summarizes. A request that fails (connection refused, non-2xx,
/// timeout) is recorded as its elapsed time so a degraded target still produces
/// numbers; if *every* request fails the run errors.
pub async fn load_http(url: &str, opts: &LoadOpts) -> Result<BenchProof> {
    if opts.requests == 0 {
        return Err(VerifyError::Load("requests must be > 0".into()));
    }
    let concurrency = opts.concurrency.max(1);

    let client = reqwest::Client::builder()
        .timeout(opts.timeout)
        .build()
        .map_err(|e| VerifyError::Load(e.to_string()))?;

    let url = url.to_string();
    let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let start = Instant::now();

    let mut tasks = Vec::with_capacity(opts.requests as usize);
    for _ in 0..opts.requests {
        let client = client.clone();
        let url = url.clone();
        let permits = permits.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = permits.acquire().await.ok()?;
            let t0 = Instant::now();
            let resp = client.get(&url).send().await;
            let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
            match resp {
                Ok(r) if r.status().is_success() => {
                    // Drain the body so timing reflects a full response.
                    let _ = r.bytes().await;
                    Some((elapsed, true))
                }
                Ok(_) => Some((elapsed, false)),
                Err(_) => Some((elapsed, false)),
            }
        }));
    }

    let mut samples = Vec::with_capacity(opts.requests as usize);
    let mut ok = 0u64;
    for t in tasks {
        if let Ok(Some((ms, success))) = t.await {
            samples.push(ms);
            if success {
                ok += 1;
            }
        }
    }
    let wall = start.elapsed();

    if ok == 0 {
        return Err(VerifyError::Load(format!(
            "all {} requests to {url} failed",
            opts.requests
        )));
    }
    Ok(summarize(&samples, wall))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank_on_known_set() {
        // 1..=10 sorted. p50 -> rank ceil(5)=5 -> value 5; p95 -> rank 10 -> 10.
        let s: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        assert_eq!(percentile(&s, 50.0), 5.0);
        assert_eq!(percentile(&s, 90.0), 9.0);
        assert_eq!(percentile(&s, 95.0), 10.0);
        assert_eq!(percentile(&s, 99.0), 10.0);
        assert_eq!(percentile(&s, 100.0), 10.0);
    }

    #[test]
    fn percentile_single_sample() {
        assert_eq!(percentile(&[42.0], 50.0), 42.0);
        assert_eq!(percentile(&[42.0], 99.0), 42.0);
    }

    #[test]
    fn summarize_orders_percentiles_and_counts_samples() {
        let samples = vec![10.0, 5.0, 30.0, 20.0, 15.0];
        let proof = summarize(&samples, Duration::from_secs(1));
        assert_eq!(proof.samples, Some(5));
        let p50 = proof.p50.unwrap();
        let p95 = proof.p95.unwrap();
        let p99 = proof.p99.unwrap();
        assert!(p50 <= p95 && p95 <= p99, "percentiles ordered: {p50} {p95} {p99}");
        // 5 samples in 1s -> 5 ops/s.
        assert_eq!(proof.throughput, Some(5.0));
    }

    #[test]
    fn summarize_empty_is_default() {
        let proof = summarize(&[], Duration::from_secs(1));
        assert!(proof.p50.is_none());
        assert!(proof.samples.is_none());
        assert!(proof.throughput.is_none());
    }

    #[test]
    fn summarize_zero_wall_omits_throughput() {
        let proof = summarize(&[1.0, 2.0], Duration::ZERO);
        assert_eq!(proof.samples, Some(2));
        assert!(proof.throughput.is_none(), "no throughput without a time window");
    }

    #[test]
    fn bench_proof_into_tags_surface_and_matches_route() {
        let proof = bench_proof_into(summarize(&[1.0, 2.0, 3.0], Duration::from_secs(1)), Surface::Api);
        assert_eq!(proof.surface, Surface::Api);
        assert!(proof.block_matches_surface());
        assert!(proof.bench.is_some());
        assert!(proof.web.is_none());
    }

    #[tokio::test]
    async fn load_http_rejects_zero_requests() {
        let err = load_http("http://127.0.0.1:1", &LoadOpts { requests: 0, ..Default::default() })
            .await
            .unwrap_err();
        assert!(matches!(err, VerifyError::Load(_)));
    }
}
