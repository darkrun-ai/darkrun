//! The relay client connection — the web app's transport.
//!
//! WASM in a browser can't open raw sockets, so this rides the browser's
//! WebSocket via `gloo-net`, speaking the shared tunnel protocol (the same
//! [`ServerFrame`]/[`ClientFrame`] the native clients use). It connects to the
//! relay's client endpoint, greets with [`ClientFrame::Hello`], and renders each
//! [`ServerFrame::Snapshot`]/`Update` it receives — so the page reads into the
//! live session on connect. A dropped socket reconnects with a fixed backoff
//! (the snapshot-on-connect resync means no state is lost).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use darkrun_api::session::SessionPayload;
use darkrun_api::tunnel::{ClientCommand, ClientFrame, RetryPolicy, RetryStep, ServerFrame};
use dioxus::prelude::*;
use futures::channel::mpsc::UnboundedReceiver;
use futures::{select, FutureExt, SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};

/// The live connection state the UI renders. (Not `PartialEq` — the session
/// payload isn't; the UI re-renders on every set, which is what we want for a
/// live feed.)
#[derive(Clone)]
pub enum RemoteState {
    /// No connection target was found in the URL.
    Unconfigured,
    /// Opening the socket / awaiting the first snapshot.
    Connecting,
    /// A live session payload is in hand — a checkpoint Review, or an interactive
    /// Question / Direction / Picker the engine mirrored onto the run feed.
    Live(Box<SessionPayload>),
    /// The socket dropped; retrying.
    Reconnecting,
}

/// The outcome of the operator's most recent remote command (approve a gate,
/// answer a question). Surfaced in the UI so a remote action is never a silent
/// no-op: the host's ack — or its rejection — is shown.
#[derive(Clone, PartialEq)]
pub enum CommandOutcome {
    /// No command has been issued yet.
    Idle,
    /// A command was dispatched; awaiting the host's ack.
    Pending,
    /// The host applied the command.
    Applied,
    /// The host rejected the command, or it couldn't be delivered.
    Failed(String),
}

/// Where to connect: the relay client URL for a session, assembled from the page
/// query (`?relay=wss://…&session=…&token=…`).
#[derive(Clone, PartialEq)]
pub struct Target {
    /// The full `wss://…/relay/client/{session}?token=…` URL.
    pub url: String,
    /// The session id, for display.
    pub session: String,
}

/// Resolve the connection target from the page URL query string, or `None` when
/// the app was opened without one (e.g. the bare landing). Reads the browser's
/// `location.search`; the parsing itself lives in the pure [`target_from_query`]
/// so it is testable off-browser.
pub fn target_from_url() -> Option<Target> {
    let search = web_sys::window()?.location().search().ok()?;
    target_from_query(&search)
}

/// Build the relay [`Target`] from a raw query string (`?relay=…&session=…&token=…`,
/// with or without the leading `?`). Returns `None` unless all three of `relay`,
/// `session`, and `token` are present and non-empty. The URL is the shared
/// `{relay}/relay/client/{session}?token=…` contract (mirrors
/// [`darkrun_api::tunnel::RelayCandidate::client_ws_url`]); `relay`'s trailing
/// slash is trimmed so the path never doubles up.
///
/// Pure (no `web_sys`) so the reachability parsing is unit-tested on native.
fn target_from_query(search: &str) -> Option<Target> {
    let query = search.trim_start_matches('?');
    let mut relay = None;
    let mut session = None;
    let mut token = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("relay", v)) => relay = Some(decode(v)),
            Some(("session", v)) => session = Some(decode(v)),
            Some(("token", v)) => token = Some(decode(v)),
            _ => {}
        }
    }
    let (relay, session, token) = (relay?, session?, token?);
    if relay.is_empty() || session.is_empty() || token.is_empty() {
        return None;
    }
    let url = format!(
        "{}/relay/client/{}?token={}",
        relay.trim_end_matches('/'),
        session,
        token
    );
    Some(Target { url, session })
}

/// Minimal percent-decoding for the query values we read.
fn decode(s: &str) -> String {
    s.replace("%3A", ":").replace("%2F", "/").replace("%2f", "/")
}

/// A monotonic id for outbound commands (the protocol's idempotency/ack key).
/// STABLE for a command's whole life: a resend — even across a reconnect —
/// reuses it, so paired with the connection's stable `instance` the host's
/// `(instance, id)` dedup collapses the redelivery.
///
/// This counter is PROCESS-GLOBAL (from zero), so two tabs both mint `c0`. That
/// alone would collide at the host — which is why the dedup key also carries the
/// per-page-load [`new_instance_id`], unique per tab, keeping the two apart.
fn next_command_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("c{}", N.fetch_add(1, Ordering::Relaxed))
}

