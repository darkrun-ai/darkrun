//! In-process axum tests for the darkrun-http server.
//!
//! Drives the router directly via `tower::ServiceExt::oneshot` (no socket
//! bind) for status/payload/middleware coverage, and over a real loopback
//! bind for the end-to-end + WebSocket smoke checks.

use std::net::SocketAddr;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use darkrun_api::{
    ApproveAction, ApproveActionKind, GateType, ReviewSessionPayload, SessionPayload, SessionStatus,
};
use darkrun_core::StateStore;
use darkrun_http::{
    build_router, AppState, Limits, DEFAULT_BODY_MAX_BYTES, DEFAULT_MAX_CONNECTIONS,
    DEFAULT_MAX_WS_SESSIONS, DEFAULT_RATE_LIMIT_PER_MIN,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ── Fixtures ──────────────────────────────────────────────────────────────

fn stub_review(session_id: &str) -> SessionPayload {
    SessionPayload::Review(ReviewSessionPayload {
        session_id: session_id.into(),
        status: SessionStatus::Pending,
        run_slug: Some("my-run".into()),
        gate_type: Some(GateType::Ask),
        station: Some("frame".into()),
        approve_action: Some(ApproveAction {
            label: "Complete Frame Station".into(),
            kind: ApproveActionKind::CompleteStation,
        }),
        await_active: Some(true),
        ..Default::default()
    })
}

/// Build an `AppState` over a fresh tempdir. Returns the state and the
/// tempdir guard so the caller controls its lifetime.
fn test_state_with_dir() -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tmp");
    let store = StateStore::new(tmp.path());
    (AppState::new(store, Limits::default()), tmp)
}

fn test_state() -> AppState {
    let (state, tmp) = test_state_with_dir();
    // Keep the tempdir alive for the test process lifetime.
    std::mem::forget(tmp);
    state
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

// ── Health ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_200() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

// ── Session ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn session_returns_stub_review_payload() {
    let state = test_state();
    state.sessions.upsert(stub_review("s-1"));
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/session/s-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["session_type"], "review");
    assert_eq!(json["session_id"], "s-1");
    assert_eq!(json["gate_type"], "ask");
}

#[tokio::test]
async fn unknown_session_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/session/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = body_json(resp).await;
    assert_eq!(json["error"], "session not found");
}

#[tokio::test]
async fn heartbeat_reflects_existence() {
    let state = test_state();
    state.sessions.upsert(stub_review("hb"));
    let app = build_router(state);

    let present = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/api/session/hb/heartbeat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(present.status(), StatusCode::OK);

    let absent = app
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/api/session/nope/heartbeat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(absent.status(), StatusCode::NOT_FOUND);
}

// ── Review decide ─────────────────────────────────────────────────────────

#[tokio::test]
async fn review_decide_canonicalizes_and_updates() {
    let state = test_state();
    state.sessions.upsert(stub_review("dec"));
    let app = build_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/review/dec/decide")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "decision": "APPROVED",
                        "feedback": "looks good"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["ok"], true);
    assert_eq!(json["decision"], "approved");
    assert_eq!(json["feedback"], "looks good");

    let SessionPayload::Review(updated) = state.sessions.get("dec").unwrap() else {
        panic!("expected review session");
    };
    assert_eq!(updated.status, SessionStatus::Approved);
    assert_eq!(updated.decision.as_deref(), Some("approved"));
}

#[tokio::test]
async fn review_decide_non_approved_requests_changes() {
    let state = test_state();
    state.sessions.upsert(stub_review("dec2"));
    let app = build_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/review/dec2/decide")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "decision": "nope" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["decision"], "changes_requested");
    // No feedback supplied → echoed as an empty string.
    assert_eq!(json["feedback"], "");

    let SessionPayload::Review(updated) = state.sessions.get("dec2").unwrap() else {
        panic!("expected review session");
    };
    assert_eq!(updated.status, SessionStatus::ChangesRequested);
}

