//! The relay dial credential and its self-refresh.
//!
//! `/darkrun:darkrun-login` parks a browser-minted Firebase ID token at
//! `~/.darkrun/relay-token`, and the engine dials the relay with it. A raw ID
//! token expires in ~1h, which is fatal for the long lights-out runs remote
//! access exists for: a run started (or reconnecting) after that hour would dial
//! an expired token and 401 forever.
//!
//! The fix keeps the *refresh* material next to the ID token so the CLI and the
//! engine can re-mint the ID token THEMSELVES — no backend endpoint, no Firebase
//! Admin SDK. Google's PUBLIC secure-token endpoint refreshes an ID token
//! directly from a refresh token + the (public) Firebase Web API key:
//!
//! ```text
//! POST https://securetoken.googleapis.com/v1/token?key={api_key}
//! grant_type=refresh_token&refresh_token={refresh_token}
//! -> { id_token, refresh_token, expires_in, ... }
//! ```
//!
//! The stored file is a JSON [`RelayToken`] blob carrying `id_token`,
//! `refresh_token`, `api_key`, and an absolute `expires_at`. A LEGACY bare
//! ID-token string still loads (as an `id_token` with no refresh material) — it
//! simply can't refresh, so it forces a re-login once it expires rather than
//! breaking.
//!
//! Everything network-touching goes through the injectable
//! [`darkrun_vcs::HttpTransport`] seam (the request SHAPE and response handling
//! are unit-tested offline), and the actual securetoken call reuses the engine's
//! pure-Rust [`crate::hosting::UreqTransport`].

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use darkrun_vcs::{HttpRequest, HttpResponse, HttpTransport};
use serde::{Deserialize, Serialize};

/// Google's public secure-token endpoint. It refreshes a Firebase ID token from
/// a refresh token; the Web API key it keys on is public (it ships in the web
/// app's firebase config), so no server/admin credential is involved.
const SECURETOKEN_ENDPOINT: &str = "https://securetoken.googleapis.com/v1/token";

/// Refresh this many seconds BEFORE the stored expiry, so a dial never races the
/// ID token's ~1h lifetime to the wire.
pub const REFRESH_SKEW_SECS: i64 = 300;

/// First retry delay after a refresh FAILURE; it doubles per consecutive failure
/// up to [`MAX_REFRESH_BACKOFF`]. Guards against re-POSTing a dead refresh token
/// to securetoken on every ~5s dial tick (~720 req/hr + a log flood).
const INITIAL_REFRESH_BACKOFF: Duration = Duration::from_secs(30);

/// The cap on the refresh backoff: a persistently-dead refresh token is retried
/// no more often than this (a few minutes), instead of every dial tick.
const MAX_REFRESH_BACKOFF: Duration = Duration::from_secs(300);

/// The relay dial credential stored at `~/.darkrun/relay-token`.
///
/// Historically the file held a BARE Firebase ID-token string; it now holds this
/// JSON blob so the ID token can be re-minted in place. [`RelayToken::parse`]
/// still accepts the legacy bare form (no refresh material → can't refresh).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayToken {
    /// The Firebase ID token the relay verifies — the value dialed on the URL.
    pub id_token: String,
    /// The Firebase refresh token, when the browser captured one. Absent for a
    /// legacy bare-string token (which therefore can't refresh).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// The (public) Firebase Web API key the securetoken endpoint keys on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Absolute expiry (unix seconds). `None` for a legacy token whose lifetime
    /// is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// When the `id_token` was issued / last refreshed (unix seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<i64>,
}

