//! OAuth2 authorization-code flow helpers.
//!
//! The website hosts the OAuth dance; this module provides the two pieces the
//! server needs: build the provider authorize URL the browser is sent to, and
//! exchange the returned `code` for an access token using the client secret.
//! The exchange runs through the injectable [`HttpTransport`] so the secret
//! never appears in this crate and tests stay offline.

use crate::error::{Result, VcsError};
use crate::provider::{Credential, Provider};
use crate::transport::{HttpRequest, HttpTransport};

/// Percent-encode a string for use in a query component.
///
/// Encodes everything outside the RFC 3986 unreserved set, which is the safe
/// choice for OAuth `state`, `redirect_uri`, and `scope` values.
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

/// Build the provider authorize URL the browser is redirected to.
///
/// `redirect_uri` is the website's `<web>/auth/<provider>/callback`; `state` is
/// the CLI-generated nonce that ties the callback back to the waiting terminal.
/// The scope is provider-defaulted ([`Provider::oauth_scope`]).
pub fn authorize_url(
    provider: Provider,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
) -> String {
    let scope = provider.oauth_scope();
    format!(
        "{base}?client_id={client_id}&redirect_uri={redirect}&scope={scope}&state={state}&response_type=code",
        base = provider.authorize_endpoint(),
        client_id = percent_encode(client_id),
        redirect = percent_encode(redirect_uri),
        scope = percent_encode(scope),
        state = percent_encode(state),
    )
}

/// Exchange an authorization `code` for an access token (server-side).
///
/// Posts to the provider token endpoint with the client id/secret and code,
/// requesting a JSON response. On success returns a populated [`Credential`];
/// on an OAuth error payload returns [`VcsError::OauthExchange`].
pub fn exchange_code(
    transport: &dyn HttpTransport,
    provider: Provider,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<Credential> {
    let body = serde_json::json!({
        "client_id": client_id,
        "client_secret": client_secret,
        "code": code,
        "redirect_uri": redirect_uri,
        "grant_type": "authorization_code",
    });

    let request = HttpRequest::post(provider.token_endpoint())
        .header("Accept", "application/json")
        .json_body(&body)?;

    let response = transport.execute(request)?;
    parse_token_response(provider, response, None)
}

/// Re-mint an expired/expiring access token from a `refresh_token` grant
/// (server-side).
///
/// GitLab access tokens expire (~2h) and are issued alongside a refresh token;
/// this exchanges that refresh token for a fresh [`Credential`] — a new access
/// token, a new `expires_in`, and (GitLab rotates them) usually a new refresh
/// token. Mirrors [`exchange_code`]: the website holds the client secret and
/// drives this through the injectable transport. GitHub OAuth App tokens are
/// long-lived and issue no refresh token, so this is a GitLab-shaped path in
/// practice.
///
/// If the provider's response omits a rotated `refresh_token`, the one passed in
/// is carried forward so the credential stays refreshable.
pub fn refresh_access_token(
    transport: &dyn HttpTransport,
    provider: Provider,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<Credential> {
    let body = serde_json::json!({
        "client_id": client_id,
        "client_secret": client_secret,
        "refresh_token": refresh_token,
        "grant_type": "refresh_token",
    });

    let request = HttpRequest::post(provider.token_endpoint())
        .header("Accept", "application/json")
        .json_body(&body)?;

    let response = transport.execute(request)?;
    parse_token_response(provider, response, Some(refresh_token))
}

/// Parse a token-endpoint response (shared by the authorization-code and
/// refresh grants) into a [`Credential`].
///
/// Both providers report failures as a JSON object with an `error` key,
/// sometimes alongside a 200 status (GitHub), so the body is checked first.
/// `fallback_refresh` is carried into the credential when the response omits a
/// `refresh_token` (the refresh grant preserves the incoming token that way).
fn parse_token_response(
    provider: Provider,
    response: crate::transport::HttpResponse,
    fallback_refresh: Option<&str>,
) -> Result<Credential> {
    let value: serde_json::Value = response.json()?;

    if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
        return Err(VcsError::OauthExchange {
            error: err.to_string(),
            description: value
                .get("error_description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }

    if !response.is_success() {
        return Err(VcsError::Api {
            provider: provider.display_name(),
            status: response.status,
            message: response.text().unwrap_or_default(),
        });
    }

    let access_token = value
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(VcsError::MissingField("access_token"))?
        .to_string();

    Ok(Credential {
        provider,
        access_token,
        refresh_token: value
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| fallback_refresh.map(str::to_string)),
        expires_in: value.get("expires_in").and_then(|v| v.as_u64()),
        token_type: value
            .get("token_type")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}
