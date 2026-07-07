//! OAuth2 authorization-code flow helpers.
//!
//! The website hosts the OAuth dance; this module provides the two pieces the
//! server needs: build the provider authorize URL the browser is sent to, and
//! exchange the returned `code` for an access token using the client secret.
//! The exchange runs through the injectable [`HttpTransport`] so the secret
//! never appears in this crate and tests stay offline.

use crate::error::{Result, VcsError};
use crate::provider::{Credential, Provider};
use crate::store::CredentialStore;
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

/// The confidential OAuth client credentials (`client_id` + `client_secret`) the
/// refresh grant needs, bundled so [`refresh_before_use`] carries exactly what
/// [`refresh_access_token`] requires.
///
/// In the normal product flow the WEBSITE holds these and brokers the initial
/// code exchange, so the CLI never sees them — which is why a GitLab token would
/// otherwise silently expire ~2h into a run. A deployment that ships (or
/// self-hosts) its own OAuth app can instead hand the client its credentials via
/// the environment ([`from_env`](Self::from_env)); the hosting/PR paths then use
/// them to re-mint a near-expiry GitLab token in place. GitHub OAuth-app tokens
/// never expire (they report no `expires_in`), so this only matters for GitLab.
#[derive(Debug, Clone)]
pub struct OauthClient {
    client_id: String,
    client_secret: String,
}

impl OauthClient {
    /// Build a client from an explicit id/secret pair.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }

    /// Load the client credentials for `provider` from the environment:
    /// `DARKRUN_<PROVIDER>_CLIENT_ID` and `DARKRUN_<PROVIDER>_CLIENT_SECRET`
    /// (e.g. `DARKRUN_GITLAB_CLIENT_ID`). Returns `None` unless BOTH are set to a
    /// non-empty value — absent them the caller skips refresh and uses the stored
    /// token as-is (the default brokered flow, where the secret stays on the
    /// website).
    pub fn from_env(provider: Provider) -> Option<Self> {
        let up = provider.key().to_ascii_uppercase();
        let id = non_empty_env(&format!("DARKRUN_{up}_CLIENT_ID"))?;
        let secret = non_empty_env(&format!("DARKRUN_{up}_CLIENT_SECRET"))?;
        Some(Self::new(id, secret))
    }

    /// Re-mint `credential` via the refresh grant, returning a fresh credential.
    ///
    /// Errors with [`VcsError::MissingField`] when the credential carries no
    /// refresh token (there is nothing to exchange).
    pub fn refresh(
        &self,
        transport: &dyn HttpTransport,
        credential: &Credential,
    ) -> Result<Credential> {
        let refresh_token = credential
            .refresh_token
            .as_deref()
            .ok_or(VcsError::MissingField("refresh_token"))?;
        refresh_access_token(
            transport,
            credential.provider,
            &self.client_id,
            &self.client_secret,
            refresh_token,
        )
    }
}

/// Read an environment variable, treating unset OR empty as absent.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Return a usable credential for `provider`, refreshing it in place first if it
/// is about to expire — the refresh-BEFORE-use guard the hosting and PR paths run
/// ahead of a provider REST call.
///
/// Loads the stored credential and the unix time it was obtained
/// ([`CredentialStore::get_with_obtained_at`]); if the token
/// [`needs_refresh`](Credential::needs_refresh) AND is
/// [`refreshable`](Credential::is_refreshable), it mints a fresh one through
/// `oauth`, persists it (which restamps `obtained_at`), and returns the fresh
/// credential. Otherwise it returns the stored credential untouched — and a
/// credential with no `expires_in` (GitHub) never triggers a refresh. `Ok(None)`
/// when no credential is stored for `provider`.
pub fn refresh_before_use(
    store: &CredentialStore,
    transport: &dyn HttpTransport,
    oauth: &OauthClient,
    provider: Provider,
    now_unix: u64,
) -> Result<Option<Credential>> {
    let Some((credential, obtained_at)) = store.get_with_obtained_at(provider)? else {
        return Ok(None);
    };
    if credential.needs_refresh(obtained_at, now_unix) && credential.is_refreshable() {
        let fresh = oauth.refresh(transport, &credential)?;
        store.save(&fresh)?;
        Ok(Some(fresh))
    } else {
        Ok(Some(credential))
    }
}

#[cfg(test)]
mod oauth_tests {
    use super::*;
    use crate::transport::{HttpResponse, Method, MockTransport};

    const GITLAB_TOKEN_URL: &str = "https://gitlab.com/oauth/token";

    /// A GitLab token-endpoint reply carrying a rotated access + refresh token.
    fn token_reply(access: &str, refresh: &str, expires_in: u64) -> HttpResponse {
        HttpResponse::new(
            200,
            serde_json::to_vec(&serde_json::json!({
                "access_token": access,
                "refresh_token": refresh,
                "expires_in": expires_in,
                "token_type": "bearer",
            }))
            .unwrap(),
        )
    }

    /// Write a GitLab credential straight into a fresh store at a chosen issue
    /// time by round-tripping through `save` (which stamps `obtained_at = now`).
    fn seeded_store(cred: &Credential) -> (tempfile::TempDir, CredentialStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::at(dir.path().join("credentials"));
        store.save(cred).unwrap();
        (dir, store)
    }