impl RelayToken {
    /// Parse the on-disk form: a JSON [`RelayToken`] blob, or (legacy) a bare
    /// ID-token string. `None` only for empty/whitespace input.
    ///
    /// A bare Firebase ID token is `header.payload.signature` — never a JSON
    /// object — so a `serde_json` object parse cleanly separates the two forms.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(tok) = serde_json::from_str::<RelayToken>(trimmed) {
            if !tok.id_token.trim().is_empty() {
                return Some(tok);
            }
        }
        // Legacy bare ID-token string: keep it working, just unrefreshable.
        Some(RelayToken {
            id_token: trimmed.to_string(),
            refresh_token: None,
            api_key: None,
            expires_at: None,
            issued_at: None,
        })
    }

    /// Load + parse the token at `path`; `None` if absent, empty, or unreadable.
    pub fn load(path: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        Self::parse(&raw)
    }

    /// Serialize + write to `path` (`0600` on unix), creating parent dirs. The
    /// canonical form is always the JSON blob, so a legacy file is upgraded on
    /// the next store.
    ///
    /// The write is ATOMIC and never world-readable: the blob (which carries the
    /// long-lived `refresh_token`) is written to a sibling temp file created
    /// `0600` FROM THE START, then renamed over the target. A plain `fs::write`
    /// creates the file at the umask default (typically `0644`) and chmods to
    /// `0600` only AFTER, leaving a brief window in which the secret is
    /// world-readable — this closes it.
    pub fn store(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(self).map_err(std::io::Error::other)?;
        let tmp = tmp_sibling(path);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let write_result = opts.open(&tmp).and_then(|mut f| {
            use std::io::Write;
            f.write_all(json.as_bytes())
        });
        #[cfg(unix)]
        {
            // Belt-and-suspenders: `mode` only applies when the temp file is
            // CREATED, so if a prior crash left the temp path around, pin 0600
            // explicitly before renaming it into place.
            use std::os::unix::fs::PermissionsExt;
            if write_result.is_ok() {
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
        }
        if let Err(e) = write_result {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, path)
    }

    /// Whether the refresh material (a refresh token + an API key) is present, so
    /// [`refresh_over_transport`] can actually re-mint the ID token.
    pub fn can_refresh(&self) -> bool {
        self.refresh_token.as_deref().is_some_and(|t| !t.trim().is_empty())
            && self.api_key.as_deref().is_some_and(|k| !k.trim().is_empty())
    }

    /// Whether the ID token is at or within `skew_secs` of its expiry as of
    /// `now_unix`. A token with no known expiry (legacy) never reports `true`:
    /// it can't be refreshed anyway, so it is left until it actually 401s.
    pub fn needs_refresh(&self, now_unix: i64, skew_secs: i64) -> bool {
        match self.expires_at {
            Some(exp) => now_unix.saturating_add(skew_secs) >= exp,
            None => false,
        }
    }
}

/// A sibling temp path for the atomic [`RelayToken::store`]: `<file>.tmp.<pid>`
/// in the same directory, so the rename onto the target stays on one filesystem
/// (and is therefore atomic).
fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("relay-token"));
    name.push(format!(".tmp.{}", std::process::id()));
    path.with_file_name(name)
}

/// Build the securetoken refresh request for `api_key` + `refresh_token`:
/// `POST {SECURETOKEN_ENDPOINT}?key={api_key}` with a form body
/// `grant_type=refresh_token&refresh_token={refresh_token}`. Pure — no I/O — so
/// the request SHAPE is unit-tested offline.
fn securetoken_request(api_key: &str, refresh_token: &str) -> HttpRequest {
    let url = format!(
        "{SECURETOKEN_ENDPOINT}?key={}",
        darkrun_vcs::percent_encode(api_key)
    );
    let body = format!(
        "grant_type=refresh_token&refresh_token={}",
        darkrun_vcs::percent_encode(refresh_token)
    );
    HttpRequest::post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .raw_body(body.into_bytes())
}

/// Apply a securetoken response to `token`: on success update `id_token`,
/// `expires_at` (= `now_unix + expires_in`), `issued_at`, and the rotated
/// `refresh_token` when the response carries one. Pure — the response is already
/// fetched — so it's unit-tested offline.
///
/// The securetoken endpoint reports `expires_in` as a STRING of seconds; a
/// missing/garbled one leaves `expires_at` unknown (a subsequent dial then just
/// won't preemptively refresh, falling back to the 401-driven path).
fn apply_refresh_response(
    token: &mut RelayToken,
    resp: &HttpResponse,
    now_unix: i64,
) -> Result<(), String> {
    if !resp.is_success() {
        return Err(format!("securetoken refresh returned {}", resp.status));
    }
    let value: serde_json::Value = resp
        .json()
        .map_err(|e| format!("parsing the securetoken response: {e}"))?;
    let id_token = value
        .get("id_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("the securetoken response is missing id_token")?;
    token.id_token = id_token.to_string();
    let expires_in = value
        .get("expires_in")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok());
    token.issued_at = Some(now_unix);
    token.expires_at = expires_in.map(|secs| now_unix.saturating_add(secs));
    // Firebase rotates the refresh token; carry a new one forward when present.
    if let Some(rt) = value
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        token.refresh_token = Some(rt.to_string());
    }
    Ok(())
}

