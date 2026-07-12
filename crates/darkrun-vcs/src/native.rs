//! The native HTTP edge and the website/relay browser-bridge login flows.
//!
//! [`ReqwestTransport`] is the real, blocking [`HttpTransport`] adapter that the
//! CLI and the desktop app wire in at the edge (the rest of the crate stays
//! client-agnostic behind the transport seam). Alongside it live the two
//! browser-brokered login flows, shared so every native surface drives them the
//! same way:
//!
//! - `darkrun auth login` (the OAuth broker): [`login`] opens the browser to the
//!   website's start URL, then polls `<web>/auth/broker/<nonce>` until the
//!   provider token is parked.
//! - `darkrun login` (remote access): [`relay_login`] opens the browser to the
//!   web app's Firebase sign-in, then polls `<web>/auth/relay/claim/<nonce>`
//!   until the relay token is deposited, handing it to a caller-supplied sink for
//!   persistence.
//!
//! Everything that touches the network rides the [`HttpTransport`] seam, so the
//! URL building, broker/relay claims, and status/logout paths are all
//! unit-testable offline with [`MockTransport`](crate::MockTransport).

use std::time::Duration;

use crate::{
    Credential, CredentialStore, HttpRequest, HttpResponse, HttpTransport, Provider, Refresher,
};

/// The default website base when `DARKRUN_WEB_BASE` is unset.
pub const DEFAULT_WEB_BASE: &str = "https://darkrun.ai";

/// Environment variable overriding the website base URL.
pub const WEB_BASE_ENV: &str = "DARKRUN_WEB_BASE";

/// How long the login flows wait for the browser round-trip before giving up.
const POLL_TIMEOUT: Duration = Duration::from_secs(180);

/// How long between broker/relay polls.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Resolve the website base, honoring `DARKRUN_WEB_BASE`, trimming any trailing
/// slash so callers can append `/auth/...` cleanly.
pub fn web_base() -> String {
    let raw = std::env::var(WEB_BASE_ENV).unwrap_or_else(|_| DEFAULT_WEB_BASE.to_string());
    raw.trim_end_matches('/').to_string()
}

/// Build the browser-facing OAuth start URL:
/// `<web>/auth/<provider>/start?state=<nonce>`.
pub fn start_url(web_base: &str, provider: Provider, nonce: &str) -> String {
    format!(
        "{base}/auth/{provider}/start?state={nonce}",
        base = web_base.trim_end_matches('/'),
        provider = provider.key(),
        nonce = crate::percent_encode(nonce),
    )
}

/// Build the broker poll URL: `<web>/auth/broker/<nonce>`.
pub fn broker_url(web_base: &str, nonce: &str) -> String {
    format!(
        "{base}/auth/broker/{nonce}",
        base = web_base.trim_end_matches('/'),
        nonce = crate::percent_encode(nonce),
    )
}

/// Build the website refresh-broker URL: `<web>/auth/<provider>/refresh`.
pub fn refresh_url(web_base: &str, provider: Provider) -> String {
    format!(
        "{base}/auth/{provider}/refresh",
        base = web_base.trim_end_matches('/'),
        provider = provider.key(),
    )
}

/// A [`Refresher`] that re-mints a near-expiry token through the WEBSITE broker
/// instead of a locally-held OAuth client secret.
///
/// This is the hosted default. The GitLab client secret lives only on the
/// website, so the CLI can't run the refresh grant itself; it POSTs its stored
/// refresh token to `<web>/auth/<provider>/refresh`, and the website performs the
/// secret-bearing grant and returns the rotated [`Credential`], which
/// [`refresh_before_use`](crate::refresh_before_use) then persists. The
/// self-hosted/desktop path that DOES hold the secret uses
/// [`OauthClient`](crate::OauthClient) directly instead.
pub struct BrokerRefresher {
    web_base: String,
}

impl BrokerRefresher {
    /// A refresher that brokers through `web_base` (e.g. `https://darkrun.ai`).
    pub fn new(web_base: impl Into<String>) -> Self {
        Self {
            web_base: web_base.into(),
        }
    }
}

