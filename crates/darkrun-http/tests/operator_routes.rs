//! In-process route tests for the operator-surface endpoints, area
//! "operator_routes".
//!
//! Drives `build_router` via `tower::ServiceExt::oneshot` (no socket bind) to
//! cover the operator-facing halves the audits found missing:
//!   - `POST /review/:id/decide` accepting BOTH body shapes (the canonical
//!     `{"decision": "..."}` and the `{"approved": bool}` alias agents send)
//!   - `POST /api/runs/:slug/archive` (reversible; archived runs drop out of
//!     the default list; the active pointer clears)
//!   - `GET  /api/feedback/:run/:station` carrying each item's reply thread
//!   - `GET  /api/runs` carrying the per-run `open_drift` count

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use darkrun_api::{ReviewSessionPayload, SessionPayload, SessionStatus};
use darkrun_core::domain::{Run, RunFrontmatter, Status};
use darkrun_core::StateStore;
use darkrun_http::{build_router, AppState, Limits};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

// ── Fixtures ────────────────────────────────────────────────────────────────

/// A state plus the store behind it, sharing one leaked tempdir so the on-disk
/// runs survive for the whole test.
fn state_with_store() -> (AppState, StateStore) {
    let tmp = tempfile::tempdir().expect("tmp");
    let path = tmp.path().to_path_buf();
    std::mem::forget(tmp);
    let store = StateStore::new(&path);
    (AppState::new(StateStore::new(&path), Limits::default()), store)
}

fn seed_run(store: &StateStore, slug: &str, archived: bool) {
    let run = Run {
        slug: slug.to_string(),
        frontmatter: RunFrontmatter {
            title: Some(slug.to_string()),
            factory: "software".to_string(),
            mode: darkrun_core::domain::Mode::Solo,
            active_station: "build".to_string(),
            status: Status::Active,
            archived: if archived { Some(true) } else { None },
            ..Default::default()
        },
        title: slug.to_string(),
        body: format!("# {slug}\n"),
    };
    store.write_run(&run).expect("write run");
}

fn review(session_id: &str) -> SessionPayload {
    SessionPayload::Review(ReviewSessionPayload {
        session_id: session_id.into(),
        status: SessionStatus::Pending,
        station: Some("build".into()),
        await_active: Some(true),
        ..Default::default()
    })
}

/// A feedback sidecar in the on-disk frontmatter format, with a reply thread.
fn drift_doc(id: &str, status: &str, replies: &[&str]) -> String {
    let mut out = format!(
        "---\nid: {id}\nstation: build\nstatus: {status}\norigin: drift\n\
         title: premise moved\nauthor: system\ncreated_at: 2026-07-01T00:00:00Z\n\
         visit: 1\nreplies:\n"
    );
    for r in replies {
        out.push_str(&format!("  - \"{r}\"\n"));
    }
    out.push_str("---\nThe input premise changed.\n");
    out
}

// ── Request helpers ─────────────────────────────────────────────────────────