/// Why a securetoken refresh failed. A HARD failure means the refresh material
/// itself is dead — securetoken answers a revoked/expired/disabled refresh token
/// with `400` (`TOKEN_EXPIRED` / `USER_DISABLED` / `INVALID_REFRESH_TOKEN`), and
/// `401`/`403` are likewise credential rejections, or there is no refresh
/// material at all; only a re-login (`/darkrun:darkrun-login`) fixes it. A
/// TRANSIENT failure (network, a `5xx`, a parse hiccup) is worth retrying on its
/// own. The dial supervisor uses `hard` for the log line (re-login vs "retrying")
/// and backs BOTH off through a [`RefreshLatch`] so neither hot-loops.
#[derive(Debug, Clone)]
pub struct RefreshError {
    /// The refresh credential is dead — the operator must re-run `darkrun login`.
    pub hard: bool,
    /// A human-readable reason, for the log line.
    pub message: String,
}

impl RefreshError {
    fn hard(message: impl Into<String>) -> Self {
        Self { hard: true, message: message.into() }
    }
    fn transient(message: impl Into<String>) -> Self {
        Self { hard: false, message: message.into() }
    }
    /// Classify a non-success securetoken status. The endpoint answers a dead
    /// refresh token with `400` (and a rejected credential with `401`/`403`);
    /// `5xx` and everything else are transient and retried.
    fn from_status(status: u16) -> Self {
        let hard = matches!(status, 400 | 401 | 403);
        Self { hard, message: format!("securetoken refresh returned {status}") }
    }
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RefreshError {}

/// Refresh `token` in place over `transport` as of `now_unix`, classifying any
/// failure as HARD (dead credential — re-login) or TRANSIENT (retry). On success
/// `token` holds a fresh `id_token` + expiry. Mirrors
/// [`darkrun_vcs::refresh_access_token`] over the same [`HttpTransport`] seam.
pub fn refresh_over_transport_classified(
    transport: &dyn HttpTransport,
    token: &mut RelayToken,
    now_unix: i64,
) -> Result<(), RefreshError> {
    let (Some(api_key), Some(refresh_token)) =
        (token.api_key.clone(), token.refresh_token.clone())
    else {
        return Err(RefreshError::hard(
            "the stored relay token has no refresh material — re-run /darkrun:darkrun-login",
        ));
    };
    let resp = transport
        .execute(securetoken_request(&api_key, &refresh_token))
        .map_err(|e| RefreshError::transient(e.to_string()))?;
    // Classify a non-success status BEFORE applying, so a 400 (revoked/expired
    // refresh token) is flagged hard vs a transient 5xx/network blip.
    if !resp.is_success() {
        return Err(RefreshError::from_status(resp.status));
    }
    apply_refresh_response(token, &resp, now_unix).map_err(RefreshError::transient)
}

/// Refresh `token` in place over `transport`, discarding the failure
/// classification. Kept for callers (and tests) that only need the string error;
/// [`refresh_over_transport_classified`] is the classifying core.
pub fn refresh_over_transport(
    transport: &dyn HttpTransport,
    token: &mut RelayToken,
    now_unix: i64,
) -> Result<(), String> {
    refresh_over_transport_classified(transport, token, now_unix).map_err(|e| e.message)
}

/// The stored relay-token path: `~/.darkrun/relay-token`.
pub fn default_relay_token_path() -> Option<PathBuf> {
    crate::registry::default_root().map(|root| root.join("relay-token"))
}

/// The current unix time in seconds (saturating at 0 before the epoch).
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Remembers a refresh FAILURE so a dead refresh token isn't re-POSTed to
/// securetoken on every ~5s dial tick (~720 req/hr + a log flood). After a
/// failure a refresh is re-attempted only once EITHER the relay-token file is
/// replaced (an operator re-login — detected by an mtime change) OR a capped
/// exponential backoff elapses. A transient blip recovers on the first backoff;
/// a permanently-dead token backs off toward [`MAX_REFRESH_BACKOFF`]. The dial
/// supervisor holds ONE latch across ticks, so it also throttles the
/// force-on-rejection refresh (a 401 can't turn into a hot loop).
#[derive(Debug, Default)]
pub struct RefreshLatch {
    failure: Option<RefreshFailure>,
}

/// A recorded refresh failure and the backoff scheduled off it.
#[derive(Debug)]
struct RefreshFailure {
    /// The credential the failure was for (the refresh token, or the id token
    /// when there is none) — a DIFFERENT credential clears the latch.
    key: String,
    /// The relay-token file's mtime at failure — a CHANGE means a re-login
    /// replaced the file, so retry immediately.
    file_mtime: Option<SystemTime>,
    /// The earliest instant a re-attempt is allowed for this same credential.
    next_attempt_at: Instant,
    /// The current backoff, doubled per consecutive failure up to the cap.
    backoff: Duration,
}

impl RefreshLatch {
    /// A fresh latch with no recorded failure.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a refresh should be attempted now for the credential `key`, given
    /// the token file's current `mtime`. `true` unless a prior failure for this
    /// SAME credential is still inside its backoff window and the file hasn't
    /// been replaced.
    fn should_attempt(&self, key: &str, mtime: Option<SystemTime>, now: Instant) -> bool {
        match &self.failure {
            None => true,
            Some(f) if f.file_mtime != mtime => true, // a re-login replaced the file
            Some(f) if f.key != key => true,          // a different credential
            Some(f) => now >= f.next_attempt_at,      // same dead cred: only after backoff
        }
    }