#[tokio::test]
async fn review_decide_unknown_session_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/review/ghost/decide")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "decision": "approved" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn review_decide_malformed_json_is_rejected() {
    let state = test_state();
    state.sessions.upsert(stub_review("bad"));
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/review/bad/decide")
                .header("content-type", "application/json")
                .body(Body::from("{ not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    // axum's Json extractor rejects a malformed body with 400.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Advance ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn advance_marks_decided() {
    let state = test_state();
    state.sessions.upsert(stub_review("adv"));
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/advance/adv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["advanced"], true);

    let SessionPayload::Review(updated) = state.sessions.get("adv").unwrap() else {
        panic!("expected review session");
    };
    assert_eq!(updated.status, SessionStatus::Decided);
}

#[tokio::test]
async fn advance_unknown_session_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/advance/ghost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Feedback: list ────────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_list_is_empty_for_unknown_run() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/feedback/some-run/frame")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["run"], "some-run");
    assert_eq!(json["station"], "frame");
    assert_eq!(json["count"], 0);
    assert_eq!(json["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn feedback_list_filters_by_station() {
    let state = test_state();
    state
        .store
        .write_feedback_raw(
            "r1",
            "FB-01",
            "---\nid: FB-01\nstation: frame\nstatus: pending\ntitle: A\n---\nbody a",
        )
        .unwrap();
    state
        .store
        .write_feedback_raw(
            "r1",
            "FB-02",
            "---\nid: FB-02\nstation: build\nstatus: pending\ntitle: B\n---\nbody b",
        )
        .unwrap();
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/feedback/r1/frame")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    // Only the frame item comes back; the build item is filtered out.
    assert_eq!(json["count"], 1);
    assert_eq!(json["items"][0]["feedback_id"], "FB-01");
    assert_eq!(json["items"][0]["title"], "A");
    assert_eq!(json["items"][0]["status"], "pending");
}

// ── Feedback: create ──────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_create_mints_id_and_persists() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/feedback/run-x/frame")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "title": "Tighten the spec",
                        "body": "needs acceptance criteria",
                        "author": "attacker"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp).await;
    assert_eq!(json["feedback_id"], "FB-01");
    assert_eq!(json["status"], "pending");
    assert_eq!(json["file"], "feedback/FB-01.md");

    // Persisted on disk; author forced to `user`, not the client value.
    let raw = state.store.read_feedback_raw("run-x").unwrap();
    let doc = raw.get("FB-01").unwrap();
    assert!(doc.contains("author: user"));
    assert!(!doc.contains("attacker"));
    assert!(doc.contains("station: frame"));
}

#[tokio::test]
async fn feedback_create_rejects_empty_fields() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/feedback/run-x/frame")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "title": "", "body": "" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn feedback_create_then_list_roundtrips() {
    let state = test_state();
    let app = build_router(state);

    for (i, title) in ["first", "second"].iter().enumerate() {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/feedback/rr/frame")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "title": title,
                            "body": "b"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["feedback_id"], format!("FB-{:02}", i + 1));
    }

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/feedback/rr/frame")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["count"], 2);
}

// ── Feedback: update ──────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_update_changes_status() {
    let state = test_state();
    state
        .store
        .write_feedback_raw(
            "ru",
            "FB-01",
            "---\nid: FB-01\nstation: frame\nstatus: pending\ntitle: T\n---\nbody",
        )
        .unwrap();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/feedback/ru/frame/FB-01")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "status": "closed",
                        "closed_by": "unit-03"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let fields = json["updated_fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f == "status"));
    assert!(fields.iter().any(|f| f == "closed_by"));

    let doc = state.store.read_feedback_raw("ru").unwrap();
    let content = doc.get("FB-01").unwrap();
    assert!(content.contains("status: closed"));
    assert!(content.contains("closed_by: unit-03"));
}

#[tokio::test]
async fn feedback_update_empty_body_is_400() {
    let state = test_state();
    state
        .store
        .write_feedback_raw("ru", "FB-01", "---\nstatus: pending\n---\nx")
        .unwrap();
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/feedback/ru/frame/FB-01")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&serde_json::json!({})).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn feedback_update_unknown_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/feedback/ru/frame/FB-99")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "status": "closed" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Feedback: delete ──────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_delete_closed_item_succeeds() {
    let state = test_state();
    state
        .store
        .write_feedback_raw(
            "rd",
            "FB-01",
            "---\nid: FB-01\nstation: frame\nstatus: closed\ntitle: T\n---\nx",
        )
        .unwrap();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/feedback/rd/frame/FB-01")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["deleted"], true);
    assert!(state.store.read_feedback_raw("rd").unwrap().is_empty());
}

