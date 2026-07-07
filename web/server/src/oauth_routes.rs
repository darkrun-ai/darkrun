//! The OAuth endpoints the website hosts.
//!
//! ```text
//! GET  /auth/:provider/start?state=NONCE     -> 302 to the provider authorize URL
//! GET  /auth/:provider/callback?code&state   -> exchange code, park under nonce, HTML
//! GET  /auth/broker/:nonce                    -> one-time JSON credential
//! POST /auth/:provider/refresh                -> re-mint a token from a refresh token
//! ```
//!
//! The browser is the only client of `start`/`callback`; the CLI is the only
//! client of `broker` and `refresh`. The client secret is used solely inside the
//! server-side `callback` exchange and the `refresh` grant, and never crosses to
//! the browser or the CLI.
//!
//! `refresh` closes the hosted-flow half of "GitLab OAuth tokens expire": the
//! GitLab client secret lives only on the website, so the CLI cannot run the
//! refresh grant itself. It posts its stored refresh token here; the website
//! performs the secret-bearing grant and returns the rotated credential, so a
//! long run's GitLab token is re-minted before it expires (~2h) mid-run.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use darkrun_vcs::{authorize_url, exchange_code, refresh_access_token, Credential, Provider};
use serde::{Deserialize, Serialize};

use crate::state::WebState;

/// Query for `/auth/:provider/start` — the CLI-generated nonce.
#[derive(Debug, Deserialize)]
pub struct StartQuery {
    /// The opaque nonce tying this login to the waiting terminal.
    pub state: String,
}

/// Query for `/auth/:provider/callback` — what the provider returns.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    /// The authorization code to exchange for a token.
    pub code: Option<String>,
    /// The nonce echoed back, used to park the resulting credential.
    pub state: Option<String>,
    /// An OAuth error code, when the provider denies the request.
    pub error: Option<String>,
    /// The human-readable OAuth error description, when present.
    pub error_description: Option<String>,
}

/// The one-time payload the CLI claims from `/auth/broker/:nonce`.
///
/// Carries the OAuth **refresh material** (`refresh_token` + `expires_in`) as
/// well as the access token, so the hosted CLI can later re-mint a near-expiry
/// GitLab token through `/auth/:provider/refresh`. Without them the CLI would
/// store only a bare access token and a GitLab run would still die ~2h in. Both
/// are omitted from the JSON when absent — GitHub OAuth-app tokens are long-lived
/// and issue neither, so a GitHub payload is unchanged (`{ provider, access_token }`).
#[derive(Debug, Serialize, Deserialize)]
pub struct BrokerPayload {
    /// The provider this token authenticates against.
    pub provider: Provider,
    /// The OAuth access token.
    pub access_token: String,
    /// The OAuth refresh token, when the provider issues one (GitLab does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Seconds-from-issue lifetime, when the provider reports one (GitLab does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
}

/// Body for `POST /auth/:provider/refresh` — the refresh token to rotate.
#[derive(Debug, Deserialize)]
pub struct RefreshBody {
    /// The stored OAuth refresh token the CLI wants re-minted.
    pub refresh_token: String,
}

/// Resolve a `:provider` path segment, or `400` if unknown.
///
/// The `Err` branch carries a ready-to-return [`Response`] (an axum type that is
/// intentionally large); this is local control flow, not an error propagated up
/// a deep call stack, so the size is fine here.
#[allow(clippy::result_large_err)]
fn parse_provider(raw: &str) -> Result<Provider, Response> {
    Provider::from_key(raw).ok_or_else(|| {
        error_page(
            StatusCode::BAD_REQUEST,
            "Unknown provider",
            &format!("`{raw}` is not a supported provider."),
        )
    })
}

/// `GET /auth/:provider/start?state=NONCE`
///
/// Redirects the browser to the provider authorize URL with the configured
/// client id, the server's `redirect_uri`, the provider-default scope, and the
/// caller's nonce as `state`.
pub async fn start(
    State(state): State<WebState>,
    Path(provider_key): Path<String>,
    Query(query): Query<StartQuery>,
) -> Response {
    let provider = match parse_provider(&provider_key) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if query.state.trim().is_empty() {
        return error_page(
            StatusCode::BAD_REQUEST,
            "Missing state",
            "A login nonce is required to start authorization.",
        );
    }

    let creds = match state.config.credentials(provider) {
        Some(c) => c,
        None => {
            return error_page(
                StatusCode::SERVICE_UNAVAILABLE,
                "Provider not configured",
                &format!(
                    "{} sign-in is not available on this server.",
                    provider.display_name()
                ),
            )
        }
    };

    let redirect_uri = state.config.redirect_uri(provider);
    let url = authorize_url(provider, &creds.client_id, &redirect_uri, &query.state);
    Redirect::temporary(&url).into_response()
}