/// A per-page-load INSTANCE id: unique per browser tab / app launch and held
/// stable for a [`run_connection`]'s whole life (across every reconnect). It — not
/// the relay's per-socket connection id, which is minted fresh on every dial —
/// anchors the host's `(instance, id)` command dedup, so a resend on a fresh
/// socket after a drop collapses to a single effect, while two tabs get distinct
/// instances and never collide even though both number their command ids from
/// `c0`.
///
/// Drawn from the browser's crypto RNG (`crypto.getRandomValues`); if that is
/// somehow unavailable it falls back to the wall clock plus a `Math.random` draw,
/// which is still distinct per load (uniqueness, not secrecy, is what's needed).
fn new_instance_id() -> String {
    let mut buf = [0u8; 16];
    if let Some(window) = web_sys::window() {
        if let Ok(crypto) = window.crypto() {
            if crypto.get_random_values_with_u8_array(&mut buf).is_ok() {
                let mut s = String::with_capacity(32);
                for b in buf {
                    s.push_str(&format!("{b:02x}"));
                }
                return s;
            }
        }
    }
    // Fallback: wall clock + a Math.random draw — distinct per page load.
    let rand = (js_sys::Math::random() * f64::from(u32::MAX)) as u64;
    format!("i{:x}{:08x}", now_ms() as u64, rand)
}

/// One command awaiting its ack. Carries the STABLE id (kept across resends and
/// reconnects), the command to resend verbatim, how many times it's been sent,
/// when its next action is due, and whether that action is a final give-up (the
/// retry budget is spent) rather than another resend.
struct PendingCmd {
    id: String,
    command: ClientCommand,
    attempts: u32,
    due_at_ms: f64,
    expiring: bool,
}

/// Wall-clock milliseconds (`Date.now()`) — the retry clock. `Instant` panics on
/// wasm, so the browser clock is the monotonic-enough source for the backoff.
fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// (Re)schedule a pending command's next action from the retry policy, now that
/// it's been sent `attempts` times: a `Resend` waits its backoff; a `GiveUp`
/// waits one final ack window (the cap) before the command is failed.
fn schedule(p: &mut PendingCmd, now: f64, policy: &RetryPolicy) {
    match policy.step(p.attempts) {
        RetryStep::Resend { backoff } => {
            p.due_at_ms = now + backoff.as_millis() as f64;
            p.expiring = false;
        }
        RetryStep::GiveUp => {
            p.due_at_ms = now + policy.cap.as_millis() as f64;
            p.expiring = true;
        }
    }
}

/// Serialize + send one `Cmd` frame (instance + id + command). `Err(())` means the
/// socket send failed, so the caller reconnects and resends the pending set. The
/// `instance` is the connection's stable page-load id — the SAME on the initial
/// send and every resend — so the host's `(instance, id)` dedup collapses a
/// cross-reconnect redelivery.
async fn send_cmd<S>(tx: &mut S, instance: &str, id: &str, command: &ClientCommand) -> Result<(), ()>
where
    S: futures::Sink<Message> + Unpin,
{
    let frame = ClientFrame::Cmd {
        instance: instance.to_string(),
        id: id.to_string(),
        command: command.clone(),
    };
    match serde_json::to_string(&frame) {
        Ok(j) => tx.send(Message::Text(j)).await.map_err(|_| ()),
        // Serialization can't fail for these types; treat as sent.
        Err(_) => Ok(()),
    }
}

/// The milliseconds to sleep before the next retry check: the soonest pending
/// deadline, or a long idle wait when nothing is pending (the select is also
/// woken by inbound frames and freshly-issued commands).
fn next_wait_ms(pending: &[PendingCmd], now: f64) -> u64 {
    const IDLE_MS: u64 = 3_600_000;
    pending
        .iter()
        .map(|p| (p.due_at_ms - now).max(0.0) as u64)
        .min()
        .unwrap_or(IDLE_MS)
}