impl Refresher for BrokerRefresher {
    fn refresh(
        &self,
        transport: &dyn HttpTransport,
        credential: &Credential,
    ) -> crate::Result<Credential> {
        // A refresh needs a refresh token, mirror the direct grant's precondition.
        let refresh_token = credential
            .refresh_token
            .as_deref()
            .ok_or(crate::VcsError::MissingField("refresh_token"))?;

        let url = refresh_url(&self.web_base, credential.provider);
        let body = serde_json::json!({ "refresh_token": refresh_token });
        let request = HttpRequest::post(url)
            .header("Accept", "application/json")
            .json_body(&body)?;
        let response = transport.execute(request)?;
        if !response.is_success() {
            return Err(crate::VcsError::Api {
                provider: credential.provider.display_name(),
                status: response.status,
                message: response.text().unwrap_or_default(),
            });
        }
        // The website returns the rotated credential in `Credential` wire shape.
        let fresh: Credential = response.json()?;
        Ok(fresh)
    }
}

/// Generate a URL-safe random nonce. The nonce doubles as the OAuth `state`
/// value guarding the browser round-trip, so it must be unguessable: it is drawn
/// from the operating-system CSPRNG (`getrandom`), never a seeded PRNG.
pub fn generate_nonce() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    // 252 = 36 * 7, the largest multiple of the alphabet length <= 256. Rejecting
    // bytes at or above it keeps the mapping onto the 36-char alphabet unbiased.
    const REJECT_AT: u8 = (256 / ALPHABET.len() * ALPHABET.len()) as u8;
    let mut out = String::with_capacity(32);
    let mut buf = [0u8; 64];
    let mut i = buf.len();
    while out.len() < 32 {
        if i >= buf.len() {
            getrandom::fill(&mut buf).expect("OS CSPRNG unavailable");
            i = 0;
        }
        let byte = buf[i];
        i += 1;
        if byte < REJECT_AT {
            out.push(ALPHABET[(byte % ALPHABET.len() as u8) as usize] as char);
        }
    }
    out
}

/// The outcome of a single broker poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// The token is ready: here is the parked credential.
    Ready(Box<Credential>),
    /// The token is not parked yet; keep polling.
    Pending,
}

/// Poll the broker once. A `200` with a JSON [`Credential`] body means ready; a
/// `404` (or any other non-2xx) means the token has not been parked yet.
///
/// Pure over the transport seam: the timing loop lives in [`poll_until_ready`].
pub fn poll_broker(
    transport: &dyn HttpTransport,
    web_base: &str,
    nonce: &str,
) -> Result<PollOutcome, Box<dyn std::error::Error>> {
    let url = broker_url(web_base, nonce);
    let response = transport.execute(HttpRequest::get(url))?;
    if response.is_success() {
        let cred: Credential = response.json()?;
        Ok(PollOutcome::Ready(Box::new(cred)))
    } else {
        Ok(PollOutcome::Pending)
    }
}

/// A sleeper seam so the polling loop is testable without real time.
pub trait Sleeper {
    /// Sleep for `dur`.
    fn sleep(&self, dur: Duration);
    /// The elapsed time since the loop began, for timeout accounting.
    fn elapsed(&self) -> Duration;
}

/// The real, wall-clock sleeper used by the binaries.
struct RealSleeper {
    start: std::time::Instant,
}