/// `GET /auth/:provider/callback?code&state`
///
/// Exchanges the code for a token server-side, parks it under the nonce, and
/// returns the dark-branded "return to your terminal" page. Provider-reported
/// errors and missing parameters render a branded error page instead.
#[cfg(not(tarpaulin_include))] // OAuth callback: spawns a blocking token exchange over the network — irreducible I/O
pub async fn callback(
    State(state): State<WebState>,
    Path(provider_key): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let provider = match parse_provider(&provider_key) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Some(err) = query.error {
        let detail = query
            .error_description
            .unwrap_or_else(|| "The provider denied the authorization request.".to_string());
        return error_page(
            StatusCode::BAD_REQUEST,
            "Authorization failed",
            &format!("{err}: {detail}"),
        );
    }

    let (code, nonce) = match (query.code, query.state) {
        (Some(c), Some(s)) if !c.is_empty() && !s.is_empty() => (c, s),
        _ => {
            return error_page(
                StatusCode::BAD_REQUEST,
                "Incomplete callback",
                "The provider callback was missing the code or state parameter.",
            )
        }
    };

    let creds = match state.config.credentials(provider) {
        Some(c) => c.clone(),
        None => {
            return error_page(
                StatusCode::SERVICE_UNAVAILABLE,
                "Provider not configured",
                &format!(
                    "{} sign-in is not available on this server.",
                    provider.display_name()
                ),
            )
        }
    };

    let redirect_uri = state.config.redirect_uri(provider);
    let transport = state.transport.clone();

    // The exchange is synchronous (the transport seam is) and may block on I/O;
    // run it off the async reactor.
    let exchanged = tokio::task::spawn_blocking(move || {
        exchange_code(
            transport.as_ref(),
            provider,
            &creds.client_id,
            &creds.client_secret,
            &code,
            &redirect_uri,
        )
    })
    .await;

    let credential = match exchanged {
        Ok(Ok(cred)) => cred,
        Ok(Err(e)) => {
            tracing::warn!(provider = provider.key(), error = %e, "token exchange failed");
            return error_page(
                StatusCode::BAD_GATEWAY,
                "Token exchange failed",
                "darkrun could not complete sign-in with the provider. Try again.",
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "exchange task panicked");
            return error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal error",
                "Something went wrong completing sign-in.",
            );
        }
    };

    state.broker.park(nonce, credential);
    success_page(provider)
}