/// Run the connection loop forever: push live session payloads into `state`,
/// forward any [`ClientCommand`] arriving on `cmd_rx` to the host as an acked
/// `Cmd` frame, and reflect each command's ack (or rejection) into `outcome` so
/// the UI can surface it. Reconnects with a fixed backoff after any drop.
///
/// **Exactly-once writes.** An unacked command is RESENT with its original id on
/// a bounded exponential backoff ([`RetryPolicy`]), capped at a few attempts, and
/// stops the moment its ack arrives. The pending set survives a reconnect, so a
/// command whose send was lost to a drop is re-delivered on the NEXT socket. Every
/// send — initial and resend, on any socket — carries this connection's stable
/// per-page-load `instance` id (minted once below); paired with the stable command
/// id, the host's `(instance, id)` dedup makes the redelivery a true no-op even
/// though the relay hands the reconnect a fresh per-socket connection id.
/// At-least-once resend + host dedup ⇒ exactly-once in effect.
pub async fn run_connection(
    url: String,
    mut state: Signal<RemoteState>,
    mut cmd_rx: UnboundedReceiver<ClientCommand>,
    mut outcome: Signal<CommandOutcome>,
) {
    let policy = RetryPolicy::DEFAULT;
    // This page load's stable instance id: minted ONCE here, held for the whole
    // connection lifetime (across every reconnect), and stamped into every Cmd. A
    // second tab mints a different one, so the two never collide at the host.
    let instance = new_instance_id();
    // Unacked commands, kept ACROSS reconnects (declared outside the loop) so a
    // send lost to a drop is resent — same id ⇒ the host de-dupes.
    let mut pending: Vec<PendingCmd> = Vec::new();
    loop {
        state.set(RemoteState::Connecting);
        if let Ok(ws) = WebSocket::open(&url) {
            let (mut tx, mut rx) = ws.split();
            // Greet so the host opens this client its session subscription.
            if let Ok(hello) = serde_json::to_string(&ClientFrame::Hello { last_seq: None }) {
                let _ = tx.send(Message::Text(hello)).await;
            }
            // Resend everything still pending on this fresh socket — a prior send
            // may have been lost to the drop. Same ids ⇒ the host de-dupes.
            let mut socket_lost = false;
            let now = now_ms();
            for p in pending.iter_mut() {
                p.attempts += 1;
                let (id, command) = (p.id.clone(), p.command.clone());
                if send_cmd(&mut tx, &instance, &id, &command).await.is_err() {
                    socket_lost = true;
                    break;
                }
                schedule(p, now, &policy);
            }
            // Pump inbound session frames, outbound commands, and retry ticks.
            while !socket_lost {
                let now = now_ms();
                // Service any pending command whose window elapsed: resend it, or
                // (retry budget spent) fail it.
                let mut i = 0;
                while i < pending.len() {
                    if pending[i].due_at_ms <= now {
                        if pending[i].expiring {
                            outcome.set(CommandOutcome::Failed(
                                "the host never acknowledged your action".to_string(),
                            ));
                            pending.remove(i);
                            continue;
                        }
                        pending[i].attempts += 1;
                        let (id, command) = (pending[i].id.clone(), pending[i].command.clone());
                        if send_cmd(&mut tx, &instance, &id, &command).await.is_err() {
                            socket_lost = true;
                            break;
                        }
                        schedule(&mut pending[i], now, &policy);
                    }
                    i += 1;
                }
                if socket_lost {
                    break;
                }
                let wait = next_wait_ms(&pending, now);
                select! {
                    msg = rx.next().fuse() => match msg {
                        Some(Ok(Message::Text(t))) => {
                            if let Some(payload) = session_payload(&t) {
                                state.set(RemoteState::Live(Box::new(payload)));
                            } else if let Some((id, ok, error)) = command_ack(&t) {
                                // The host acked THIS id — stop retrying it and
                                // surface the verdict instead of dropping it.
                                pending.retain(|p| p.id != id);
                                outcome.set(if ok {
                                    CommandOutcome::Applied
                                } else {
                                    CommandOutcome::Failed(
                                        error.unwrap_or_else(|| "the host rejected the command".to_string()),
                                    )
                                });
                            }
                        }
                        Some(Ok(_)) => continue,
                        _ => break, // closed/errored
                    },
                    cmd = cmd_rx.next().fuse() => {
                        // `None` = the UI's sender was dropped; just keep reading.
                        if let Some(command) = cmd {
                            let id = next_command_id();
                            outcome.set(CommandOutcome::Pending);
                            let ok = send_cmd(&mut tx, &instance, &id, &command).await.is_ok();
                            let mut p = PendingCmd {
                                id,
                                command,
                                attempts: 1,
                                due_at_ms: 0.0,
                                expiring: false,
                            };
                            schedule(&mut p, now_ms(), &policy);
                            pending.push(p);
                            if !ok {
                                // The socket died before the send landed; the
                                // pending entry survives and is resent on reconnect.
                                break;
                            }
                        }
                    },
                    _ = gloo_timers::future::sleep(Duration::from_millis(wait)).fuse() => {},
                }
            }
        }
        state.set(RemoteState::Reconnecting);
        gloo_timers::future::sleep(Duration::from_secs(3)).await;
    }
}

