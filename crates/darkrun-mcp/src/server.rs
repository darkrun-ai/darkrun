//! In-process MCP + HTTP/WS host.
//!
//! [`serve_stdio`] is the function `darkrun-cli` calls to run the manager as an
//! MCP server over stdin/stdout. Crucially it does NOT run alone: on the same
//! tokio runtime it ALSO spawns the axum HTTP/WS review server
//! ([`darkrun_http`]), bound to a loopback port. Both halves share ONE in-memory
//! [`darkrun_http::SessionRegistry`] (and [`darkrun_http::ProofRegistry`]) on a
//! single [`darkrun_http::AppState`], so an interactive session a tool handler
//! raises is immediately visible to the desktop app connected to that port —
//! with no on-disk `session.json` bridge.
//!
//! The bound port is announced to the agent in the MCP server `instructions`
//! string and on stderr, AND written to the home discovery registry
//! (`~/.darkrun/<slug>/engine-<pid>.json`, see [`crate::registry`]) so the
//! desktop app can discover the engine and the port it serves on — no fixed
//! port required.
//!
//! By default the server binds an EPHEMERAL loopback port (`127.0.0.1:0`) and
//! reads the kernel-assigned port back before advertising it, so many engines
//! coexist. `DARKRUN_PORT` (or `--addr` passed through by the CLI) overrides
//! this with an explicit port when a caller needs a fixed one.

use std::net::SocketAddr;
use std::path::PathBuf;

use rmcp::transport::io::stdio;
use rmcp::ServiceExt;

use darkrun_core::StateStore;
use darkrun_harness::Harness;
use darkrun_http::{AppState, Limits};

use crate::registry::EngineRegistry;
use crate::tools::DarkrunServer;

/// The default loopback address the in-process HTTP/WS server binds.
///
/// Retained for callers that want an explicit fixed port; the default boot path
/// now binds an EPHEMERAL port (see [`resolve_addr`]).
pub const DEFAULT_ADDR: &str = "127.0.0.1:4317";

/// The ephemeral loopback bind address: port `0` lets the kernel assign a free
/// port, which is read back via `local_addr()` after binding.
const EPHEMERAL_ADDR: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
    0,
);

/// Resolve the HTTP/WS bind address: the `DARKRUN_PORT` env override (as a bare
/// port on loopback, or a full `host:port`) else an EPHEMERAL loopback port
/// (`127.0.0.1:0`), whose real value is read back after binding.
fn resolve_addr() -> SocketAddr {
    let raw = std::env::var("DARKRUN_PORT").ok();
    if let Some(raw) = raw {
        let raw = raw.trim();
        // A bare port (e.g. "4400") binds loopback; a full "host:port" parses
        // directly.
        if let Ok(port) = raw.parse::<u16>() {
            return SocketAddr::from(([127, 0, 0, 1], port));
        }
        if let Ok(addr) = raw.parse::<SocketAddr>() {
            return addr;
        }
    }
    EPHEMERAL_ADDR
}

/// Serve the darkrun MCP server over stdio, rooted at `repo_root`, while also
/// hosting the HTTP/WS review server in-process on the resolved address
/// (`DARKRUN_PORT` or [`DEFAULT_ADDR`]).
///
/// Blocks until the MCP client disconnects. Durable state lives under
/// `<repo_root>/.darkrun`; interactive sessions live only in the shared
/// in-memory registry.
pub async fn serve_stdio(repo_root: impl Into<PathBuf>, harness: Harness) -> std::io::Result<()> {
    serve_stdio_on(repo_root, resolve_addr(), harness).await
}