/// `GET /auth/broker/:nonce`
///
/// Returns the parked credential as JSON exactly once, then evicts it. A second
/// poll, an unknown nonce, or an expired entry all return `404`.
pub async fn broker_claim(
    State(state): State<WebState>,
    Path(nonce): Path<String>,
) -> Response {
    match state.broker.claim(&nonce) {
        Some(cred) => Json(BrokerPayload {
            provider: cred.provider,
            access_token: cred.access_token,
            // Hand the CLI the refresh material so a hosted GitLab run can later
            // re-mint its token via `/auth/:provider/refresh`. GitHub issues
            // neither, so its payload stays `{ provider, access_token }`.
            refresh_token: cred.refresh_token,
            expires_in: cred.expires_in,
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found" })),
        )
            .into_response(),
    }
}

/// `POST /auth/:provider/refresh`
///
/// Re-mints a near-expiry token from a refresh token, server-side. The CLI (the
/// only client) posts `{ refresh_token }`; the server runs the OAuth refresh
/// grant against the provider with the website-held client id/secret (the same
/// [`WebConfig::credentials`](crate::WebConfig::credentials) the `callback`
/// exchange reads) and returns the rotated [`Credential`] as JSON
/// (`{ provider, access_token, refresh_token, expires_in, token_type }`).
///
/// Authorization is possession-based, exactly like `callback`'s `code`: a caller
/// must present a valid refresh token, or the provider's grant rejects it and
/// this answers `502`. The refresh token and client secret are never logged.
///
/// GitHub OAuth-app tokens don't expire and issue no refresh token, so in
/// practice this is a GitLab-shaped path; it stays provider-generic for symmetry.
#[cfg(not(tarpaulin_include))] // spawns a blocking refresh grant over the network — irreducible I/O
pub async fn refresh(
    State(state): State<WebState>,
    Path(provider_key): Path<String>,
    Json(body): Json<RefreshBody>,
) -> Response {
    let provider = match Provider::from_key(&provider_key) {
        Some(p) => p,
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("`{provider_key}` is not a supported provider."),
            )
        }
    };

    if body.refresh_token.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "A refresh_token is required.");
    }

    let creds = match state.config.credentials(provider) {
        Some(c) => c.clone(),
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                &format!(
                    "{} sign-in is not available on this server.",
                    provider.display_name()
                ),
            )
        }
    };

    let transport = state.transport.clone();
    let refresh_token = body.refresh_token;

    // The grant is synchronous (the transport seam is) and may block on I/O; run
    // it off the async reactor, exactly like the `callback` exchange.
    let refreshed = tokio::task::spawn_blocking(move || {
        refresh_access_token(
            transport.as_ref(),
            provider,
            &creds.client_id,
            &creds.client_secret,
            &refresh_token,
        )
    })
    .await;

    match refreshed {
        Ok(Ok(cred)) => refreshed_response(cred),
        Ok(Err(e)) => {
            // `e` carries the provider's OAuth error (code/description) or an HTTP
            // status + body — never the refresh token or client secret.
            tracing::warn!(provider = provider.key(), error = %e, "token refresh failed");
            json_error(
                StatusCode::BAD_GATEWAY,
                "darkrun could not refresh the token with the provider.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "refresh task panicked");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong refreshing the token.",
            )
        }
    }
}

/// Serialize a rotated credential as the refresh response JSON.
///
/// A [`Credential`] serializes to `{ provider, access_token, refresh_token?,
/// expires_in?, token_type? }` — a superset of the `{ access_token,
/// refresh_token, expires_in }` the CLI needs, and the exact shape its
/// credential store round-trips, so the CLI deserializes it straight back into a
/// [`Credential`] and persists it.
fn refreshed_response(cred: Credential) -> Response {
    Json(cred).into_response()
}

/// A JSON error body with `status`. The CLI parses only the HTTP status, but a
/// JSON body keeps this API surface consistent with `/api/*`.
fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

/// The minimal dark-branded "return to your terminal" page.
fn success_page(provider: Provider) -> Response {
    let body = page_shell(
        "Signed in",
        &format!(
            r#"<div class="badge">darkrun</div>
      <h1>You're signed in to {}.</h1>
      <p>Authorization is complete. Return to your terminal — darkrun is
      finishing the handshake. You can close this tab.</p>"#,
            provider.display_name()
        ),
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

/// A branded error page with the given status and message.
fn error_page(status: StatusCode, heading: &str, detail: &str) -> Response {
    let body = page_shell(
        heading,
        &format!(
            r#"<div class="badge">darkrun</div>
      <h1>{heading}</h1>
      <p>{detail}</p>"#
        ),
    );
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

/// The shared dark-only HTML shell. No external assets; inline styles keep the
/// page self-contained for the brief moment it is shown.
fn page_shell(title: &str, inner: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="robots" content="noindex" />
    <title>{title} — darkrun</title>
    <style>
      :root {{ color-scheme: dark; }}
      html, body {{ margin: 0; height: 100%; background: #07090c; color: #e6e8ec;
        font: 16px/1.6 ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif; }}
      body {{ display: grid; place-items: center; padding: 2rem; }}
      main {{ max-width: 32rem; text-align: center; }}
      .badge {{ display: inline-block; letter-spacing: .12em; text-transform: uppercase;
        font-size: .72rem; color: #8a93a3; border: 1px solid #1c222b; border-radius: 999px;
        padding: .3rem .8rem; margin-bottom: 1.5rem; }}
      h1 {{ font-size: 1.5rem; font-weight: 700; margin: 0 0 .75rem; }}
      h1 b {{ font-weight: 800; }}
      p {{ color: #aab2c0; margin: 0; }}
    </style>
  </head>
  <body>
    <main>
      {inner}
    </main>
  </body>
</html>"#
    )
}