/// Decode a relay text frame to the session payload it carries, keeping only the
/// surfaces the web app renders: the checkpoint `Review` and the interactive
/// `Question` / `Direction` / `Picker` prompts the engine mirrors onto the run
/// feed. Other variants (view/visual_review/proof) return `None`, so the UI
/// keeps the last state it knew how to render.
fn session_payload(text: &str) -> Option<SessionPayload> {
    let frame = serde_json::from_str::<ServerFrame>(text).ok()?;
    let payload = match frame {
        ServerFrame::Snapshot { payload, .. } | ServerFrame::Update { payload, .. } => payload,
        _ => return None,
    };
    let session = serde_json::from_value::<SessionPayload>(payload).ok()?;
    match session {
        SessionPayload::Review(_)
        | SessionPayload::Question(_)
        | SessionPayload::Direction(_)
        | SessionPayload::Picker(_) => Some(session),
        _ => None,
    }
}

/// Decode a relay text frame to a command ack `(id, ok, error)`, if it is one.
/// The `id` matches the ack back to its pending command so retries stop.
fn command_ack(text: &str) -> Option<(String, bool, Option<String>)> {
    match serde_json::from_str::<ServerFrame>(text).ok()? {
        ServerFrame::Ack { id, ok, error } => Some((id, ok, error)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! Native (`#[test]`) coverage of the pure transport logic: the relay-URL
    //! construction from the page query, percent-decoding, the command-id
    //! generator, and the two frame decoders (session payload + command ack).
    //! None of these touch `web_sys`, so they run off-browser under
    //! `cargo test -p darkrun-app`.
    use super::*;
    use darkrun_api::common::{GateType, SessionStatus};
    use darkrun_api::session::{
        DirectionSessionPayload, PickerKind, PickerSessionPayload, QuestionSessionPayload,
        ReviewSessionPayload, SessionPayload, ViewMode, ViewSessionPayload, ViewStatus,
        VisualReviewSessionPayload,
    };

    /// Wrap a `SessionPayload` in a `Snapshot` frame's JSON, the way the host
    /// sends it — the exact bytes `session_payload` parses off the wire.
    fn snapshot_json(payload: &SessionPayload) -> String {
        let frame = ServerFrame::Snapshot {
            seq: 1,
            payload: serde_json::to_value(payload).unwrap(),
        };
        serde_json::to_string(&frame).unwrap()
    }

    fn review(gate: Option<GateType>) -> SessionPayload {
        SessionPayload::Review(ReviewSessionPayload {
            session_id: "sess-1".into(),
            status: SessionStatus::Pending,
            run_slug: Some("my-run".into()),
            gate_type: gate,
            ..Default::default()
        })
    }

    #[test]
    fn target_from_query_builds_the_relay_client_url() {
        let t = target_from_query("?relay=wss://relay.darkrun.ai&session=sess-1&token=tok-9")
            .expect("all three params present");
        assert_eq!(t.session, "sess-1");
        assert_eq!(t.url, "wss://relay.darkrun.ai/relay/client/sess-1?token=tok-9");
    }

    #[test]
    fn target_from_query_trims_a_trailing_relay_slash() {
        // A trailing slash on the relay base must not double up in the path.
        let t = target_from_query("relay=wss://relay.darkrun.ai/&session=s&token=t").unwrap();
        assert_eq!(t.url, "wss://relay.darkrun.ai/relay/client/s?token=t");
    }

    #[test]
    fn target_from_query_percent_decodes_the_relay_scheme() {
        // The relay arrives percent-encoded (`wss%3A%2F%2F…`); decode restores it.
        let t = target_from_query("relay=wss%3A%2F%2Frelay.darkrun.ai&session=s&token=t").unwrap();
        assert_eq!(t.url, "wss://relay.darkrun.ai/relay/client/s?token=t");
    }

    #[test]
    fn target_from_query_requires_all_three_params() {
        assert!(target_from_query("session=s&token=t").is_none()); // no relay
        assert!(target_from_query("relay=wss://r&token=t").is_none()); // no session
        assert!(target_from_query("relay=wss://r&session=s").is_none()); // no token
        assert!(target_from_query("").is_none());
        assert!(target_from_query("?").is_none());
    }

    #[test]
    fn target_from_query_rejects_empty_values() {
        // Present-but-empty is as good as absent — never build a half URL.
        assert!(target_from_query("relay=&session=s&token=t").is_none());
        assert!(target_from_query("relay=wss://r&session=&token=t").is_none());
        assert!(target_from_query("relay=wss://r&session=s&token=").is_none());
    }

    #[test]
    fn target_from_query_ignores_unrelated_params() {
        let t = target_from_query("web=https://x&relay=wss://r&foo=bar&session=s&token=t").unwrap();
        assert_eq!(t.url, "wss://r/relay/client/s?token=t");
    }

    #[test]
    fn decode_restores_percent_encoded_scheme_and_slashes() {
        assert_eq!(decode("wss%3A%2F%2Fhost"), "wss://host");
        assert_eq!(decode("a%2fb"), "a/b"); // lowercase %2f too
        assert_eq!(decode("plain"), "plain"); // nothing to decode
    }

    #[test]
    fn next_command_id_is_prefixed_and_strictly_monotonic() {
        let a = next_command_id();
        let b = next_command_id();
        assert!(a.starts_with('c'), "ids are `c<n>`: {a}");
        assert!(b.starts_with('c'));
        assert_ne!(a, b, "each id is unique");
        let na: u64 = a[1..].parse().unwrap();
        let nb: u64 = b[1..].parse().unwrap();
        assert_eq!(nb, na + 1, "ids increment by one");
    }

    #[test]
    fn session_payload_keeps_the_four_rendered_surfaces() {
        // Review / Question / Direction / Picker are the surfaces the web app
        // renders — each must survive the Snapshot round-trip.
        for payload in [
            review(Some(GateType::Ask)),
            SessionPayload::Question(QuestionSessionPayload {
                session_id: "q".into(),
                ..Default::default()
            }),
            SessionPayload::Direction(DirectionSessionPayload {
                session_id: "d".into(),
                ..Default::default()
            }),
            SessionPayload::Picker(PickerSessionPayload {
                session_id: "p".into(),
                status: SessionStatus::Pending,
                run_slug: None,
                kind: PickerKind::Factory,
                title: "pick".into(),
                prompt: "choose".into(),
                options: vec![],
                selection: None,
            }),
        ] {
            let got = session_payload(&snapshot_json(&payload))
                .unwrap_or_else(|| panic!("{} should be kept", payload.session_type()));
            assert_eq!(got.session_type(), payload.session_type());
        }
    }

    #[test]
    fn session_payload_reads_an_update_frame_too() {
        let frame = ServerFrame::Update {
            seq: 7,
            payload: serde_json::to_value(review(None)).unwrap(),
        };
        let got = session_payload(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(got.session_type(), "review");
    }

    #[test]
    fn session_payload_drops_non_rendered_variants() {
        // View / Proof / VisualReview are never rendered here → None, so the UI
        // holds the last state it knew how to draw.
        let view = SessionPayload::View(ViewSessionPayload {
            session_id: "v".into(),
            status: ViewStatus::Open,
            run_slug: "r".into(),
            scope: Default::default(),
            artifacts: vec![],
            factory: None,
            station: None,
            artifact: None,
            mode: ViewMode::Viewer,
            boot_port: None,
            boot_command: None,
        });
        assert!(session_payload(&snapshot_json(&view)).is_none());

        let visual = SessionPayload::VisualReview(VisualReviewSessionPayload {
            session_id: "vr".into(),
            ..Default::default()
        });
        assert!(session_payload(&snapshot_json(&visual)).is_none());
    }

    #[test]
    fn session_payload_rejects_acks_and_garbage() {
        let ack = ServerFrame::Ack { id: "c1".into(), ok: true, error: None };
        assert!(session_payload(&serde_json::to_string(&ack).unwrap()).is_none());
        assert!(session_payload("not json").is_none());
        assert!(session_payload("{}").is_none());
    }

    #[test]
    fn command_ack_reads_success_and_failure() {
        let ok = ServerFrame::Ack { id: "c1".into(), ok: true, error: None };
        assert_eq!(
            command_ack(&serde_json::to_string(&ok).unwrap()),
            Some(("c1".to_string(), true, None))
        );

        let bad = ServerFrame::Ack {
            id: "c2".into(),
            ok: false,
            error: Some("host rejected".into()),
        };
        assert_eq!(
            command_ack(&serde_json::to_string(&bad).unwrap()),
            Some(("c2".to_string(), false, Some("host rejected".into())))
        );
    }

    #[test]
    fn command_ack_ignores_non_ack_frames() {
        assert!(command_ack(&serde_json::to_string(&ServerFrame::Pong).unwrap()).is_none());
        assert!(command_ack(&snapshot_json(&review(None))).is_none());
        assert!(command_ack("garbage").is_none());
    }
}