async fn send(app: axum::Router, req: Request<Body>) -> axum::response::Response {
    app.oneshot(req).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn post_json(uri: &str, v: &Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(v).unwrap()))
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ════════════════════════════════════════════════════════════════════════════
// POST /review/:id/decide: the {"approved": bool} alias
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn decide_accepts_the_approved_true_alias() {
    let (state, _store) = state_with_store();
    state.sessions.upsert(review("al-1"));
    let resp = send(
        build_router(state),
        post_json("/review/al-1/decide", &json!({ "approved": true })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["decision"], "approved");
}

#[tokio::test]
async fn decide_alias_false_maps_to_changes_requested_with_feedback() {
    let (state, _store) = state_with_store();
    state.sessions.upsert(review("al-2"));
    let resp = send(
        build_router(state),
        post_json(
            "/review/al-2/decide",
            &json!({ "approved": false, "feedback": "tighten the spec" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["decision"], "changes_requested");
    assert_eq!(body["feedback"], "tighten the spec");
}

#[tokio::test]
async fn decide_canonical_shape_still_works() {
    let (state, _store) = state_with_store();
    state.sessions.upsert(review("al-3"));
    let resp = send(
        build_router(state),
        post_json("/review/al-3/decide", &json!({ "decision": "approved" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["decision"], "approved");
}

#[tokio::test]
async fn decide_rejects_a_body_matching_neither_shape() {
    let (state, _store) = state_with_store();
    state.sessions.upsert(review("al-4"));
    let resp = send(
        build_router(state),
        post_json("/review/al-4/decide", &json!({ "approved": "yes" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ════════════════════════════════════════════════════════════════════════════
// POST /api/runs/:slug/archive
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn archive_drops_the_run_from_the_default_list_and_restores() {
    let (state, store) = state_with_store();
    seed_run(&store, "alpha", false);
    seed_run(&store, "beta", false);

    // Archive (an empty body defaults to archived: true).
    let resp = send(
        build_router(state.clone()),
        post_json("/api/runs/alpha/archive", &json!({})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["slug"], "alpha");
    assert_eq!(body["archived"], true);

    // The flag landed on disk and the default list omits the run.
    assert_eq!(store.read_run("alpha").unwrap().frontmatter.archived, Some(true));
    let list = body_json(send(build_router(state.clone()), get("/api/runs")).await).await;
    let slugs: Vec<&str> = list["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, vec!["beta"]);

    // Restore is the same route with the flag flipped: reversible, no confirm.
    let resp = send(
        build_router(state.clone()),
        post_json("/api/runs/alpha/archive", &json!({ "archived": false })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(send(build_router(state), get("/api/runs")).await).await;
    assert_eq!(list["count"], 2);
}

#[tokio::test]
async fn archiving_the_active_run_clears_the_pointer() {
    let (state, store) = state_with_store();
    seed_run(&store, "gamma", false);
    store.set_active_run("gamma").expect("set active");

    let resp = send(
        build_router(state),
        post_json("/api/runs/gamma/archive", &json!({ "archived": true })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(store.active_run().unwrap(), None);
}

#[tokio::test]
async fn archive_unknown_run_is_404() {
    let (state, _store) = state_with_store();
    let resp = send(
        build_router(state),
        post_json("/api/runs/ghost/archive", &json!({})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ════════════════════════════════════════════════════════════════════════════
// GET /api/feedback/:run/:station: the reply thread rides the list
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn feedback_list_carries_each_items_replies() {
    let (state, store) = state_with_store();
    seed_run(&store, "r-replies", false);
    store
        .write_feedback_raw(
            "r-replies",
            "FB-01",
            &drift_doc("FB-01", "pending", &["agent: applied the fix", "user: thanks"]),
        )
        .expect("write feedback");

    let resp = send(
        build_router(state),
        get("/api/feedback/r-replies/build"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let replies = body["items"][0]["replies"].as_array().expect("replies present");
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["author"], "agent");
    assert_eq!(replies[0]["author_type"], "agent");
    assert_eq!(replies[0]["body"], "applied the fix");
    assert_eq!(replies[1]["author"], "user");
    assert_eq!(replies[1]["author_type"], "human");
}

#[tokio::test]
async fn a_posted_reply_round_trips_into_the_list() {
    let (state, store) = state_with_store();
    seed_run(&store, "r-rt", false);
    store
        .write_feedback_raw("r-rt", "FB-01", &drift_doc("FB-01", "pending", &[]))
        .expect("write feedback");

    let resp = send(
        build_router(state.clone()),
        post_json(
            "/api/feedback/r-rt/build/FB-01/replies",
            &json!({ "body": "please re-measure" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body = body_json(send(build_router(state), get("/api/feedback/r-rt/build")).await).await;
    let replies = body["items"][0]["replies"].as_array().expect("replies present");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["author"], "user");
    assert_eq!(replies[0]["body"], "please re-measure");
}

// ════════════════════════════════════════════════════════════════════════════
// GET /api/runs: the open_drift count
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_list_counts_only_open_drift_feedback() {
    let (state, store) = state_with_store();
    seed_run(&store, "drifty", false);
    // Two open drift items, one closed drift item, one open NON-drift item:
    // only the open drift pair counts.
    store
        .write_feedback_raw("drifty", "FB-01", &drift_doc("FB-01", "pending", &[]))
        .unwrap();
    store
        .write_feedback_raw("drifty", "FB-02", &drift_doc("FB-02", "fixing", &[]))
        .unwrap();
    store
        .write_feedback_raw("drifty", "FB-03", &drift_doc("FB-03", "closed", &[]))
        .unwrap();
    store
        .write_feedback_raw(
            "drifty",
            "FB-04",
            "---\nid: FB-04\nstation: build\nstatus: pending\norigin: user-visual\n\
             title: not drift\nauthor: user\ncreated_at: x\nvisit: 0\nreplies:\n---\nbody\n",
        )
        .unwrap();

    let body = body_json(send(build_router(state), get("/api/runs")).await).await;
    assert_eq!(body["runs"][0]["slug"], "drifty");
    assert_eq!(body["runs"][0]["open_drift"], 2);
}

#[tokio::test]
async fn run_list_open_drift_is_zero_without_drift_feedback() {
    let (state, store) = state_with_store();
    seed_run(&store, "calm", false);
    let body = body_json(send(build_router(state), get("/api/runs")).await).await;
    assert_eq!(body["runs"][0]["open_drift"], 0);
}

// ════════════════════════════════════════════════════════════════════════════
// POST /api/annotation/:run/:id/resolve: the desktop's annotation-close verb
// ════════════════════════════════════════════════════════════════════════════

/// A minimal OPEN `must` annotation on `build`/`payment` — the kind that blocks a
/// clean Approve until it's resolved.
fn open_must_annotation(id: &str) -> darkrun_api::annotation::Annotation {
    serde_json::from_value(json!({
        "id": id,
        "created_at": "2026-07-01T00:00:00Z",
        "author": "human",
        "work_item": { "kind": "output", "id": "payment", "station": "build" },
        "artifact": null,
        "anchor": null,
        "expression": null,
        "comment": "fix the total",
        "ask": { "kind": "change", "severity": "must" },
        "suggestion": null,
        "status": "open"
    }))
    .expect("minimal annotation")
}

#[tokio::test]
async fn resolve_annotation_closes_a_blocking_annotation() {
    use darkrun_api::annotation::AnnotationStatus;
    let (state, store) = state_with_store();
    store.write_annotation("ann-run", &open_must_annotation("anno_1")).unwrap();

    // Default status (`addressed`) — no body key needed.
    let resp = send(
        build_router(state),
        post_json("/api/annotation/ann-run/anno_1/resolve", &json!({})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["annotation"]["status"], "addressed");
    // The status landed on disk, so the severity gate no longer counts it as open.
    let back = store.read_annotation("ann-run", "anno_1").unwrap().unwrap();
    assert_eq!(back.status, AnnotationStatus::Addressed);
}

#[tokio::test]
async fn resolve_annotation_dismiss_and_bad_status_and_404() {
    use darkrun_api::annotation::AnnotationStatus;
    let (state, store) = state_with_store();
    store.write_annotation("ann-run", &open_must_annotation("anno_2")).unwrap();

    // `dismissed` is the no-code-change resolution.
    let resp = send(
        build_router(state.clone()),
        post_json("/api/annotation/ann-run/anno_2/resolve", &json!({ "status": "dismissed" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        store.read_annotation("ann-run", "anno_2").unwrap().unwrap().status,
        AnnotationStatus::Dismissed
    );

    // An unknown resolution is a 400 (not a silent no-op).
    store.write_annotation("ann-run", &open_must_annotation("anno_3")).unwrap();
    let resp = send(
        build_router(state.clone()),
        post_json("/api/annotation/ann-run/anno_3/resolve", &json!({ "status": "banana" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // An unknown annotation is a 404.
    let resp = send(
        build_router(state),
        post_json("/api/annotation/ann-run/ghost/resolve", &json!({})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