    fn gitlab_cred(access: &str, refresh: Option<&str>, expires_in: Option<u64>) -> Credential {
        Credential {
            provider: Provider::GitLab,
            access_token: access.to_string(),
            refresh_token: refresh.map(str::to_string),
            expires_in,
            token_type: Some("bearer".into()),
        }
    }

    #[test]
    fn refreshes_and_persists_when_token_is_near_expiry() {
        // A GitLab token issued ~now with a 2h lifetime; ask as if we are past it.
        let cred = gitlab_cred("old-access", Some("old-refresh"), Some(7200));
        let (_dir, store) = seeded_store(&cred);
        let issued = crate::now_unix();

        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            GITLAB_TOKEN_URL,
            token_reply("new-access", "new-refresh", 7200),
        );
        let oauth = OauthClient::new("client-id", "client-secret");

        // now = issue + full ttl → comfortably inside the refresh skew window.
        let fresh = refresh_before_use(&store, &mock, &oauth, Provider::GitLab, issued + 7200)
            .unwrap()
            .expect("a credential is stored");

        // The returned credential is the re-minted one …
        assert_eq!(fresh.access_token, "new-access");
        assert_eq!(fresh.refresh_token.as_deref(), Some("new-refresh"));
        // … it was persisted (so the next request reads the fresh token) …
        assert_eq!(
            store.get(Provider::GitLab).unwrap().unwrap().access_token,
            "new-access"
        );
        // … and exactly one refresh-grant POST hit the token endpoint.
        let posts = mock.requests();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].url, GITLAB_TOKEN_URL);
        let body: serde_json::Value = serde_json::from_slice(posts[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["grant_type"], "refresh_token");
        assert_eq!(body["refresh_token"], "old-refresh");
    }

    #[test]
    fn leaves_a_healthy_token_untouched_and_makes_no_request() {
        // Same 2h token, but asked well before expiry → no refresh, no network.
        let cred = gitlab_cred("live-access", Some("live-refresh"), Some(7200));
        let (_dir, store) = seeded_store(&cred);
        let issued = crate::now_unix();

        let mock = MockTransport::new();
        let oauth = OauthClient::new("client-id", "client-secret");

        let out = refresh_before_use(&store, &mock, &oauth, Provider::GitLab, issued + 10)
            .unwrap()
            .unwrap();
        assert_eq!(out.access_token, "live-access");
        assert!(mock.requests().is_empty(), "healthy token must not be refreshed");
    }

    #[test]
    fn a_token_with_no_lifetime_never_refreshes() {
        // GitHub-shaped: no expires_in → needs_refresh is always false, even with
        // a refresh token present and a far-future clock.
        let cred = gitlab_cred("gh-style", Some("r"), None);
        let (_dir, store) = seeded_store(&cred);
        let mock = MockTransport::new();
        let oauth = OauthClient::new("id", "secret");
        let out = refresh_before_use(&store, &mock, &oauth, Provider::GitLab, u64::MAX / 2)
            .unwrap()
            .unwrap();
        assert_eq!(out.access_token, "gh-style");
        assert!(mock.requests().is_empty());
    }

    #[test]
    fn an_expiring_but_unrefreshable_token_is_returned_as_is() {
        // Near expiry but no refresh token → nothing to exchange, so the stored
        // token is handed back rather than erroring.
        let cred = gitlab_cred("stuck", None, Some(7200));
        let (_dir, store) = seeded_store(&cred);
        let issued = crate::now_unix();
        let mock = MockTransport::new();
        let oauth = OauthClient::new("id", "secret");
        let out = refresh_before_use(&store, &mock, &oauth, Provider::GitLab, issued + 7200)
            .unwrap()
            .unwrap();
        assert_eq!(out.access_token, "stuck");
        assert!(mock.requests().is_empty());
    }

    #[test]
    fn missing_credential_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::at(dir.path().join("credentials"));
        let mock = MockTransport::new();
        let oauth = OauthClient::new("id", "secret");
        assert!(refresh_before_use(&store, &mock, &oauth, Provider::GitLab, 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn refresh_errors_without_a_refresh_token() {
        let cred = gitlab_cred("a", None, Some(10));
        let mock = MockTransport::new();
        let oauth = OauthClient::new("id", "secret");
        let err = oauth.refresh(&mock, &cred).unwrap_err();
        assert!(matches!(err, VcsError::MissingField("refresh_token")));
    }

    #[test]
    fn from_env_needs_both_id_and_secret() {
        // Isolated to a provider whose vars no other test touches.
        let id_key = "DARKRUN_GITLAB_CLIENT_ID";
        let secret_key = "DARKRUN_GITLAB_CLIENT_SECRET";
        std::env::remove_var(id_key);
        std::env::remove_var(secret_key);
        assert!(OauthClient::from_env(Provider::GitLab).is_none());

        std::env::set_var(id_key, "the-id");
        assert!(
            OauthClient::from_env(Provider::GitLab).is_none(),
            "id without secret is not enough"
        );

        std::env::set_var(secret_key, "the-secret");
        let client = OauthClient::from_env(Provider::GitLab).expect("both set");
        assert_eq!(client.client_id, "the-id");
        assert_eq!(client.client_secret, "the-secret");

        std::env::remove_var(id_key);
        std::env::remove_var(secret_key);
    }
}
