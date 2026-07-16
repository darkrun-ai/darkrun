//! Runs browse handlers — `GET /api/runs`, `GET /api/runs/:slug`, and
//! `POST /api/runs/:slug/archive`.
//!
//! The read handlers are thin adapters over [`darkrun_browse`], which projects
//! the on-disk `.darkrun/` state into the [`darkrun_api`] browse payloads off a
//! bare [`darkrun_core::StateStore`] with no engine dependency — the SAME reader
//! the desktop uses to render a run offline, so every surface agrees:
//!
//! - `GET /api/runs` — [`darkrun_browse::list_runs`] wrapped in `Json`.
//! - `GET /api/runs/:slug` — [`darkrun_browse::run_detail`]; `404` when unknown.
//! - `POST /api/runs/:slug/archive` sets (or clears) a run's archived flag,
//!   mirroring the engine's `run_archive` semantics. This one mutates, so it
//!   stays here rather than in the read-only reader.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use darkrun_api::{RunArchiveRequest, RunArchiveResponse};
use serde_json::json;

use crate::state::AppState;

/// Reject a `:slug` path segment that is not a safe `.darkrun/` path component.
///
/// The slug becomes a literal filesystem component (`store.run_dir(slug)`), so a
/// traversal value would let a request read/write outside the state sandbox.
/// The browse routes reject it with `400` before the store joins it, mirroring
/// the guard in `handlers` and the store's own containment. Returns `Some(400)`
/// to short-circuit, `None` to proceed.
fn reject_unsafe_slug(slug: &str) -> Option<Response> {
    if darkrun_core::state::validate::is_valid_slug(slug) {
        None
    } else {
        Some(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid run slug: must be a safe path component" })),
            )
                .into_response(),
        )
    }
}

/// `GET /api/runs` — list the project's runs as summaries, sorted by slug.
pub async fn list_runs(State(state): State<AppState>) -> Response {
    (StatusCode::OK, Json(darkrun_browse::list_runs(&state.store))).into_response()
}

/// `GET /api/runs/:slug` — a single run's detail. `404` when no such run exists.
pub async fn get_run(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    if let Some(resp) = reject_unsafe_slug(&slug) {
        return resp;
    }
    match darkrun_browse::run_detail(&state.store, &slug) {
        Some(payload) => (StatusCode::OK, Json(payload)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "run not found", "id": slug })),
        )
            .into_response(),
    }
}

/// `POST /api/runs/:slug/archive`: set (or clear) a run's archived flag.
///
/// Mirrors the engine's `run_archive` tool semantics: reversible, no
/// confirmation step, and an archived run drops out of the default
/// `GET /api/runs` list (restore with `archived: false`). Archiving also
/// clears the active-run pointer when it names this run, so an archived run
/// stops surfacing as the default. `404` when the run is unknown.
pub async fn set_run_archived(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(req): Json<RunArchiveRequest>,
) -> Response {
    if let Some(resp) = reject_unsafe_slug(&slug) {
        return resp;
    }
    let store = &state.store;
    let Ok(mut run) = store.read_run(&slug) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "run not found", "id": slug })),
        )
            .into_response();
    };

    run.frontmatter.archived = Some(req.archived);
    if store.write_run(&run).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to persist the archived flag" })),
        )
            .into_response();
    }
    if req.archived {
        if let Ok(Some(active)) = store.active_run() {
            if active == slug {
                let _ = store.clear_active_run();
            }
        }
    }

    (
        StatusCode::OK,
        Json(RunArchiveResponse {
            ok: true,
            slug,
            archived: req.archived,
        }),
    )
        .into_response()
}
