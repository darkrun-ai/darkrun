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
use serde::Serialize;

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
    /// The fraction of requests (0.0..=1.0) allowed to fail before the whole run
    /// is treated as a failure. A load run against a mostly-broken target must
    /// NOT certify as healthy, so above this rate [`load_http`] errors instead of
    /// returning a proof.
    pub max_failure_rate: f64,
}

impl Default for LoadOpts {
    fn default() -> Self {
        LoadOpts {
            requests: 100,
            concurrency: 8,
            timeout: Duration::from_secs(10),
            // Half the requests failing is already a broken target; refuse to
            // certify anything worse.
            max_failure_rate: 0.5,
        }
    }
}

/// A completed HTTP load run: the reduced [`BenchProof`] plus the success /
/// failure accounting the proof itself does not carry. The proof's percentiles
/// alone can flatter a broken target (fast-failing requests look fast), so the
/// counts travel alongside it and a high failure rate fails the run outright.
#[derive(Debug, Clone, Serialize)]
pub struct LoadReport {
    /// Total requests attempted.
    pub requests: u64,
    /// Requests that returned a 2xx response.
    pub ok: u64,
    /// Requests that failed (non-2xx, connection error, timeout, or dropped).
    pub failed: u64,
    /// Latency percentiles + throughput over the SUCCESSFUL requests only, so a
    /// flood of fast-failing requests can't deflate the numbers into looking
    /// healthy.
    pub proof: BenchProof,
}

impl LoadReport {
    /// The fraction of requests that failed (0.0..=1.0). Zero requests → 0.0.
    pub fn failure_rate(&self) -> f64 {
        if self.requests == 0 {
            0.0
        } else {
            self.failed as f64 / self.requests as f64
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

/// Reduce the raw outcome of a load run into a [`LoadReport`], FAILING when too
/// many requests failed. `success_samples_ms` are the latencies of the OK
/// requests only (the ones the percentiles describe). Anything not counted in
/// `ok` is a failure, so `failed = requests - ok`.
///
/// **Pure** (no network, no clock), so the honesty rule (a mostly-failing
/// target can never certify as healthy) is unit-tested directly. Fails with no
/// report when every request failed, or when the failure rate exceeds
/// `max_failure_rate`.
pub fn assess(
    requests: u64,
    ok: u64,
    success_samples_ms: &[f64],
    wall: Duration,
    max_failure_rate: f64,
) -> Result<LoadReport> {
    let failed = requests.saturating_sub(ok);
    let failure_rate = if requests == 0 {
        0.0
    } else {
        failed as f64 / requests as f64
    };
    // No successes at all is always a failure: there are no numbers to stand
    // behind, regardless of the configured threshold.
    if ok == 0 {
        return Err(VerifyError::Load(format!("all {requests} requests failed")));
    }
    if failure_rate > max_failure_rate {
        return Err(VerifyError::Load(format!(
            "{failed}/{requests} requests failed ({:.0}% > {:.0}% allowed)",
            failure_rate * 100.0,
            max_failure_rate * 100.0
        )));
    }
    Ok(LoadReport {
        requests,
        ok,
        failed,
        proof: summarize(success_samples_ms, wall),
    })
}

/// Fire an HTTP load run against `url` and reduce it into a [`LoadReport`].
///
/// Issues `opts.requests` GETs with at most `opts.concurrency` in flight and
/// times each. Only SUCCESSFUL (2xx) requests feed the percentiles: a failed
/// request (connection refused, non-2xx, timeout) returns fast and would
/// otherwise deflate the numbers into looking healthy, so it is counted as a
/// failure instead of being averaged into the latency. The run errors (no
/// report) when the failure rate exceeds `opts.max_failure_rate`.
pub async fn load_http(url: &str, opts: &LoadOpts) -> Result<LoadReport> {
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

    // Only the successful requests' latencies feed the percentiles; every other
    // outcome (failed response, dropped task) is a failure via `requests - ok`.
    let mut success_samples = Vec::with_capacity(opts.requests as usize);
    let mut ok = 0u64;
    for t in tasks {
        if let Ok(Some((ms, true))) = t.await {
            ok += 1;
            success_samples.push(ms);
        }
    }
    let wall = start.elapsed();

    assess(opts.requests, ok, &success_samples, wall, opts.max_failure_rate)
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

    #[test]
    fn assess_refuses_a_mostly_failing_target() {
        // The dishonest case: 1 of 100 requests succeeded. The old harness handed
        // back a healthy-looking proof; `assess` must refuse it.
        let err = assess(100, 1, &[2.0], Duration::from_secs(1), 0.5).unwrap_err();
        assert!(matches!(err, VerifyError::Load(_)));
        assert!(err.to_string().contains("99/100"), "surfaces the counts: {err}");
    }

    #[test]
    fn assess_refuses_when_every_request_failed() {
        let err = assess(20, 0, &[], Duration::from_secs(1), 0.9).unwrap_err();
        assert!(err.to_string().contains("all 20 requests failed"), "got {err}");
    }

    #[test]
    fn assess_passes_within_threshold_and_records_the_counts() {
        // 8 of 10 succeeded (20% failure, under the 50% bar): a report, with the
        // success/failure accounting surfaced and percentiles over the OK ones.
        let samples = vec![5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        let report = assess(10, 8, &samples, Duration::from_secs(1), 0.5).unwrap();
        assert_eq!(report.requests, 10);
        assert_eq!(report.ok, 8);
        assert_eq!(report.failed, 2);
        assert_eq!(report.failure_rate(), 0.2);
        // Percentiles describe the 8 successes only.
        assert_eq!(report.proof.samples, Some(8));
        assert!(report.proof.p95.is_some());
    }

    #[test]
    fn assess_serializes_the_counts_alongside_the_proof() {
        // The counts must be visible in the emitted JSON (they are what the
        // proof block alone omits).
        let report = assess(4, 4, &[1.0, 2.0, 3.0, 4.0], Duration::from_secs(1), 0.5).unwrap();
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["requests"], 4);
        assert_eq!(json["ok"], 4);
        assert_eq!(json["failed"], 0);
        assert_eq!(json["proof"]["samples"], 4);
    }
}