#[tokio::test]
async fn feedback_delete_open_item_is_409() {
    let state = test_state();
    state
        .store
        .write_feedback_raw(
            "rd",
            "FB-01",
            "---\nid: FB-01\nstation: frame\nstatus: pending\ntitle: T\n---\nx",
        )
        .unwrap();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/feedback/rd/frame/FB-01")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    // Still on disk.
    assert!(state.store.read_feedback_raw("rd").unwrap().contains_key("FB-01"));
}

#[tokio::test]
async fn feedback_delete_unknown_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/feedback/rd/frame/FB-77")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Feedback: replies ─────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_reply_appends_and_can_close() {
    let state = test_state();
    state
        .store
        .write_feedback_raw(
            "rp",
            "FB-01",
            "---\nid: FB-01\nstation: frame\nstatus: pending\ntitle: T\n---\nx",
        )
        .unwrap();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/feedback/rp/frame/FB-01/replies")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "body": "fixed in the latest pass",
                        "close_as_answered": true
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp).await;
    assert_eq!(json["reply_index"], 0);
    assert_eq!(json["status"], "answered");

    let content = state.store.read_feedback_raw("rp").unwrap();
    let doc = content.get("FB-01").unwrap();
    assert!(doc.contains("status: answered"));
    assert!(doc.contains("fixed in the latest pass"));
}

#[tokio::test]
async fn feedback_reply_empty_body_is_400() {
    let state = test_state();
    state
        .store
        .write_feedback_raw("rp", "FB-01", "---\nstatus: pending\n---\nx")
        .unwrap();
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/feedback/rp/frame/FB-01/replies")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "body": "" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn feedback_reply_unknown_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/feedback/rp/frame/FB-99/replies")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "body": "hi" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Routing edges ─────────────────────────────────────────────────────────

#[tokio::test]
async fn unknown_route_is_404() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/nope/nowhere")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn wrong_method_is_405() {
    // /health is GET-only; a POST should be method-not-allowed.
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ── Body limit ────────────────────────────────────────────────────────────

#[tokio::test]
async fn oversize_body_is_413() {
    let state = test_state();
    state.sessions.upsert(stub_review("big"));
    let app = build_router(state);
    let huge = vec![b'a'; DEFAULT_BODY_MAX_BYTES + 1];
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/review/big/decide")
                .header("content-type", "application/json")
                .body(Body::from(huge))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

// ── CORS ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn remote_mode_sets_permissive_cors() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::new(tmp.path());
    let limits = Limits {
        remote: true,
        ..Limits::default()
    };
    let app = build_router(AppState::new(store, limits));

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/session/x")
                .header("origin", "https://example.com")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let allow = resp
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert_eq!(allow, "*");
}

// ── Rate limit ────────────────────────────────────────────────────────────