impl RealSleeper {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Sleeper for RealSleeper {
    fn sleep(&self, dur: Duration) {
        std::thread::sleep(dur);
    }
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Poll the broker until the token is ready or `timeout` elapses.
pub fn poll_until_ready(
    transport: &dyn HttpTransport,
    sleeper: &dyn Sleeper,
    web_base: &str,
    nonce: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<Credential, Box<dyn std::error::Error>> {
    loop {
        match poll_broker(transport, web_base, nonce)? {
            PollOutcome::Ready(cred) => return Ok(*cred),
            PollOutcome::Pending => {
                if sleeper.elapsed() >= timeout {
                    return Err(format!(
                        "timed out after {}s waiting for the browser to finish authorizing",
                        timeout.as_secs()
                    )
                    .into());
                }
                sleeper.sleep(interval);
            }
        }
    }
}

/// A real [`HttpTransport`] backed by a blocking `reqwest` client: the native
/// edge adapter the CLI and desktop app wire in so the rest of the crate stays
/// HTTP-client-agnostic.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// Build a transport with a sensible default timeout.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestTransport {
    #[cfg(not(tarpaulin_include))] // real blocking HTTP, irreducible network I/O
    fn execute(&self, request: HttpRequest) -> crate::Result<HttpResponse> {
        let method = match request.method {
            crate::Method::Get => reqwest::Method::GET,
            crate::Method::Post => reqwest::Method::POST,
            crate::Method::Put => reqwest::Method::PUT,
        };
        let mut builder = self.client.request(method, &request.url);
        for (k, v) in &request.headers {
            builder = builder.header(k, v);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let resp = builder
            .send()
            .map_err(|e| crate::VcsError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp
            .bytes()
            .map_err(|e| crate::VcsError::Transport(e.to_string()))?;
        Ok(HttpResponse::new(status, bytes.to_vec()))
    }
}

/// Open `url` in the operator's default browser, best-effort. A failure is not
/// fatal, the URL is always printed so the operator can open it by hand.
#[cfg(not(tarpaulin_include))] // spawns the OS browser opener, irreducible process I/O
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let prog = ("open", vec![url]);
    #[cfg(target_os = "linux")]
    let prog = ("xdg-open", vec![url]);
    #[cfg(target_os = "windows")]
    let prog = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let prog: (&str, Vec<&str>) = ("true", vec![]);

    let _ = std::process::Command::new(prog.0)
        .args(prog.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// `darkrun auth login --provider ...`.
///
/// Generates a nonce, opens the browser to the website's start URL (printing it
/// too), polls the broker for the token, and persists it to the store.
#[cfg(not(tarpaulin_include))] // opens a browser + polls a live broker
pub fn login(
    provider: Provider,
    store: &CredentialStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = web_base();
    let nonce = generate_nonce();
    let url = start_url(&base, provider, &nonce);

    println!("Opening your browser to authorize with {}...", provider.display_name());
    println!("  {url}");
    println!("If it doesn't open automatically, paste the URL above into your browser.");
    open_browser(&url);

    let transport = ReqwestTransport::new()?;
    let sleeper = RealSleeper::new();
    let cred = poll_until_ready(
        &transport,
        &sleeper,
        &base,
        &nonce,
        POLL_TIMEOUT,
        POLL_INTERVAL,
    )?;

    store.save(&cred)?;
    println!(
        "Authorized with {}, credential saved to {}",
        provider.display_name(),
        store.path().display()
    );
    Ok(())
}

// Remote login (`darkrun login`): the relay token.

/// The default web-app base where Firebase sign-in happens.
pub const DEFAULT_APP_BASE: &str = "https://app.darkrun.ai";
/// Env override for the app base.
pub const APP_BASE_ENV: &str = "DARKRUN_APP_BASE";

/// Resolve the web-app base (`DARKRUN_APP_BASE`, trailing slash trimmed).
pub fn app_base() -> String {
    let raw = std::env::var(APP_BASE_ENV).unwrap_or_else(|_| DEFAULT_APP_BASE.to_string());
    raw.trim_end_matches('/').to_string()
}

/// The browser URL where the user signs in with Firebase Auth:
/// `<app>/login?provider=<p>&nonce=<n>`. After sign-in the web app deposits the
/// minted ID token to the relay broker under this nonce.
pub fn app_login_url(app_base: &str, provider: Provider, nonce: &str) -> String {
    format!(
        "{app}/login?provider={provider}&nonce={nonce}",
        app = app_base,
        provider = provider.key(),
        nonce = crate::percent_encode(nonce),
    )
}

/// The relay-broker claim URL the client polls: `<web>/auth/relay/claim/<nonce>`.
pub fn relay_claim_url(web_base: &str, nonce: &str) -> String {
    format!(
        "{base}/auth/relay/claim/{nonce}",
        base = web_base,
        nonce = crate::percent_encode(nonce),
    )
}

/// The one-time relay-token payload returned by the claim endpoint.
#[derive(Debug, serde::Deserialize)]
struct RelayClaim {
    token: String,
}

/// Outcome of one relay-claim poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayPoll {
    /// The token is parked: here it is.
    Ready(String),
    /// Not deposited yet; keep polling.
    Pending,
}

/// Poll the relay broker once: a `200` with `{token}` is ready; anything else
/// (typically `404`) is pending. Pure over the transport seam.
pub fn poll_relay_claim(
    transport: &dyn HttpTransport,
    web_base: &str,
    nonce: &str,
) -> Result<RelayPoll, Box<dyn std::error::Error>> {
    let url = relay_claim_url(web_base, nonce);
    let response = transport.execute(HttpRequest::get(url))?;
    if response.is_success() {
        let claim: RelayClaim = response.json()?;
        Ok(RelayPoll::Ready(claim.token))
    } else {
        Ok(RelayPoll::Pending)
    }
}

/// Poll the relay broker until the token is deposited or `timeout` elapses.
pub fn poll_relay_until_ready(
    transport: &dyn HttpTransport,
    sleeper: &dyn Sleeper,
    web_base: &str,
    nonce: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<String, Box<dyn std::error::Error>> {
    loop {
        match poll_relay_claim(transport, web_base, nonce)? {
            RelayPoll::Ready(token) => return Ok(token),
            RelayPoll::Pending => {
                if sleeper.elapsed() >= timeout {
                    return Err(format!(
                        "timed out after {}s waiting for the browser to finish signing in",
                        timeout.as_secs()
                    )
                    .into());
                }
                sleeper.sleep(interval);
            }
        }
    }
}

/// `darkrun login [provider]`: enable REMOTE access. Generates a nonce, opens the
/// browser to the web app's Firebase sign-in, polls the relay broker for the
/// deposited token, and hands the raw token to `store_token` for persistence.
///
/// The persistence is a caller-supplied sink because the on-disk relay-token
/// shape layers on `darkrun_mcp::relay_token`, which sits above this crate in the
/// dependency graph: the CLI and desktop app each pass their own storer (both
/// resolve to `~/.darkrun/relay-token`, where the engine dials the relay from).
/// The sink returns the path it wrote, which is echoed to the operator.
#[cfg(not(tarpaulin_include))] // opens a browser + polls a live broker
pub fn relay_login<F>(provider: Provider, store_token: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(&str) -> Result<std::path::PathBuf, Box<dyn std::error::Error>>,
{
    let app = app_base();
    let web = web_base();
    let nonce = generate_nonce();
    let url = app_login_url(&app, provider, &nonce);

    println!("Opening your browser to sign in with {}...", provider.display_name());
    println!("  {url}");
    println!("If it doesn't open automatically, paste the URL above into your browser.");
    open_browser(&url);

    let transport = ReqwestTransport::new()?;
    let sleeper = RealSleeper::new();
    let token = poll_relay_until_ready(&transport, &sleeper, &web, &nonce, POLL_TIMEOUT, POLL_INTERVAL)?;

    let path = store_token(&token)?;
    println!("Signed in, remote access enabled. Token saved to {}", path.display());
    println!("Runs you start now are reachable from app.darkrun.ai and the mobile app.");
    Ok(())
}

/// `darkrun auth status`: print which providers currently have a stored
/// credential. Returns the lines printed (for testing).
pub fn status_lines(store: &CredentialStore) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let authed = store.list()?;
    let mut lines = Vec::new();
    for provider in [Provider::GitHub, Provider::GitLab] {
        let mark = if authed.contains(&provider) {
            "authorized"
        } else {
            "not authorized"
        };
        lines.push(format!("{:<8} {}", provider.display_name(), mark));
    }
    Ok(lines)
}

/// `darkrun auth status`.
pub fn status(store: &CredentialStore) -> Result<(), Box<dyn std::error::Error>> {
    for line in status_lines(store)? {
        println!("{line}");
    }
    Ok(())
}

/// `darkrun auth logout --provider ...`: remove a stored credential. Returns
/// whether one was removed.
pub fn logout(
    provider: Provider,
    store: &CredentialStore,
) -> Result<bool, Box<dyn std::error::Error>> {
    let removed = store.remove(provider)?;
    if removed {
        println!("Removed {} credential.", provider.display_name());
    } else {
        println!("No {} credential to remove.", provider.display_name());
    }
    Ok(removed)
}

/// Parse a `--provider` CLI value into a [`Provider`].
pub fn parse_provider(value: &str) -> Result<Provider, Box<dyn std::error::Error>> {
    Provider::from_key(value)
        .ok_or_else(|| format!("unknown provider '{value}' (expected github or gitlab)").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Method, MockTransport};
    use std::cell::Cell;

    fn temp_store() -> (tempfile::TempDir, CredentialStore) {
        let dir = tempfile::tempdir().expect("tmp");
        let store = CredentialStore::at(dir.path().join("credentials"));
        (dir, store)
    }

    #[test]
    fn start_url_includes_provider_and_state() {
        let url = start_url("https://darkrun.ai", Provider::GitHub, "abc123");
        assert_eq!(
            url,
            "https://darkrun.ai/auth/github/start?state=abc123"
        );
        let gl = start_url("https://darkrun.ai", Provider::GitLab, "n");
        assert_eq!(gl, "https://darkrun.ai/auth/gitlab/start?state=n");
    }

    #[test]
    fn start_url_trims_trailing_slash() {
        let url = start_url("https://darkrun.ai/", Provider::GitHub, "x");
        assert_eq!(url, "https://darkrun.ai/auth/github/start?state=x");
    }

    #[test]
    fn broker_url_is_built_under_auth_broker() {
        assert_eq!(
            broker_url("https://darkrun.ai", "nonce-1"),
            "https://darkrun.ai/auth/broker/nonce-1"
        );
    }

    #[test]
    fn refresh_url_is_provider_scoped() {
        assert_eq!(
            refresh_url("https://darkrun.ai", Provider::GitLab),
            "https://darkrun.ai/auth/gitlab/refresh"
        );
        // Trailing slash is trimmed so the path joins cleanly.
        assert_eq!(
            refresh_url("https://darkrun.ai/", Provider::GitHub),
            "https://darkrun.ai/auth/github/refresh"
        );
    }

    #[test]
    fn broker_refresher_posts_refresh_token_and_returns_rotated_credential() {
        // The website returns the rotated credential in `Credential` wire shape;
        // the refresher deserializes it and hands it back for persistence.
        let mock = MockTransport::new();
        let rotated = Credential {
            provider: Provider::GitLab,
            access_token: "new-access".into(),
            refresh_token: Some("new-refresh".into()),
            expires_in: Some(7200),
            token_type: Some("bearer".into()),
        };
        mock.expect(
            Method::Post,
            "https://darkrun.ai/auth/gitlab/refresh",
            HttpResponse::new(200, serde_json::to_vec(&rotated).unwrap()),
        );

        let stale = Credential {
            provider: Provider::GitLab,
            access_token: "old-access".into(),
            refresh_token: Some("old-refresh".into()),
            expires_in: Some(7200),
            token_type: Some("bearer".into()),
        };
        let broker = BrokerRefresher::new("https://darkrun.ai");
        let fresh = broker.refresh(&mock, &stale).expect("refresh succeeds");

        assert_eq!(fresh.access_token, "new-access");
        assert_eq!(fresh.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(fresh.expires_in, Some(7200));

        // Exactly one POST to the website carrying the OLD refresh token; the
        // client secret is never sent (it lives on the website).
        let req = mock.single_request();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.url, "https://darkrun.ai/auth/gitlab/refresh");
        let body: serde_json::Value =
            serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["refresh_token"], "old-refresh");
        assert!(body.get("client_secret").is_none());
    }

    #[test]
    fn broker_refresher_errors_without_a_refresh_token() {
        // Nothing to exchange yields the same MissingField the direct grant
        // reports, and no request is made.
        let mock = MockTransport::new();
        let cred = Credential::new(Provider::GitLab, "access-only");
        let broker = BrokerRefresher::new("https://darkrun.ai");
        let err = broker.refresh(&mock, &cred).unwrap_err();
        assert!(matches!(
            err,
            crate::VcsError::MissingField("refresh_token")
        ));
        assert!(mock.requests().is_empty());
    }

    #[test]
    fn broker_refresher_surfaces_a_website_failure() {
        // A non-2xx from the website (e.g. the grant was rejected upstream)
        // surfaces as an Api error rather than a bogus credential.
        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://darkrun.ai/auth/gitlab/refresh",
            HttpResponse::new(502, br#"{"error":"refresh failed"}"#.to_vec()),
        );
        let cred = Credential {
            provider: Provider::GitLab,
            access_token: "old".into(),
            refresh_token: Some("r".into()),
            expires_in: Some(7200),
            token_type: None,
        };
        let broker = BrokerRefresher::new("https://darkrun.ai");
        let err = broker.refresh(&mock, &cred).unwrap_err();
        match err {
            crate::VcsError::Api { status, .. } => assert_eq!(status, 502),
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[test]
    fn web_base_honors_env_and_default() {
        // Default path (env unset).
        std::env::remove_var(WEB_BASE_ENV);
        assert_eq!(web_base(), "https://darkrun.ai");
        std::env::set_var(WEB_BASE_ENV, "http://localhost:8080/");
        assert_eq!(web_base(), "http://localhost:8080");
        std::env::remove_var(WEB_BASE_ENV);
    }

    #[test]
    fn nonce_is_long_and_url_safe() {
        let n = generate_nonce();
        assert_eq!(n.len(), 32);
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn nonces_differ_across_calls() {
        // Process+time entropy: two calls should not collide in practice.
        let a = generate_nonce();
        let b = generate_nonce();
        assert_ne!(a, b);
    }

    #[test]
    fn poll_broker_pending_on_404() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/broker/n",
            HttpResponse::new(404, b"not found".to_vec()),
        );
        let out = poll_broker(&mock, "https://darkrun.ai", "n").unwrap();
        assert_eq!(out, PollOutcome::Pending);
    }

    #[test]
    fn poll_broker_ready_parses_credential() {
        let mock = MockTransport::new();
        let body = serde_json::to_vec(&Credential::new(Provider::GitHub, "tok-xyz")).unwrap();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/broker/n",
            HttpResponse::new(200, body),
        );
        let out = poll_broker(&mock, "https://darkrun.ai", "n").unwrap();
        match out {
            PollOutcome::Ready(c) => {
                assert_eq!(c.access_token, "tok-xyz");
                assert_eq!(c.provider, Provider::GitHub);
            }
            PollOutcome::Pending => panic!("expected ready"),
        }
    }

    #[test]
    fn relay_login_urls_are_well_formed() {
        assert_eq!(
            app_login_url("https://app.darkrun.ai", Provider::GitLab, "n0"),
            "https://app.darkrun.ai/login?provider=gitlab&nonce=n0"
        );
        assert_eq!(
            relay_claim_url("https://darkrun.ai", "n0"),
            "https://darkrun.ai/auth/relay/claim/n0"
        );
    }

    #[test]
    fn poll_relay_pending_on_404_then_reads_token() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/relay/claim/n",
            HttpResponse::new(404, b"not found".to_vec()),
        );
        assert_eq!(
            poll_relay_claim(&mock, "https://darkrun.ai", "n").unwrap(),
            RelayPoll::Pending
        );

        let mock = MockTransport::new();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/relay/claim/n",
            HttpResponse::new(200, br#"{"token":"fb-id-token"}"#.to_vec()),
        );
        assert_eq!(
            poll_relay_claim(&mock, "https://darkrun.ai", "n").unwrap(),
            RelayPoll::Ready("fb-id-token".to_string())
        );
    }

    /// A test sleeper that counts ticks and reports a caller-controlled elapsed.
    struct FakeSleeper {
        ticks: Cell<u32>,
        elapsed_after: Duration,
        // Returns `elapsed_after` once `ticks` reaches `time_out_at`.
        time_out_at: u32,
    }

    impl Sleeper for FakeSleeper {
        fn sleep(&self, _dur: Duration) {
            self.ticks.set(self.ticks.get() + 1);
        }
        fn elapsed(&self) -> Duration {
            if self.ticks.get() >= self.time_out_at {
                self.elapsed_after
            } else {
                Duration::ZERO
            }
        }
    }

    #[test]
    fn poll_until_ready_returns_on_first_ready() {
        let mock = MockTransport::new();
        let body = serde_json::to_vec(&Credential::new(Provider::GitLab, "gl-tok")).unwrap();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/broker/n",
            HttpResponse::new(200, body),
        );
        let sleeper = FakeSleeper {
            ticks: Cell::new(0),
            elapsed_after: Duration::ZERO,
            time_out_at: u32::MAX,
        };
        let cred = poll_until_ready(
            &mock,
            &sleeper,
            "https://darkrun.ai",
            "n",
            Duration::from_secs(10),
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(cred.access_token, "gl-tok");
        assert_eq!(sleeper.ticks.get(), 0, "ready on first poll means no sleeps");
    }

    #[test]
    fn poll_until_ready_polls_then_succeeds() {
        let mock = MockTransport::new();
        // First poll pending (404), second ready (200).
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/broker/n",
            HttpResponse::new(404, b"".to_vec()),
        );
        let body = serde_json::to_vec(&Credential::new(Provider::GitHub, "late")).unwrap();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/broker/n",
            HttpResponse::new(200, body),
        );
        let sleeper = FakeSleeper {
            ticks: Cell::new(0),
            elapsed_after: Duration::ZERO,
            time_out_at: u32::MAX,
        };
        let cred = poll_until_ready(
            &mock,
            &sleeper,
            "https://darkrun.ai",
            "n",
            Duration::from_secs(10),
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(cred.access_token, "late");
        assert_eq!(sleeper.ticks.get(), 1, "one sleep between the two polls");
    }

