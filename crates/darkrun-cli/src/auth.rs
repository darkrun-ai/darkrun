//! `darkrun auth` / `darkrun login` — the CLI surface over the browser-bridge
//! login flows.
//!
//! The reusable machinery (the [`ReqwestTransport`] edge, the OAuth-broker
//! [`login`] flow, the relay [`relay_login`](darkrun_vcs::relay_login) sign-in,
//! plus the URL builders, poll loops, and status/logout paths) now lives in
//! `darkrun-vcs` so the desktop app can drive the same flows natively. The
//! re-exports below keep every existing `auth::…` call site unchanged.
//!
//! What stays here is the relay-TOKEN persistence: it layers on
//! `darkrun_mcp::relay_token` (the engine's on-disk token shape), which sits
//! ABOVE `darkrun-vcs` in the crate graph, so it can't move down into it. The CLI
//! wires that persistence into the shared [`relay_login`] flow as its token sink.

use darkrun_vcs::{HttpTransport, Provider};

// The shared login surface, re-exported so `main.rs`, `pr.rs`, and friends keep
// their `auth::…` paths working unchanged.
pub use darkrun_vcs::{
    login, logout, parse_provider, status, web_base, BrokerRefresher, ReqwestTransport,
};

/// The relay dial credential, re-exported so the CLI stores/refreshes the same
/// shape the engine dials with. See [`darkrun_mcp::relay_token`].
pub use darkrun_mcp::relay_token::RelayToken;

/// Where the relay token is stored (`~/.darkrun/relay-token`), the path the
/// engine reads (`darkrun_mcp::relay_token::resolve_dial_id_token`).
pub fn relay_token_path() -> Option<std::path::PathBuf> {
    darkrun_mcp::relay_token::default_relay_token_path()
}

/// Persist the claimed relay-token payload to `~/.darkrun/relay-token`
/// (`0600` on unix).
///
/// The broker hands the deposited blob back verbatim: the web app now deposits a
/// JSON [`RelayToken`] (carrying the refresh material), while a LEGACY deposit is
/// a bare ID-token string. Either is parsed via [`RelayToken::parse`] and
/// rewritten as the canonical JSON form, so the engine's refresh-aware resolve
/// reads one consistent shape.
pub fn store_relay_token(token: &str) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = relay_token_path().ok_or("could not resolve the ~/.darkrun directory")?;
    let parsed = RelayToken::parse(token).ok_or("the relay broker returned an empty token")?;
    parsed.store(&path)?;
    Ok(path)
}

/// `darkrun login [provider]`: enable REMOTE access.
///
/// Runs the shared browser-bridge sign-in ([`darkrun_vcs::relay_login`]) and
/// persists the deposited relay token to `~/.darkrun/relay-token` through
/// [`store_relay_token`], where the engine dials the relay from.
#[cfg(not(tarpaulin_include))] // opens a browser + polls a live broker
pub fn relay_login(provider: Provider) -> Result<(), Box<dyn std::error::Error>> {
    darkrun_vcs::relay_login(provider, store_relay_token)
}

/// Whether the token at `path` is within the refresh skew of expiry AND can be
/// refreshed. The path-taking core so it's testable over a temp file.
fn relay_token_needs_refresh_at(path: &std::path::Path) -> bool {
    let Some(token) = RelayToken::load(path) else {
        return false;
    };
    token.needs_refresh(
        darkrun_mcp::relay_token::now_unix(),
        darkrun_mcp::relay_token::REFRESH_SKEW_SECS,
    ) && token.can_refresh()
}

/// Whether the stored relay token is within the refresh skew of expiry AND can
/// be refreshed (has refresh material). A legacy bare token reports `false`: it
/// can't refresh, so it forces a re-login only once it actually expires.
pub fn relay_token_needs_refresh() -> bool {
    relay_token_path().is_some_and(|p| relay_token_needs_refresh_at(&p))
}

/// Refresh the token at `path` in place over `transport`. The path/transport
/// core so it's driven offline with a `MockTransport` over a temp file.
///
/// The refresh error is the CLASSIFIED [`darkrun_mcp::relay_token::RefreshError`]
/// (boxed), so the boot pre-flight can downcast it to tell a HARD credential
/// rejection (re-login) from a transient blip when it logs.
fn refresh_relay_token_at(
    transport: &dyn HttpTransport,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut token =
        RelayToken::load(path).ok_or("no relay token stored, run `darkrun login` first")?;
    darkrun_mcp::relay_token::refresh_over_transport_classified(
        transport,
        &mut token,
        darkrun_mcp::relay_token::now_unix(),
    )?;
    token.store(path)?;
    Ok(())
}