/// Like [`serve_stdio`], but binds the HTTP/WS server to an explicit `addr`
/// (pass port `0` for an ephemeral port). The MCP `instructions` announce the
/// ACTUAL bound port to the agent, and the engine advertises itself in the home
/// discovery registry (`~/.darkrun/<slug>/engine-<pid>.json`). `harness` selects
/// the capability set the server adapts its tools and prompts to.
pub async fn serve_stdio_on(
    repo_root: impl Into<PathBuf>,
    addr: SocketAddr,
    harness: Harness,
) -> std::io::Result<()> {
    let repo_root = repo_root.into();

    // One shared AppState: the registries are clonable shared handles, so the
    // MCP tool handlers and the HTTP/WS handlers observe the same sessions and
    // proofs without any disk round-trip.
    let store = StateStore::new(&repo_root);
    let state = AppState::new(store, Limits::default());
    // On-demand show sessions: a desktop asking for a session id that names a
    // RUN materializes its review payload from state, so clicking a run in the
    // sidebar works before the engine has ticked. Unfocused — a passive read
    // must not repoint the `current` focus channel other windows watch.
    let state = {
        let sessions = state.sessions.clone();
        let mat_store = state.store.clone();
        state.with_session_materializer(move |id| {
            crate::sessions::create_show_with_focus(&sessions, &mat_store, id, false).is_ok()
        })
    };
    // Re-surface a run after the operator resolves an interactive session: drop
    // the answered prompt and push the next open one (or the review) onto the
    // run channel, so answering dismisses + advances without waiting for the
    // agent's next tick.
    let state = {
        let sessions = state.sessions.clone();
        let res_store = state.store.clone();
        state.with_surface_resolver(move |run| {
            // A run still in SETUP: answering a factory/mode/size picker drives
            // the chain forward WITHOUT the agent — raise the next selection's
            // picker, or, on the LAST pick, START the run so the picker closes
            // and the review surfaces immediately (the agent's resume loop then
            // just walks the line).
            if let Some(setup) = res_store.read_run_setup(run) {
                match setup.first_unset() {
                    Some(kind) => {
                        let title =
                            res_store.read_run(run).ok().and_then(|r| r.frontmatter.title);
                        crate::sessions::raise_setup_picker(&sessions, run, title.as_deref(), kind);
                    }
                    None => {
                        // All three chosen — materialize the run, then surface it.
                        let title =
                            res_store.read_run(run).ok().and_then(|r| r.frontmatter.title);
                        let factory = setup.factory.clone().unwrap_or_default();
                        let mode = darkrun_core::domain::Mode::from_label(
                            &setup.mode.clone().unwrap_or_default(),
                        );
                        let size = setup.size.clone().unwrap_or_default();
                        if crate::position::run_start(&res_store, run, &factory, title, mode, &size)
                            .is_ok()
                        {
                            let _ = crate::sessions::create_show(&sessions, &res_store, run);
                        }
                    }
                }
                return;
            }
            let _ = crate::sessions::create_show_with_focus(&sessions, &res_store, run, false);
        })
    };
    // Durable gate land: `POST /review/:slug/decide` flips the in-memory review
    // session, but the ENGINE reads the on-disk StateStore. Wrap
    // `checkpoint_decide` so a desktop/web Approve or Request-changes lands the
    // decision on disk too — `checkpoint_decide` resolves BOTH the pre-execution
    // UserGate and the post-execution Checkpoint, so one hook covers both desktop
    // buttons. Without this the gate never clears on disk and the agent must
    // re-issue advance to move past it.
    let state = {
        let ds = state.store.clone();
        state.with_gate_decider(move |run, approved, fb| {
            // Carry the refusal reason (not just a bool): checkpoint_decide
            // rejects a Prove approve with no measured evidence, or an approve
            // over open must/should, so the HTTP layer can surface it instead of
            // reporting a false success.
            crate::position::checkpoint_decide(&ds, run, approved, fb)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    };
    // With the durable decider in place, tell the shared session registry so the
    // MCP advance HOLDS at operator gates (a registry with no decision path
    // keeps the immediate-return contract instead of waiting forever).
    state.sessions.enable_durable_decisions();
    // Durability: every interactive session (question / direction / picker) the
    // registry upserts — on raise AND on answer — is written to the run's
    // `interactive/` dir, so an open question and its eventual answer survive an
    // engine restart and reappear when the desktop reconnects.
    {
        let persist_store = state.store.clone();
        state.sessions.on_persist(std::sync::Arc::new(move |payload| {
            let _ = persist_store.write_interactive_session(payload);
        }));
    }

    // Bind the listener up front so we can read the REAL port back (the
    // requested addr may carry port 0 for an ephemeral bind) before advertising
    // it. Everything downstream — instructions, stderr, the discovery
    // descriptor — uses this concrete address.
    let listener = darkrun_http::bind_listener(addr).await?;
    let bound = listener.local_addr()?;

    // The active run (if any) is what a remote client tunnels into, and its slug
    // is the relay session id.
    let active_run = state.store.active_run().ok().flatten();

    // Remote access is OPT-IN. With `DARKRUN_RELAY_URL` set AND an active run, the
    // engine advertises a relay candidate and (below) dials the relay so remote
    // clients can reach the run. Without it the engine is LOCAL-ONLY — reachable
    // on loopback without auth, exactly as before. The dial token
    // (`DARKRUN_RELAY_TOKEN`, from `/darkrun:darkrun-login`) is a SECRET: it's
    // used to connect but never written to the descriptor.
    let relay_candidate = resolve_relay_candidate(active_run.as_deref());

    // Advertise the engine in the home discovery registry. Best-effort: a write
    // failure (e.g. no home dir) is non-fatal — the engine still serves, it's
    // just not auto-discoverable. The descriptor is RETAINED on exit (flagged
    // stale, never deleted), so we hold the registry handle to mark it stale.
    let engine_registry = announce_engine(&repo_root, bound, harness.key(), relay_candidate.clone());

    // Dial the relay when remote access is enabled: the host connector parks an
    // outbound WebSocket and bridges remote clients to this loopback server.
    //
    // The candidate is RE-RESOLVED as the active run changes — a supervisor
    // watches the active-run pointer and dials each new run once. This is what
    // reaches a run STARTED AFTER boot (the common case: the engine comes up
    // with no active run, then `darkrun_run_new` starts one); the boot snapshot
    // alone would never dial it. Remote engages once the operator has logged in
    // (a dial token is present); until then the engine stays local-only.
    {
        let relay_store = state.store.clone();
        tokio::spawn(async move {
            // The currently-dialed run, the exact token it was dialed with, and
            // its connector task handle. The token is RE-RESOLVED (and refreshed
            // if near expiry) every tick — see `resolve_relay_token` — instead of
            // frozen at first dial, so the credential a long lights-out run dials
            // with never goes stale.
            //
            // The connector task EXITS on an auth rejection (the relay 401s a
            // stale token, see `darkrun_tunnel::run`); otherwise it reconnects in
            // place. So a finished handle means "the dialed token was rejected".
            // We then re-dial — but only once the resolved token has actually
            // CHANGED (a refresh or a fresh login fixed it), so an unrefreshable
            // legacy token that keeps 401ing doesn't spin: remote just waits for a
            // re-login rather than hammering the relay with a dead token forever.
            let mut dial: Option<DialState> = None;
            let mut warned_missing_token = false;
            // ONE refresh backoff latch across ticks: a dead refresh token is
            // retried on a capped exponential backoff (or when a re-login replaces
            // the file), NOT re-POSTed to securetoken every 5s. Held here so it
            // also throttles the force-on-rejection refresh below.
            let refresh_latch = std::sync::Arc::new(std::sync::Mutex::new(
                crate::relay_token::RefreshLatch::new(),
            ));
            loop {
                if let Some(run) = relay_store.active_run().ok().flatten() {
                    if let Some(cand) = resolve_relay_candidate(Some(&run)) {
                        // A FINISHED connector handle means the relay auth-REJECTED
                        // the dialed token (see `darkrun_tunnel::run`). Feed that into
                        // the resolve as `force_refresh`: the relay's 401 is the
                        // authoritative refresh trigger, so a host clock skewed behind
                        // real time or a missing `expires_at` (both of which pin the
                        // clock heuristic to "still valid") can no longer strand a
                        // genuinely-expired token — the crit#6 failure that must
                        // self-heal.
                        let rejected = dial.as_ref().is_some_and(|d| d.handle.is_finished());
                        // Refresh-aware resolve is blocking (ureq + file I/O), so
                        // keep it off the async worker.
                        let latch = std::sync::Arc::clone(&refresh_latch);
                        let token =
                            tokio::task::spawn_blocking(move || resolve_relay_token(&latch, rejected))
                                .await
                                .ok()
                                .flatten();
                        if let Some(token) = token {
                            let run_changed =
                                dial.as_ref().map(|d| d.run.as_str()) != Some(run.as_str());
                            let token_changed =
                                dial.as_ref().map(|d| d.token.as_str()) != Some(token.as_str());
                            let redial = match &dial {
                                None => true,
                                Some(_) if run_changed => true,
                                // The dialed token was rejected: the forced refresh
                                // above re-minted it, so re-dial ONLY when the resolved
                                // token actually CHANGED. If it didn't — the latch is
                                // backing off a dead refresh token — hold and wait for a
                                // re-login rather than spin.
                                Some(_) if rejected => token_changed,
                                _ => false,
                            };
                            if redial {
                                if let Some(prev) = dial.take() {
                                    prev.handle.abort();
                                }
                                let cfg = darkrun_tunnel::ConnectorConfig {
                                    relay_host_url: format!(
                                        "{}/relay/host/{}?token={}",
                                        cand.url.trim_end_matches('/'),
                                        cand.session,
                                        token
                                    ),
                                    local_http_base: format!("http://{bound}"),
                                    run: run.clone(),
                                    reconnect: std::time::Duration::from_secs(3),
                                };
                                eprintln!(
                                    "darkrun: remote access enabled — dialing relay {} for run {run}",
                                    cand.url
                                );
                                let handle = tokio::spawn(darkrun_tunnel::run(cfg));
                                dial = Some(DialState { run, token, handle });
                                warned_missing_token = false;
                            }
                        } else if !warned_missing_token {
                            eprintln!(
                                "darkrun: remote relay configured but no DARKRUN_RELAY_TOKEN — \
                                 run /darkrun:darkrun-login to enable remote access (staying local-only)"
                            );
                            warned_missing_token = true;
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }

    // Spawn the axum HTTP/WS server on the same runtime, sharing the state, on
    // the already-bound listener.
    let http_state = state.clone();
    let limits = http_state.limits;
    let router = darkrun_http::build_router(http_state);
    tokio::spawn(async move {
        if let Err(e) = darkrun_http::serve_router_on(listener, router, limits).await {
            eprintln!("darkrun: in-process HTTP server on {bound} stopped: {e}");
        }
    });

    // Announce the bound port so the desktop app (DARKRUN_PORT) can connect.
    eprintln!("darkrun: HTTP/WS review server listening on http://{bound}");
    eprintln!("darkrun: harness = {}", harness.key());

    let server = DarkrunServer::with_sessions(repo_root, state.sessions.clone())
        .with_announced_addr(bound)
        .with_harness(harness);
    // A session opening onto a project with an ACTIVE run brings the desktop up
    // immediately — the operator watches the run live from the first moment,
    // instead of waiting for a gate (or even a first tick) to raise it. The
    // show session is PUSHED here too (focused), so the app has a payload to
    // render the instant it connects.
    if let Some(active) = active_run.as_deref() {
        let _ = crate::sessions::create_show(&state.sessions, &state.store, active);
        server.surface_desktop_once(active);
    }
    let running = server
        .serve(stdio())
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let wait = running
        .waiting()
        .await
        .map_err(|e| std::io::Error::other(e.to_string()));

    // On a clean shutdown, flag the discovery descriptor stale (retains the
    // record). Best-effort.
    if let Some(registry) = &engine_registry {
        if let Err(e) = registry.mark_stale() {
            eprintln!("darkrun: could not flag discovery descriptor stale: {e}");
        }
    }

    wait?;
    Ok(())
}

/// The live dial the supervisor is holding: the run being tunneled, the exact
/// dial token in use, and the connector task's handle (a finished handle means
/// the relay rejected that token — see the supervisor loop).
struct DialState {
    run: String,
    token: String,
    handle: tokio::task::JoinHandle<()>,
}

/// The relay dial token (a Firebase ID token from `/darkrun:darkrun-login`):
/// the `DARKRUN_RELAY_TOKEN` env override, else the [`RelayToken`] blob
/// `darkrun login` stored at `~/.darkrun/relay-token`. `None` when not logged in
/// (remote stays off).
///
/// The stored blob is REFRESH-AWARE: when its `id_token` is within the refresh
/// skew of expiry — OR `force_refresh` is set because the relay auth-rejected the
/// last dial — this re-mints it from the parked refresh token (rewriting the
/// file) and returns the FRESH `id_token`, so a run started or reconnecting after
/// the ~1h ID-token lifetime dials a live token instead of 401ing forever.
/// `latch` throttles a persistently-failing refresh (a dead refresh token is not
/// re-POSTed to securetoken every tick). The env override is a bare token and is
/// used verbatim (never refreshed, so the latch/force don't apply to it).
///
/// Blocking (network on refresh + file I/O) — the dial supervisor calls it from
/// `spawn_blocking`.
///
/// [`RelayToken`]: crate::relay_token::RelayToken
fn resolve_relay_token(
    latch: &std::sync::Mutex<crate::relay_token::RefreshLatch>,
    force_refresh: bool,
) -> Option<String> {
    if let Ok(t) = std::env::var("DARKRUN_RELAY_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    let mut latch = latch.lock().unwrap_or_else(|p| p.into_inner());
    crate::relay_token::resolve_dial_id_token_with(&mut latch, force_refresh)
}

/// The production relay endpoint the engine dials by default. `DARKRUN_RELAY_URL`
/// overrides it (staging / self-hosted); nothing needs to SET it for remote
/// access to work, so `darkrun login` (which stores the dial token) is
/// sufficient to make a run reachable — the URL no longer has to be exported by
/// hand, which nothing did.
///
/// The relay is served by the web service on the APEX host (`darkrun.ai/relay/…`);
/// `relay.darkrun.ai` has no DNS record today, so defaulting to it made every
/// default-config engine fail its dial at DNS resolution. If a dedicated relay
/// host is stood up later (Terraform has the mapping prepped), flipping this
/// constant (or setting `DARKRUN_RELAY_URL`) is the whole migration.
pub const DEFAULT_RELAY_URL: &str = "wss://darkrun.ai";

/// The relay candidate to advertise + dial. The base URL defaults to the
/// production relay ([`DEFAULT_RELAY_URL`]) and is overridable via
/// `DARKRUN_RELAY_URL`; the session is the active run's slug. Returns `None` only
/// when there is no active run to tunnel into. The dial token is read separately
/// — it's a secret and never enters the candidate (which is written to the
/// public descriptor).
fn resolve_relay_candidate(active_run: Option<&str>) -> Option<darkrun_api::tunnel::RelayCandidate> {
    let url = std::env::var("DARKRUN_RELAY_URL")
        .ok()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string());
    let session = active_run?.to_string();
    Some(darkrun_api::tunnel::RelayCandidate { url, session })
}

/// Write the home discovery descriptor for this engine, returning the registry
/// handle (used to flag the descriptor stale on shutdown) or `None` if the
/// registry could not be set up or the write failed. `relay` advertises the
/// remote candidate alongside the local one when remote access is enabled.
fn announce_engine(
    repo_root: &std::path::Path,
    addr: SocketAddr,
    harness_key: &str,
    relay: Option<darkrun_api::tunnel::RelayCandidate>,
) -> Option<EngineRegistry> {
    let mut registry = match EngineRegistry::new(repo_root) {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("darkrun: discovery registry unavailable: {e}");
            return None;
        }
    };
    if let Some(cand) = relay {
        registry = registry.with_relay(cand);
    }
    match registry.announce(addr, harness_key) {
        Ok(_descriptor) => {
            eprintln!(
                "darkrun: discovery descriptor written to {}",
                registry.descriptor_path().display()
            );
            // Backfill the durable project record so this session's repo shows up
            // in the desktop even if it was never added by hand. Best-effort and
            // idempotent (an existing record, with its added_at, is kept).
            if let Err(e) = crate::registry::ensure_project_registered(repo_root) {
                eprintln!("darkrun: could not register project for discovery: {e}");
            }
            Some(registry)
        }
        Err(e) => {
            eprintln!("darkrun: could not write discovery descriptor: {e}");
            None
        }
    }
}