    #[test]
    fn poll_until_ready_times_out() {
        let mock = MockTransport::new();
        // Always pending.
        for _ in 0..5 {
            mock.expect(
                Method::Get,
                "https://darkrun.ai/auth/broker/n",
                HttpResponse::new(404, b"".to_vec()),
            );
        }
        let sleeper = FakeSleeper {
            ticks: Cell::new(0),
            elapsed_after: Duration::from_secs(999),
            time_out_at: 1, // report timed-out after the first sleep
        };
        let err = poll_until_ready(
            &mock,
            &sleeper,
            "https://darkrun.ai",
            "n",
            Duration::from_secs(10),
            Duration::from_millis(1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn status_lines_reflect_stored_credentials() {
        let (_d, store) = temp_store();
        // Nothing stored.
        let lines = status_lines(&store).unwrap();
        assert!(lines.iter().all(|l| l.contains("not authorized")));

        // Save a GitHub credential.
        store
            .save(&Credential::new(Provider::GitHub, "tok"))
            .unwrap();
        let lines = status_lines(&store).unwrap();
        let gh = lines.iter().find(|l| l.contains("GitHub")).unwrap();
        assert!(gh.contains("authorized") && !gh.contains("not authorized"));
        let gl = lines.iter().find(|l| l.contains("GitLab")).unwrap();
        assert!(gl.contains("not authorized"));
    }

    #[test]
    fn logout_removes_existing_credential() {
        let (_d, store) = temp_store();
        store
            .save(&Credential::new(Provider::GitLab, "tok"))
            .unwrap();
        assert!(logout(Provider::GitLab, &store).unwrap());
        // Second logout is a no-op.
        assert!(!logout(Provider::GitLab, &store).unwrap());
        assert!(store.get(Provider::GitLab).unwrap().is_none());
    }

    #[test]
    fn parse_provider_accepts_keys_and_aliases() {
        assert_eq!(parse_provider("github").unwrap(), Provider::GitHub);
        assert_eq!(parse_provider("gh").unwrap(), Provider::GitHub);
        assert_eq!(parse_provider("gitlab").unwrap(), Provider::GitLab);
        assert_eq!(parse_provider("gl").unwrap(), Provider::GitLab);
        assert!(parse_provider("bitbucket").is_err());
    }

    #[test]
    fn login_round_trip_saves_credential() {
        // Exercise the full login path minus the browser by reusing the broker
        // poll + save against a temp store. (The browser open is best-effort and
        // not asserted here.)
        let (_d, store) = temp_store();
        let mock = MockTransport::new();
        let body = serde_json::to_vec(&Credential::new(Provider::GitHub, "logged-in")).unwrap();
        mock.expect(
            Method::Get,
            "https://darkrun.ai/auth/broker/abc",
            HttpResponse::new(200, body),
        );
        let sleeper = FakeSleeper {
            ticks: Cell::new(0),
            elapsed_after: Duration::ZERO,
            time_out_at: u32::MAX,
        };
        let cred = poll_until_ready(
            &mock,
            &sleeper,
            "https://darkrun.ai",
            "abc",
            Duration::from_secs(5),
            Duration::from_millis(1),
        )
        .unwrap();
        store.save(&cred).unwrap();
        assert_eq!(
            store.get(Provider::GitHub).unwrap().unwrap().access_token,
            "logged-in"
        );
    }

    #[test]
    fn real_sleeper_and_reqwest_transport_smoke() {
        use std::time::Duration;
        let s = RealSleeper::new();
        s.sleep(Duration::from_millis(0));
        let _ = s.elapsed();
        // A real transport builds; a request to a dead port fails fast.
        let t = ReqwestTransport::new().expect("client builds");
        assert!(t.execute(HttpRequest::get("http://127.0.0.1:1/x")).is_err());
    }
}