    /// Record a refresh failure, scheduling the next attempt after an
    /// (exponentially growing, capped) backoff.
    fn record_failure(&mut self, key: String, mtime: Option<SystemTime>, now: Instant) {
        let backoff = match &self.failure {
            Some(f) if f.key == key => (f.backoff * 2).min(MAX_REFRESH_BACKOFF),
            _ => INITIAL_REFRESH_BACKOFF,
        };
        let next_attempt_at = now.checked_add(backoff).unwrap_or(now);
        self.failure = Some(RefreshFailure { key, file_mtime: mtime, next_attempt_at, backoff });
    }

    /// Clear the latch after a successful refresh.
    fn record_success(&mut self) {
        self.failure = None;
    }
}

/// The refresh key for `token`: its refresh token, or the id token as a stand-in
/// when there is none. Identifies WHICH credential a refresh failure was for, so
/// a re-login (new credential) clears the backoff latch.
fn refresh_key(token: &RelayToken) -> String {
    token
        .refresh_token
        .clone()
        .unwrap_or_else(|| token.id_token.clone())
}

/// The token file's modification time, or `None` when it can't be read.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Resolve the dial `id_token` from the token at `path` over `transport`,
/// refreshing in place when due. The refresh-decision + latch core, so it's
/// unit-tested offline over a [`MockTransport`] + temp file + injected clocks.
///
/// A refresh fires when the clock heuristic says the token is near expiry OR
/// `force_refresh` is set — as long as the token CAN refresh AND `latch` isn't
/// backing off a dead credential. `force_refresh` is the crux of the crit#6
/// self-heal: when the relay auth-REJECTED the last dial, its 401 is the
/// AUTHORITATIVE signal (over the local clock), so a host clock skewed behind
/// real time or a missing/zero `expires_at` — either of which pins
/// `needs_refresh` to `false` — can no longer strand a genuinely-expired token.
///
/// On a successful refresh the file is rewritten; on failure the (stale)
/// `id_token` is still returned so the dial attempts and the relay's verdict
/// drives recovery — a stale token is no worse than not dialing.
fn resolve_over(
    transport: &dyn HttpTransport,
    path: &Path,
    latch: &mut RefreshLatch,
    force_refresh: bool,
    now_unix: i64,
    now_instant: Instant,
) -> Option<String> {
    let mut token = RelayToken::load(path)?;
    let want_refresh =
        (force_refresh || token.needs_refresh(now_unix, REFRESH_SKEW_SECS)) && token.can_refresh();
    if want_refresh {
        let mtime = file_mtime(path);
        let key = refresh_key(&token);
        if latch.should_attempt(&key, mtime, now_instant) {
            match refresh_over_transport_classified(transport, &mut token, now_unix) {
                Ok(()) => {
                    latch.record_success();
                    if let Err(e) = token.store(path) {
                        eprintln!(
                            "darkrun: refreshed the relay token but could not rewrite {}: {e}",
                            path.display()
                        );
                    }
                }
                Err(err) => {
                    latch.record_failure(key, mtime, now_instant);
                    if err.hard {
                        eprintln!(
                            "darkrun: the relay credential was rejected ({}); re-run \
                             /darkrun:darkrun-login to restore remote access — dialing with the \
                             current token meanwhile",
                            err.message
                        );
                    } else {
                        eprintln!(
                            "darkrun: could not refresh the relay token ({}); dialing with the \
                             current one",
                            err.message
                        );
                    }
                }
            }
        }
    }
    Some(token.id_token)
}

/// Load the stored [`RelayToken`], refresh it when due (the clock heuristic OR
/// `force_refresh` — set after a relay auth-rejection), and return the current
/// `id_token` to dial with. `latch` throttles a persistently-failing refresh
/// across calls. `None` when not logged in.
///
/// Blocking (network on refresh + file I/O) — call from `spawn_blocking`. Uses
/// the engine's pure-Rust [`crate::hosting::UreqTransport`], safe to call from a
/// synchronous context inside the tokio runtime.
#[cfg(not(tarpaulin_include))] // thin glue over unit-tested resolve_over + real I/O
pub fn resolve_dial_id_token_with(latch: &mut RefreshLatch, force_refresh: bool) -> Option<String> {
    let path = default_relay_token_path()?;
    let transport = crate::hosting::UreqTransport::new();
    resolve_over(&transport, &path, latch, force_refresh, now_unix(), Instant::now())
}

/// [`resolve_dial_id_token_with`] with a throwaway latch and no forced refresh —
/// the clock-heuristic-only resolve, for callers that don't track rejection
/// state (e.g. a one-shot resolve).
#[cfg(not(tarpaulin_include))] // thin glue over resolve_dial_id_token_with
pub fn resolve_dial_id_token() -> Option<String> {
    resolve_dial_id_token_with(&mut RefreshLatch::new(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkrun_vcs::{Method, MockTransport};

    #[test]
    fn serde_round_trips_a_full_token() {
        let token = RelayToken {
            id_token: "id-abc".into(),
            refresh_token: Some("rt-xyz".into()),
            api_key: Some("AIzaKEY".into()),
            expires_at: Some(1_800_000_000),
            issued_at: Some(1_799_996_400),
        };
        let json = serde_json::to_string(&token).unwrap();
        let back: RelayToken = serde_json::from_str(&json).unwrap();
        assert_eq!(token, back);
        // parse() reads the very same JSON form back.
        assert_eq!(RelayToken::parse(&json), Some(token));
    }

    #[test]
    fn legacy_bare_string_parses_as_an_unrefreshable_id_token() {
        // A real ID token is `header.payload.signature` — never JSON.
        let bare = "eyJhbGciOi.eyJzdWIiOiI.sig";
        let tok = RelayToken::parse(bare).unwrap();
        assert_eq!(tok.id_token, bare);
        assert_eq!(tok.refresh_token, None);
        assert_eq!(tok.api_key, None);
        assert_eq!(tok.expires_at, None);
        assert!(!tok.can_refresh(), "a legacy token cannot refresh");
        // ...and never asks to (no known expiry), so it's left until it 401s.
        assert!(!tok.needs_refresh(now_unix(), REFRESH_SKEW_SECS));
    }

    #[test]
    fn parse_rejects_empty_and_whitespace() {
        assert_eq!(RelayToken::parse(""), None);
        assert_eq!(RelayToken::parse("   \n"), None);
    }

    #[test]
    fn parse_treats_a_blank_id_token_json_as_legacy_not_valid() {
        // JSON that parses but has an empty id_token falls through to legacy,
        // which then also trims to empty -> the whole thing is the "bare" string.
        let raw = r#"{"id_token":""}"#;
        let tok = RelayToken::parse(raw).unwrap();
        // The legacy fallback stores the raw text as the id_token.
        assert_eq!(tok.id_token, raw);
        assert!(!tok.can_refresh());
    }

    #[test]
    fn needs_refresh_is_true_at_and_inside_the_skew_boundary() {
        let now = 1_000_000i64;
        let skew = REFRESH_SKEW_SECS; // 300
        let tok = |exp: i64| RelayToken {
            id_token: "id".into(),
            refresh_token: Some("rt".into()),
            api_key: Some("k".into()),
            expires_at: Some(exp),
            issued_at: Some(now - 3300),
        };
        // Expiry comfortably beyond now+skew → no refresh.
        assert!(!tok(now + skew + 1).needs_refresh(now, skew));
        // Exactly at the boundary (now + skew == expiry) → refresh.
        assert!(tok(now + skew).needs_refresh(now, skew));
        // Inside the skew window → refresh.
        assert!(tok(now + skew - 1).needs_refresh(now, skew));
        // Already expired → refresh.
        assert!(tok(now - 1).needs_refresh(now, skew));
    }

    #[test]
    fn securetoken_request_shape_is_a_form_post_to_the_public_endpoint() {
        let req = securetoken_request("AIzaKEY", "rt-value/with+chars");
        assert_eq!(req.method, Method::Post);
        assert_eq!(
            req.url,
            "https://securetoken.googleapis.com/v1/token?key=AIzaKEY"
        );
        // Form content type, not JSON.
        assert!(req.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type")
            && v == "application/x-www-form-urlencoded"));
        let body = String::from_utf8(req.body.clone().unwrap()).unwrap();
        // grant_type + a percent-encoded refresh_token.
        assert!(body.starts_with("grant_type=refresh_token&refresh_token="));
        assert!(body.contains("rt-value%2Fwith%2Bchars"), "body was {body}");
    }

    #[test]
    fn refresh_over_transport_updates_id_token_expiry_and_rotates_refresh_token() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=AIzaKEY",
            HttpResponse::new(
                200,
                br#"{"id_token":"new-id","refresh_token":"new-rt","expires_in":"3600","token_type":"Bearer"}"#
                    .to_vec(),
            ),
        );
        let mut token = RelayToken {
            id_token: "old-id".into(),
            refresh_token: Some("old-rt".into()),
            api_key: Some("AIzaKEY".into()),
            expires_at: Some(500),
            issued_at: Some(0),
        };
        refresh_over_transport(&mock, &mut token, 1_000).unwrap();
        assert_eq!(token.id_token, "new-id");
        assert_eq!(token.refresh_token.as_deref(), Some("new-rt"));
        assert_eq!(token.issued_at, Some(1_000));
        assert_eq!(token.expires_at, Some(1_000 + 3600));
        // The request carried the ORIGINAL refresh token in its form body.
        let req = mock.single_request();
        let body = String::from_utf8(req.body.unwrap()).unwrap();
        assert!(body.contains("refresh_token=old-rt"));
    }