#[tokio::test]
async fn rate_limit_engages_in_remote_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::new(tmp.path());
    let limits = Limits {
        remote: true,
        rate_limit_per_min: 3,
        ..Limits::default()
    };
    let app = build_router(AppState::new(store, limits));

    // The in-process oneshot path has no ConnectInfo, so every request maps to
    // the fallback IP and shares one counter. The 4th request (cap = 3) is 429.
    let mut statuses = Vec::new();
    for _ in 0..4 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        statuses.push(resp.status());
    }
    assert_eq!(statuses[0], StatusCode::OK);
    assert_eq!(statuses[2], StatusCode::OK);
    assert_eq!(statuses[3], StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn rate_limit_disabled_in_local_mode() {
    // Default Limits → remote = false → no rate limiting.
    let app = build_router(test_state());
    for _ in 0..(DEFAULT_RATE_LIMIT_PER_MIN + 10) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

// ── Connection cap (constants smoke) ──────────────────────────────────────

#[tokio::test]
async fn limit_defaults_are_sane() {
    let limits = Limits::default();
    assert_eq!(limits.max_connections, DEFAULT_MAX_CONNECTIONS);
    assert_eq!(limits.max_ws_sessions, DEFAULT_MAX_WS_SESSIONS);
    assert_eq!(limits.rate_limit_per_min, DEFAULT_RATE_LIMIT_PER_MIN);
    assert!(!limits.remote);
    const { assert!(DEFAULT_MAX_CONNECTIONS > 0) };
}

#[tokio::test]
async fn bind_listener_assigns_an_ephemeral_port() {
    // Port 0 → the kernel picks a free loopback port the embedder can read back
    // before serving (the discovery-descriptor seam).
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = darkrun_http::bind_listener(addr).await.expect("binds");
    let bound = listener.local_addr().unwrap();
    assert!(bound.port() != 0, "an ephemeral port was assigned");
    assert!(bound.ip().is_loopback());
}

// ── End-to-end bound server ───────────────────────────────────────────────

#[tokio::test]
async fn end_to_end_bound_server_serves_health() {
    let tmp = tempfile::tempdir().expect("tmp");
    let store = StateStore::new(tmp.path());
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let bound = listener.local_addr().unwrap();
    let app = build_router(AppState::new(store, Limits::default()));
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let url = format!("http://{bound}/health");
    let resp = raw_http_get(&url).await;
    assert!(resp.contains("\"status\":\"ok\"") || resp.contains("\"status\": \"ok\""));
    handle.abort();
}

// ── WebSocket smoke ───────────────────────────────────────────────────────

#[tokio::test]
async fn ws_session_pushes_snapshot_and_updates() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::new(tmp.path());
    let state = AppState::new(store, Limits::default());
    state.sessions.upsert(stub_review("ws-1"));
    let registry = state.sessions.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let app = build_router(state);
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let url = format!("ws://{bound}/ws/session/ws-1");
    let (mut socket, _resp) = tokio_tungstenite::connect_async(&url).await.unwrap();

    // First frame: the current snapshot.
    let snapshot = next_text(&mut socket).await;
    let json: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(json["session_id"], "ws-1");
    assert_eq!(json["session_type"], "review");

    // Push an update; the socket should receive it.
    let mut updated = stub_review("ws-1");
    if let SessionPayload::Review(ref mut r) = updated {
        r.status = SessionStatus::Approved;
    }
    registry.upsert(updated);

    let frame = next_text(&mut socket).await;
    let json: serde_json::Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(json["session_id"], "ws-1");
    assert_eq!(json["status"], "approved");

    socket.send(Message::Close(None)).await.ok();
    handle.abort();
}

#[tokio::test]
async fn ws_unknown_session_closes() {
    use tokio_tungstenite::tungstenite::Message;

    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::new(tmp.path());
    let state = AppState::new(store, Limits::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let app = build_router(state);
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let url = format!("ws://{bound}/ws/session/ghost");
    let (mut socket, _resp) = tokio_tungstenite::connect_async(&url).await.unwrap();

    use futures_util::StreamExt;
    // The server closes an unknown session almost immediately.
    let mut saw_close = false;
    while let Some(msg) = socket.next().await {
        match msg {
            Ok(Message::Close(_)) => {
                saw_close = true;
                break;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert!(saw_close, "expected a close frame for an unknown session");
    handle.abort();
}

// ── Helpers ───────────────────────────────────────────────────────────────

async fn next_text<S>(socket: &mut S) -> String
where
    S: futures_util::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(t))) => return t.to_string(),
            Some(Ok(_)) => continue,
            other => panic!("expected a text frame, got {other:?}"),
        }
    }
}

/// Minimal HTTP/1.1 GET over a raw TCP socket — avoids pulling a client crate.
async fn raw_http_get(url: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let without_scheme = url.strip_prefix("http://").unwrap();
    let (authority, path) = match without_scheme.split_once('/') {
        Some((a, p)) => (a.to_string(), format!("/{p}")),
        None => (without_scheme.to_string(), "/".to_string()),
    };
    let mut stream = tokio::net::TcpStream::connect(&authority).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).to_string()
}
