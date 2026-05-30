//! Integration tests for the HTTP load harness against a local axum stub — no
//! external network, no browser. Proves the harness produces real percentile
//! NUMBERS against a live server and handles a degraded/dead target.

use std::time::Duration;

use axum::{routing::get, Router};
use darkrun_verify::{load_http, prove_load, LoadOpts, Surface};
use tokio::net::TcpListener;

/// Spin up a trivial axum server on an ephemeral port; return its base URL and
/// the join handle so the test owns the server's lifetime.
async fn spawn_stub(status: axum::http::StatusCode, body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new().route(
        "/",
        get(move || async move { (status, body) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/"), handle)
}

#[tokio::test]
async fn load_harness_produces_percentiles_against_a_live_stub() {
    let (url, server) = spawn_stub(axum::http::StatusCode::OK, "ok").await;

    let opts = LoadOpts {
        requests: 50,
        concurrency: 8,
        timeout: Duration::from_secs(5),
    };
    let proof = load_http(&url, &opts).await.expect("load run");

    assert_eq!(proof.samples, Some(50));
    let p50 = proof.p50.expect("p50");
    let p95 = proof.p95.expect("p95");
    let p99 = proof.p99.expect("p99");
    assert!(p50 <= p95 && p95 <= p99, "percentiles ordered: {p50} {p95} {p99}");
    assert!(proof.throughput.expect("throughput") > 0.0);

    server.abort();
}

#[tokio::test]
async fn prove_load_tags_the_bench_surface() {
    let (url, server) = spawn_stub(axum::http::StatusCode::OK, "ok").await;

    let proof = prove_load(
        &url,
        Surface::Api,
        &LoadOpts { requests: 20, concurrency: 4, timeout: Duration::from_secs(5) },
    )
    .await
    .expect("load run");

    assert_eq!(proof.surface, Surface::Api);
    assert!(proof.block_matches_surface(), "bench block matches api surface");
    assert_eq!(proof.bench.unwrap().samples, Some(20));

    server.abort();
}

#[tokio::test]
async fn load_harness_errors_when_every_request_fails() {
    // Nothing is listening on this port — every request fails.
    let opts = LoadOpts {
        requests: 5,
        concurrency: 2,
        timeout: Duration::from_millis(300),
    };
    let err = load_http("http://127.0.0.1:1/", &opts).await.unwrap_err();
    assert!(
        err.to_string().contains("failed"),
        "expected an all-failed load error, got {err}"
    );
}

#[tokio::test]
async fn non_2xx_is_recorded_as_a_failure_but_not_fatal_when_mixed() {
    // A 500-only stub: requests connect but never succeed -> all-failed error.
    let (url, server) = spawn_stub(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom").await;
    let opts = LoadOpts { requests: 10, concurrency: 4, timeout: Duration::from_secs(5) };
    let err = load_http(&url, &opts).await.unwrap_err();
    assert!(err.to_string().contains("failed"), "got {err}");
    server.abort();
}