    #[test]
    fn refresh_carries_the_incoming_refresh_token_when_none_is_returned() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            HttpResponse::new(200, br#"{"id_token":"fresh","expires_in":"3600"}"#.to_vec()),
        );
        let mut token = RelayToken {
            id_token: "stale".into(),
            refresh_token: Some("keep-me".into()),
            api_key: Some("k".into()),
            expires_at: Some(0),
            issued_at: Some(0),
        };
        refresh_over_transport(&mock, &mut token, 42).unwrap();
        assert_eq!(token.id_token, "fresh");
        assert_eq!(token.refresh_token.as_deref(), Some("keep-me"));
    }

    #[test]
    fn refresh_without_material_is_an_error_and_no_request_is_made() {
        let mock = MockTransport::new();
        let mut token = RelayToken::parse("bare-legacy-token").unwrap();
        let err = refresh_over_transport(&mock, &mut token, 0).unwrap_err();
        assert!(err.contains("no refresh material"));
        assert!(mock.requests().is_empty(), "no network call without material");
    }

    #[test]
    fn refresh_surfaces_a_non_success_status() {
        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            HttpResponse::new(400, br#"{"error":{"message":"TOKEN_EXPIRED"}}"#.to_vec()),
        );
        let mut token = RelayToken {
            id_token: "x".into(),
            refresh_token: Some("rt".into()),
            api_key: Some("k".into()),
            expires_at: Some(0),
            issued_at: Some(0),
        };
        let err = refresh_over_transport(&mock, &mut token, 0).unwrap_err();
        assert!(err.contains("400"));
        // The stale token is left untouched on failure.
        assert_eq!(token.id_token, "x");
    }

    #[test]
    fn store_then_load_round_trips_through_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let token = RelayToken {
            id_token: "id".into(),
            refresh_token: Some("rt".into()),
            api_key: Some("k".into()),
            expires_at: Some(123),
            issued_at: Some(1),
        };
        token.store(&path).unwrap();
        assert_eq!(RelayToken::load(&path), Some(token));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the token file is owner-only");
        }
    }

    /// Fix 3: the credential is `0600` the INSTANT it exists (created 0600, not
    /// chmodded after a world-readable window), and the temp write file is
    /// renamed away — never left beside the target carrying the secret.
    #[cfg(unix)]
    #[test]
    fn store_creates_the_file_0600_and_leaves_no_temp_behind() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        RelayToken {
            id_token: "id".into(),
            refresh_token: Some("secret-rt".into()),
            api_key: Some("k".into()),
            expires_at: Some(1),
            issued_at: Some(0),
        }
        .store(&path)
        .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the credential is owner-only from creation");
        let strays: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() != std::ffi::OsStr::new("relay-token"))
            .collect();
        assert!(strays.is_empty(), "the temp write file is renamed, not left behind");
    }

    /// A token stored comfortably before expiry with refresh material.
    fn refreshable(now: i64, expires_at: Option<i64>) -> RelayToken {
        RelayToken {
            id_token: "stale".into(),
            refresh_token: Some("rt".into()),
            api_key: Some("k".into()),
            expires_at,
            issued_at: Some(now),
        }
    }

    /// A 200 securetoken response minting `new_id`.
    fn ok_refresh(new_id: &str) -> HttpResponse {
        HttpResponse::new(
            200,
            format!(r#"{{"id_token":"{new_id}","expires_in":"3600"}}"#).into_bytes(),
        )
    }

    /// Fix 1(a): an auth-rejection FORCES a refresh even when the clock heuristic
    /// says the token is healthy (expiry far in the future → `needs_refresh` is
    /// false). This is the host-clock-skewed-behind case.
    #[test]
    fn forced_refresh_fires_after_rejection_despite_a_healthy_clock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let now = 1_000_000i64;
        refreshable(now, Some(now + 100_000)).store(&path).unwrap();
        assert!(
            !RelayToken::load(&path).unwrap().needs_refresh(now, REFRESH_SKEW_SECS),
            "precondition: the clock heuristic says the token is fresh"
        );

        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            ok_refresh("forced-fresh"),
        );

        let mut latch = RefreshLatch::new();
        // force_refresh = true models the supervisor after an auth-rejected dial.
        let id = resolve_over(&mock, &path, &mut latch, true, now, Instant::now()).unwrap();
        assert_eq!(id, "forced-fresh", "the rejection forces a refresh past the clock heuristic");
        assert_eq!(mock.requests().len(), 1, "exactly one securetoken POST");
        assert_eq!(RelayToken::load(&path).unwrap().id_token, "forced-fresh", "file rewritten");
    }

    /// Fix 1(b): an auth-rejection forces a refresh even when `expires_at` is
    /// unknown (`None`) — `needs_refresh` is then permanently false, so only the
    /// forced path can re-mint. This is the browser-parse-yielded-0/None case.
    #[test]
    fn forced_refresh_fires_when_expiry_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let now = 1_000_000i64;
        refreshable(now, None).store(&path).unwrap();
        assert!(
            !RelayToken::load(&path).unwrap().needs_refresh(now, REFRESH_SKEW_SECS),
            "precondition: no expiry → needs_refresh is permanently false"
        );

        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            ok_refresh("fresh-no-expiry"),
        );

        let mut latch = RefreshLatch::new();
        let id = resolve_over(&mock, &path, &mut latch, true, now, Instant::now()).unwrap();
        assert_eq!(id, "fresh-no-expiry");
        assert_eq!(mock.requests().len(), 1);
    }

    /// Without a rejection AND with a healthy clock, no refresh is attempted (the
    /// forced path is the only thing that overrides the heuristic).
    #[test]
    fn no_refresh_without_rejection_when_the_clock_is_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let now = 1_000_000i64;
        refreshable(now, Some(now + 100_000)).store(&path).unwrap();
        let mock = MockTransport::new();
        let mut latch = RefreshLatch::new();
        let id = resolve_over(&mock, &path, &mut latch, false, now, Instant::now()).unwrap();
        assert_eq!(id, "stale", "no forced refresh + healthy clock → dial the current token");
        assert!(mock.requests().is_empty(), "no securetoken POST");
    }

    /// Fix 2: a failed refresh is NOT re-attempted every tick — the latch holds
    /// it for the backoff window (even under a forced retry), then allows exactly
    /// one more attempt once the window elapses.
    #[test]
    fn a_failed_refresh_is_not_re_attempted_before_the_backoff() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let now = 1_000_000i64;
        // Expired → the clock heuristic wants a refresh every tick.
        refreshable(now, Some(0)).store(&path).unwrap();

        let mock = MockTransport::new();
        // Two 400s queued; a correctly-latched resolve consumes only one until the
        // backoff elapses (a dead refresh token: TOKEN_EXPIRED).
        for _ in 0..2 {
            mock.expect(
                Method::Post,
                "https://securetoken.googleapis.com/v1/token?key=k",
                HttpResponse::new(400, br#"{"error":{"message":"TOKEN_EXPIRED"}}"#.to_vec()),
            );
        }

        let mut latch = RefreshLatch::new();
        let t0 = Instant::now();
        // First tick: attempts, fails, latches (1 POST).
        resolve_over(&mock, &path, &mut latch, false, now, t0);
        assert_eq!(mock.requests().len(), 1);
        // Immediately again — even FORCED (as after a fresh rejection): the latch
        // holds, so NO new POST. This is what stops the 5s-forever hot loop.
        resolve_over(&mock, &path, &mut latch, true, now, t0);
        assert_eq!(mock.requests().len(), 1, "a dead token is not re-POSTed every tick");
        // Once the backoff elapses, exactly one retry is allowed.
        let after = t0 + INITIAL_REFRESH_BACKOFF + Duration::from_secs(1);
        resolve_over(&mock, &path, &mut latch, false, now, after);
        assert_eq!(mock.requests().len(), 2, "one retry once the backoff elapses");
    }

    /// A transient failure recovers on the first backoff: the retry succeeds and
    /// the latch clears, leaving the token fresh.
    #[test]
    fn a_transient_failure_recovers_on_the_next_backoff() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let now = 1_000_000i64;
        refreshable(now, Some(0)).store(&path).unwrap();

        let mock = MockTransport::new();
        // A 503 (transient) then a 200.
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            HttpResponse::new(503, b"upstream".to_vec()),
        );
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            ok_refresh("recovered"),
        );

        let mut latch = RefreshLatch::new();
        let t0 = Instant::now();
        resolve_over(&mock, &path, &mut latch, false, now, t0);
        assert_eq!(RelayToken::load(&path).unwrap().id_token, "stale", "still stale after the blip");
        let after = t0 + INITIAL_REFRESH_BACKOFF;
        let id = resolve_over(&mock, &path, &mut latch, false, now, after).unwrap();
        assert_eq!(id, "recovered", "the retry after the backoff succeeds");
    }

    // ─── RefreshLatch unit behavior ───────────────────────────────────────────

    #[test]
    fn refresh_latch_holds_for_the_window_then_allows() {
        let mut latch = RefreshLatch::new();
        let t0 = Instant::now();
        let mtime = Some(SystemTime::UNIX_EPOCH);
        assert!(latch.should_attempt("rt", mtime, t0), "no prior failure → attempt");
        latch.record_failure("rt".into(), mtime, t0);
        assert!(!latch.should_attempt("rt", mtime, t0), "inside the window → hold");
        assert!(!latch.should_attempt("rt", mtime, t0 + Duration::from_secs(29)));
        assert!(
            latch.should_attempt("rt", mtime, t0 + INITIAL_REFRESH_BACKOFF),
            "at the window boundary → allowed"
        );
    }

    #[test]
    fn refresh_latch_clears_on_file_replacement_or_new_credential_or_success() {
        let mut latch = RefreshLatch::new();
        let t0 = Instant::now();
        let mtime = Some(SystemTime::UNIX_EPOCH);
        latch.record_failure("rt".into(), mtime, t0);
        // A new mtime (a re-login replaced the file) → retry immediately.
        let newer = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(5));
        assert!(latch.should_attempt("rt", newer, t0), "file replaced → retry now");
        // A different refresh credential → retry immediately.
        assert!(latch.should_attempt("rt-2", mtime, t0), "new credential → retry now");
        // A success clears the latch entirely.
        latch.record_success();
        assert!(latch.should_attempt("rt", mtime, t0));
    }

    #[test]
    fn refresh_latch_backoff_escalates_and_caps() {
        let mut latch = RefreshLatch::new();
        let mut t = Instant::now();
        let mtime = Some(SystemTime::UNIX_EPOCH);
        let mut expected = INITIAL_REFRESH_BACKOFF;
        for _ in 0..8 {
            latch.record_failure("rt".into(), mtime, t);
            assert!(
                !latch.should_attempt("rt", mtime, t + expected - Duration::from_secs(1)),
                "held just before the window"
            );
            assert!(latch.should_attempt("rt", mtime, t + expected), "allowed at the window");
            t += expected;
            expected = (expected * 2).min(MAX_REFRESH_BACKOFF);
        }
        assert_eq!(expected, MAX_REFRESH_BACKOFF, "the backoff caps at the maximum");
    }
}
