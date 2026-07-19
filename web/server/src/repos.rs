//! `GET /api/repos` — the standalone web app's repository portfolio.
//!
//! The dashboard at app.darkrun.ai asks "what repos can my linked accounts
//! reach?" This endpoint answers it: given the caller's **provider OAuth access
//! token** (the same token kind the OAuth broker hands the CLI, minted by the
//! Firebase sign-in's provider credential, see `web/app/js/firebase-login.js`),
//! it calls the provider's list-repos API and returns a normalized list.
//!
//! The token arrives as a bearer credential the server never persists; it is
//! used for exactly one upstream call and dropped. The listing walk itself lives
//! in [`darkrun_vcs::list_repos`] (shared with the CLI and desktop app) and rides
//! the same injectable [`HttpTransport`](darkrun_vcs::HttpTransport) seam the
//! OAuth token exchange uses, so this is fully offline-testable.
//!
//! This endpoint is only the repo list; a repo's committed `.darkrun/` runs are
//! discovered separately by `GET /api/repos/sessions` (see `sessions.rs`).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use darkrun_vcs::{Credential, Provider};
use serde::Deserialize;

use crate::state::WebState;

/// The normalized repository shape, hosted in `darkrun-vcs` and re-exported so
/// `crate::repos::Repo` (and `pub use repos::Repo` in `lib.rs`) keep resolving.
pub use darkrun_vcs::Repo;

/// Query for `GET /api/repos` — which provider the bearer token belongs to.
#[derive(Debug, Deserialize)]
pub struct ReposQuery {
    /// The provider the access token authenticates against (`github` | `gitlab`).
    pub provider: String,
}

/// `GET /api/repos?provider=…`
///
/// Reads the provider OAuth access token from the `Authorization: Bearer …`
/// header, lists the caller's repositories from that provider, and returns the
/// normalized list. The provider call runs on the blocking pool (the transport
/// seam is synchronous), mirroring the OAuth `callback` handler.
pub async fn list_repos(
    State(state): State<WebState>,
    Query(query): Query<ReposQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(provider) = Provider::from_key(&query.provider) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("`{}` is not a supported provider.", query.provider),
        );
    };

    let Some(token) = bearer_token(&headers) else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "A provider access token is required (Authorization: Bearer …).",
        );
    };

    let transport = state.transport.clone();
    let cred = Credential::new(provider, token);

    // The transport seam is synchronous and may block on network I/O; run it off
    // the async reactor, exactly like the OAuth token exchange.
    let listed = tokio::task::spawn_blocking(move || {
        darkrun_vcs::list_repos(transport.as_ref(), provider, &cred)
    })
    .await;

    match listed {
        Ok(Ok(repos)) => Json(repos).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(provider = provider.key(), error = %e, "repo listing failed");
            json_error(
                StatusCode::BAD_GATEWAY,
                "darkrun could not list your repositories from the provider.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "repo listing task panicked");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong listing your repositories.",
            )
        }
    }
}

/// Extract the bearer token from an `Authorization` header, if present.
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw.strip_prefix("Bearer ").or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
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
        headers.insert(axum::http::header::AUTHORIZATION, "Bearer abc123".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc123".to_string()));
        headers.insert(axum::http::header::AUTHORIZATION, "bearer xyz".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("xyz".to_string()));
        // No header yields None.
        assert_eq!(bearer_token(&axum::http::HeaderMap::new()), None);
    }
}