/// Refresh the stored relay token in place via Google's public securetoken
/// endpoint, rewriting `~/.darkrun/relay-token` with a fresh `id_token` (+ new
/// expiry and any rotated refresh token). Mirrors darkrun-vcs's OAuth
/// `refresh_access_token` grant, driven through the blocking [`ReqwestTransport`].
/// Returns the token path.
pub fn refresh_relay_token() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = relay_token_path().ok_or("could not resolve the ~/.darkrun directory")?;
    let transport = ReqwestTransport::new()?;
    refresh_relay_token_at(&transport, &path)?;
    Ok(path)
}

/// Best-effort pre-flight (called at `darkrun mcp` boot): if the stored relay
/// token is near expiry and refreshable, re-mint it now so the engine starts
/// from a fresh credential. Never fatal, the engine's per-dial refresh is the
/// backstop, so a failure here is only logged.
pub fn refresh_relay_token_if_needed() {
    if !relay_token_needs_refresh() {
        return;
    }
    match refresh_relay_token() {
        Ok(path) => eprintln!(
            "darkrun: refreshed the relay credential ({})",
            path.display()
        ),
        Err(e) => {
            // A HARD failure (securetoken 400 / revoked refresh token) won't
            // self-heal per-dial, only a re-login does, so say so plainly. A
            // transient blip keeps the softer "retry per-dial" line.
            let hard = e
                .downcast_ref::<darkrun_mcp::relay_token::RefreshError>()
                .is_some_and(|r| r.hard);
            if hard {
                eprintln!(
                    "darkrun: the relay credential was rejected ({e}); re-run \
                     /darkrun:darkrun-login to restore remote access, the engine will dial with \
                     the current token meanwhile"
                );
            } else {
                eprintln!(
                    "darkrun: could not refresh the relay credential ({e}); the engine will retry \
                     per-dial"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkrun_vcs::{HttpResponse, Method, MockTransport};

    #[test]
    fn relay_token_serde_round_trips() {
        let token = RelayToken {
            id_token: "id-1".into(),
            refresh_token: Some("rt-1".into()),
            api_key: Some("AIzaKEY".into()),
            expires_at: Some(1_800_000_000),
            issued_at: Some(1_799_996_400),
        };
        let json = serde_json::to_string(&token).unwrap();
        assert_eq!(RelayToken::parse(&json), Some(token));
    }

    #[test]
    fn legacy_bare_string_parses_and_cannot_refresh() {
        // A real Firebase ID token is `header.payload.signature`, never JSON.
        let tok = RelayToken::parse("eyJhbGci.eyJzdWIi.sig").unwrap();
        assert_eq!(tok.id_token, "eyJhbGci.eyJzdWIi.sig");
        assert_eq!(tok.refresh_token, None);
        assert!(!tok.can_refresh());
    }

    #[test]
    fn store_relay_token_writes_json_and_needs_refresh_reads_the_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        let now = darkrun_mcp::relay_token::now_unix();
        let skew = darkrun_mcp::relay_token::REFRESH_SKEW_SECS;

        // A refreshable token comfortably before expiry yields no refresh.
        RelayToken {
            id_token: "id".into(),
            refresh_token: Some("rt".into()),
            api_key: Some("k".into()),
            expires_at: Some(now + skew + 60),
            issued_at: Some(now),
        }
        .store(&path)
        .unwrap();
        assert!(!relay_token_needs_refresh_at(&path));

        // Inside the skew window yields refresh.
        RelayToken {
            id_token: "id".into(),
            refresh_token: Some("rt".into()),
            api_key: Some("k".into()),
            expires_at: Some(now + skew - 1),
            issued_at: Some(now),
        }
        .store(&path)
        .unwrap();
        assert!(relay_token_needs_refresh_at(&path));

        // A legacy bare token, stored as JSON, can't refresh, so never true.
        // (Written to a temp path; `store_relay_token` targets the real home.)
        let legacy_path = dir.path().join("legacy");
        RelayToken::parse("bare-legacy-token")
            .unwrap()
            .store(&legacy_path)
            .unwrap();
        assert!(!relay_token_needs_refresh_at(&legacy_path));
    }

    #[test]
    fn refresh_relay_token_at_rewrites_the_file_over_a_mock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-token");
        RelayToken {
            id_token: "stale".into(),
            refresh_token: Some("rt-old".into()),
            api_key: Some("k".into()),
            expires_at: Some(0),
            issued_at: Some(0),
        }
        .store(&path)
        .unwrap();

        let mock = MockTransport::new();
        mock.expect(
            Method::Post,
            "https://securetoken.googleapis.com/v1/token?key=k",
            HttpResponse::new(
                200,
                br#"{"id_token":"fresh","refresh_token":"rt-new","expires_in":"3600"}"#.to_vec(),
            ),
        );
        refresh_relay_token_at(&mock, &path).unwrap();

        let reloaded = RelayToken::load(&path).unwrap();
        assert_eq!(reloaded.id_token, "fresh");
        assert_eq!(reloaded.refresh_token.as_deref(), Some("rt-new"));
        assert!(reloaded.expires_at.unwrap() > darkrun_mcp::relay_token::now_unix());
    }
}
