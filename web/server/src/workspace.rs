//! The persistent-login **workspace** endpoints — the App-backed surface the
//! standalone web app (app.darkrun.ai) renders once signed in.
//!
//! These differ from `repos.rs`/`sessions.rs` in their credential: instead of an
//! ephemeral provider OAuth token in the `Authorization` header, the caller
//! presents a **Firebase ID token** (the durable web-app session). The server
//! verifies it ([`crate::firebase_auth`]), extracts the GitHub identity, and
//! reads the data through the darkrun **GitHub App installation**
//! ([`crate::github_app`]) — so the workspace works on every later load without
//! re-authorizing a provider.
//!
//! - `GET /api/workspace` → every repo the user's installation(s) cover, each
//!   with its committed `.darkrun/` runs embedded.
//! - `GET /api/run?repo=&id=` → one run's full committed state (from
//!   `.darkrun/<id>/state.json` + `run.md`).
//!
//! Both are read-only and ride the injectable
//! [`HttpTransport`](darkrun_vcs::HttpTransport) seam on the blocking pool,
//! exactly like the OAuth-token endpoints.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::github_app::{GitHubApp, GitHubIdentity, WorkspaceRepo};
use crate::state::WebState;

/// `GET /api/workspace` response body: the signed-in user's repositories, each
/// with its darkrun runs embedded.
#[derive(Debug, Serialize)]
struct WorkspaceResponse {
    /// The repositories the user's App installation(s) cover.
    repos: Vec<WorkspaceRepo>,
}

/// Query for `GET /api/run` — which repo + run to read.
#[derive(Debug, Deserialize)]
pub struct RunQuery {
    /// The owner-qualified repository path (e.g. `jwaldrip/darkrun`).
    pub repo: String,
    /// The run id — the `.darkrun/<id>/` directory name.
    pub id: String,
}

/// `GET /api/workspace`
///
/// Verify the Firebase ID token → resolve the GitHub identity → list the App
/// installations for the user → return their repos, each with its `.darkrun/`
/// runs. The App orchestration is synchronous (the transport seam), so it runs
/// on the blocking pool, mirroring `list_repos`.
pub async fn workspace(State(state): State<WebState>, headers: axum::http::HeaderMap) -> Response {
    let (app, identity) = match resolve(&state, &headers) {
        Ok(pair) => pair,
        Err(resp) => return *resp,
    };

    let transport = state.transport.clone();
    let now = now_unix();
    let built = tokio::task::spawn_blocking(move || {
        app.workspace(transport.as_ref(), &identity, now)
    })
    .await;

    match built {
        Ok(Ok(repos)) => Json(WorkspaceResponse { repos }).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "workspace assembly failed");
            json_error(
                StatusCode::BAD_GATEWAY,
                "darkrun could not load your workspace from GitHub.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "workspace task panicked");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong loading your workspace.",
            )
        }
    }
}

/// `GET /api/run?repo=…&id=…`
///
/// Verify the Firebase ID token, then read one run's full committed state from
/// its `.darkrun/<id>/` tree through the installation covering `repo`.
pub async fn run_detail(
    State(state): State<WebState>,
    Query(query): Query<RunQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let repo = query.repo.trim().to_string();
    let run_id = query.id.trim().to_string();
    if repo.is_empty() || run_id.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Both `repo` and `id` are required.",
        );
    }

    let (app, _identity) = match resolve(&state, &headers) {
        Ok(pair) => pair,
        Err(resp) => return *resp,
    };

    let transport = state.transport.clone();
    let now = now_unix();
    let built = tokio::task::spawn_blocking(move || {
        app.run_detail(transport.as_ref(), &repo, &run_id, now)
    })
    .await;

    match built {
        Ok(Ok(run)) => Json(run).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "run detail read failed");
            json_error(
                StatusCode::BAD_GATEWAY,
                "darkrun could not read this run from GitHub.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "run detail task panicked");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong reading this run.",
            )
        }
    }
}

/// Common preamble for both endpoints: the App must be configured, the Firebase
/// verifier present, the bearer token valid, and the account must carry a linked
/// GitHub identity. Returns the App handle + resolved identity, or the error
/// [`Response`] to send (boxed — the error path is cold, and an unboxed axum
/// `Response` is large enough to bloat the `Ok` path).
fn resolve(
    state: &WebState,
    headers: &axum::http::HeaderMap,
) -> Result<(std::sync::Arc<GitHubApp>, GitHubIdentity), Box<Response>> {
    let fail = |status: StatusCode, message: &str| Box::new(json_error(status, message));

    let Some(app) = state.github_app.clone() else {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "The darkrun GitHub App is not configured on this server.",
        ));
    };
    let Some(auth) = state.firebase_auth.clone() else {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sign-in verification is not configured on this server.",
        ));
    };
    let Some(token) = bearer_token(headers) else {
        return Err(fail(
            StatusCode::UNAUTHORIZED,
            "A Firebase ID token is required (Authorization: Bearer …).",
        ));
    };
    let Some(claims) = auth.verify(&token) else {
        return Err(fail(
            StatusCode::UNAUTHORIZED,
            "Your session token is invalid or expired. Sign in again.",
        ));
    };
    let Some(user_id) = claims.github_user_id else {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "This account has no linked GitHub identity. Sign in with GitHub.",
        ));
    };
    let identity = GitHubIdentity {
        // The token carries only the GitHub numeric id; the login is resolved
        // from the installation account itself (id is the stable match key).
        login: None,
        user_id,
    };
    Ok((app, identity))
}

/// Extract the bearer token from an `Authorization` header, if present.
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Current unix time in seconds (0 before the epoch — never, in practice).
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A JSON error body with `status`.
fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_is_extracted_case_insensitively() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer id-token-123".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), Some("id-token-123".to_string()));
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "bearer xyz".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), Some("xyz".to_string()));
        assert_eq!(bearer_token(&axum::http::HeaderMap::new()), None);
    }
}
