//! Tunnel protocol — the wire contract for **remote and local** access to a live
//! run, shared by the relay, the host connector, and every client (desktop, web,
//! mobile). Pure serde types, so the same protocol compiles native AND to wasm.
//!
//! Two layers ride one WebSocket:
//!
//! 1. **Relay routing envelope** ([`HostEvent`] / [`HostCmd`]) — between the
//!    relay and the host. The relay addresses each client by id so the host can
//!    open that client its own session subscription (snapshot on connect) and
//!    reply to just it. The relay parses ONLY this envelope, never the payload.
//!
//! 2. **App protocol** ([`ClientFrame`] / [`ServerFrame`]) — end-to-end between a
//!    client and the host, carried INSIDE the envelope's `data`. This is the
//!    durable, rock-solid layer:
//!    - the host sends a [`ServerFrame::Snapshot`] on every (re)connect — full
//!      state, so a reconnect never loses state;
//!    - heartbeats ([`ClientFrame::Ping`] / [`ServerFrame::Pong`]) detect a dead
//!      peer fast; the client reconnects with backoff;
//!    - writes are [`ClientFrame::Cmd`] carrying a stable `instance` id (the
//!      client's page load) plus a client-generated `id`; the host
//!      [`ServerFrame::Ack`]s it. The client retries unacked commands on
//!      reconnect, and the host de-dupes on `(instance, id)` — stable across
//!      reconnects, so a resend on a FRESH socket collapses to one effect
//!      (at-least-once delivery + dedup = exactly-once effect).
//!
//! [`Reachability`] expresses the **local-vs-remote** choice: a host publishes a
//! local candidate (loopback/LAN, tried first) and/or the relay candidate
//! (remote fallback). The same app protocol rides either transport, so the
//! client just races local-first then relay — the ICE-lite of our relay model.

use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A monotonic sequence number on server frames. A client uses it to tell a
/// fresh [`ServerFrame::Snapshot`] from stale updates and to detect gaps.
pub type Seq = u64;

// ─── Layer 1: relay routing envelope (relay ↔ host) ──────────────────────────

/// Delivered by the relay TO the host: a client lifecycle event or a client
/// frame, tagged with the client id so the host can open/close per-client
/// subscriptions and attribute incoming commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostEvent {
    /// A client attached — open its session subscription and stream it back,
    /// addressed to this client.
    Join {
        /// The relay-assigned client id.
        client: u64,
    },
    /// A client detached — tear down that client's subscription.
    Leave {
        /// The client id that left.
        client: u64,
    },
    /// A frame the client sent (a serialized [`ClientFrame`]).
    Msg {
        /// The originating client id.
        client: u64,
        /// The client's raw text frame.
        data: String,
    },
}

/// Sent by the host TO the relay: route a frame (a serialized [`ServerFrame`]) to
/// one client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostCmd {
    /// Deliver `data` to client `client`.
    To {
        /// The destination client id.
        client: u64,
        /// The raw text frame to deliver.
        data: String,
    },
    /// Fan a push notification out to the session owner's registered remote
    /// devices. The relay resolves the owner (fixed at host registration) and
    /// pushes over FCM — the REMOTE half of "notify as the engine ticks",
    /// complementary to the host's local OS notification. It carries no client
    /// id: it targets the account's devices, not one attached client.
    Notify {
        /// The notification title.
        title: String,
        /// The notification body.
        body: String,
    },
}

// ─── Layer 2: app protocol (client ↔ host, inside the envelope `data`) ────────

/// A frame a client sends to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientFrame {
    /// First frame on every (re)connect. `last_seq` is the highest [`Seq`] the
    /// client has applied; the host always answers with a fresh `Snapshot`
    /// (simple + safe — full resync, no lost state).
    Hello {
        /// The last applied sequence, if reconnecting.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_seq: Option<Seq>,
    },
    /// A heartbeat; the host replies [`ServerFrame::Pong`].
    Ping,
    /// A write command, with a client-generated `id` for ack + idempotency.
    Cmd {
        /// The client's STABLE per-page-load instance id (one browser tab / app
        /// launch): unique per client, kept across reconnects. It — not the
        /// relay's per-socket connection id — anchors the host's dedup, so a
        /// resend on a FRESH socket after a drop still collapses to one effect,
        /// while two tabs (each numbering `id` from zero) never collide.
        /// `#[serde(default)]` so a pre-`instance` frame degrades to the empty
        /// instance rather than failing to parse.
        #[serde(default)]
        instance: String,
        /// Idempotency / ack key (client-generated, unique per command within an
        /// `instance`).
        id: String,
        /// The write to apply.
        command: ClientCommand,
    },
}

/// A frame the host sends to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerFrame {
    /// Full current state — sent on every connect/reconnect. Supersedes any
    /// prior state the client held.
    Snapshot {
        /// The sequence this snapshot is current as of.
        seq: Seq,
        /// The review payload (an opaque session JSON the client renders).
        payload: serde_json::Value,
    },
    /// A live update at `seq`, applied on top of the latest snapshot.
    Update {
        /// This update's sequence (monotonic).
        seq: Seq,
        /// The updated review payload.
        payload: serde_json::Value,
    },
    /// The result of a [`ClientFrame::Cmd`], keyed by its `id`.
    Ack {
        /// The command id this acks.
        id: String,
        /// Whether the command applied.
        ok: bool,
        /// The failure reason when `!ok`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Heartbeat reply to [`ClientFrame::Ping`].
    Pong,
}

/// A write a client pushes onto the host — the tunnel equivalent of the desktop's
/// REST writes. The host connector translates each into the engine's local call.
/// Extensible: new operations add variants without breaking the envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientCommand {
    /// Clear a gate / wake the run past a hold (`POST /api/advance/:id`).
    Advance {
        /// The run slug.
        run: String,
    },
    /// Resolve an interactive session (answer a question/direction/picker).
    Answer {
        /// The interactive session id.
        session: String,
        /// The answer payload (shape depends on the session kind).
        answer: serde_json::Value,
    },
    /// File a feedback item on a station (`POST /api/feedback/:run/:station`).
    Feedback {
        /// The run slug.
        run: String,
        /// The target station.
        station: String,
        /// The feedback body.
        body: String,
    },
    /// Decide a checkpoint gate (`POST /review/:session/decide`) — approve to
    /// clear it, or request changes with a note to route the station back as
    /// rework. The remote mirror of the desktop's checkpoint decision, so a
    /// remote operator can clear a gate or send it back.
    Decide {
        /// The review session id the gate belongs to.
        session: String,
        /// The raw decision: `approved` clears the gate; anything else the
        /// engine canonicalizes to `changes_requested`.
        decision: String,
        /// Optional reviewer note shipped with a request-changes decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Choose a design DIRECTION archetype (`POST /direction/:session/select`).
    /// The direction half of the interactive round-trip — the operator's choice
    /// is routed to the direction-select endpoint, not the question one.
    Direction {
        /// The direction session id.
        session: String,
        /// The chosen archetype id (must match one of the session's archetypes).
        archetype: String,
    },
    /// Select a blocking PICKER option (`POST /picker/:session/select`). The
    /// picker half of the interactive round-trip.
    Picker {
        /// The picker session id.
        session: String,
        /// The chosen option id (must match one of the session's options).
        option: String,
    },
}

/// Client-side retry policy for an unacked [`ClientFrame::Cmd`]. A client resends
/// a command with its ORIGINAL `id` (and its stable `instance`) after an ack times
/// out — bounded exponential backoff, capped total attempts. Combined with the
/// host's `(instance, id)` dedup — keyed on the client's STABLE page-load
/// `instance`, NOT the relay's per-socket connection id — a resend even on a fresh
/// socket after a reconnect runs **at most once** but **at least once**:
/// exactly-once in effect.
///
/// Pure timing math (no clock, no I/O), so every client — web, mobile, desktop —
/// shares one tested decision core rather than reinventing the backoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// The wait before the FIRST resend; each further resend doubles it.
    pub base: Duration,
    /// The backoff ceiling — the doubling never exceeds this.
    pub cap: Duration,
    /// Total sends allowed for one command (the first send plus its resends). A
    /// command sent this many times with no ack is given up on.
    pub max_attempts: u32,
}

impl RetryPolicy {
    /// The default client policy: 1s base, 8s ceiling, 5 total sends.
    pub const DEFAULT: RetryPolicy = RetryPolicy {
        base: Duration::from_secs(1),
        cap: Duration::from_secs(8),
        max_attempts: 5,
    };

    /// Decide what to do after a command has been sent `attempts` times (`>= 1`)
    /// without an ack: [`RetryStep::Resend`] with the backoff to wait before the
    /// next send, or [`RetryStep::GiveUp`] once the attempt cap is reached (or on
    /// the degenerate `attempts == 0`).
    pub fn step(&self, attempts: u32) -> RetryStep {
        if attempts == 0 || attempts >= self.max_attempts {
            return RetryStep::GiveUp;
        }
        // Exponential: base, base·2, base·4, … clamped to `cap`. Work in u64
        // milliseconds so the shift can never overflow.
        let base_ms = self.base.as_millis().min(u128::from(u64::MAX)) as u64;
        let cap_ms = self.cap.as_millis().min(u128::from(u64::MAX)) as u64;
        let shift = (attempts - 1).min(20);
        let ms = base_ms.saturating_mul(1u64 << shift).min(cap_ms);
        RetryStep::Resend {
            backoff: Duration::from_millis(ms),
        }
    }
}

/// The decision from [`RetryPolicy::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryStep {
    /// Resend the command (with the SAME id) after waiting `backoff`.
    Resend {
        /// The delay before the next send.
        backoff: Duration,
    },
    /// The attempt cap is reached — stop retrying and surface a failure.
    GiveUp,
}

// ─── Local-vs-remote reachability ────────────────────────────────────────────

/// How a client can reach a host's live session. A host publishes this to the
/// session registry; the client prefers [`local`](Reachability::local) (a fast
/// direct connect to loopback/LAN, like the desktop) and falls back to
/// [`relay`](Reachability::relay). The same app protocol rides either, so
/// selection is a transport detail above which nothing changes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Reachability {
    /// The direct candidate (same machine or LAN). Tried first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalCandidate>,
    /// The relay candidate (remote fallback / NAT-crossing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<RelayCandidate>,
}

/// A direct host endpoint — `ws://{host}:{port}/...` reachable without the relay
/// (loopback for same-machine, a LAN ip for same-network).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LocalCandidate {
    /// Host/ip the engine's HTTP/WS server is reachable at (e.g. `127.0.0.1`).
    pub host: String,
    /// The engine's bound port.
    pub port: u16,
}

/// The relay endpoint + the session id to attach to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RelayCandidate {
    /// The relay base URL (e.g. `wss://relay.darkrun.ai`).
    pub url: String,
    /// The session id the host registered under.
    pub session: String,
}

impl LocalCandidate {
    /// The direct review WS URL for `run` — `ws://{host}:{port}/ws/session/{run}`.
    /// No token: a local session is reachable on loopback/LAN without auth.
    pub fn ws_url(&self, run: &str) -> String {
        format!("ws://{}:{}/ws/session/{}", self.host, self.port, run)
    }
}

impl RelayCandidate {
    /// The relay client WS URL — `{url}/relay/client/{session}?token={token}`.
    /// The token (from `/darkrun:darkrun-login`) is required: remote needs auth.
    pub fn client_ws_url(&self, token: &str) -> String {
        format!(
            "{}/relay/client/{}?token={}",
            self.url.trim_end_matches('/'),
            self.session,
            token
        )
    }
}

/// Which transport a [`ConnectCandidate`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    /// A direct connection to the host's loopback/LAN server (no relay, no auth).
    Local,
    /// A connection through the relay (remote; needs the login token).
    Relay,
}

/// One connection the client may try, with its WS URL — yielded in preference
/// order by [`Reachability::connect_candidates`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectCandidate {
    /// Local (direct) or relay (remote).
    pub kind: CandidateKind,
    /// The WebSocket URL to open.
    pub url: String,
}

impl Reachability {
    /// The connection candidates to try, **local first**: the direct loopback/LAN
    /// endpoint (no auth) ahead of the relay (which needs `token`). The client
    /// races them in order — a same-machine/LAN client connects directly and
    /// skips the relay; everyone else falls through to it. The relay candidate is
    /// dropped when no `token` is available (not logged in → remote unavailable).
    pub fn connect_candidates(&self, run: &str, token: Option<&str>) -> Vec<ConnectCandidate> {
        let mut out = Vec::new();
        if let Some(local) = &self.local {
            out.push(ConnectCandidate {
                kind: CandidateKind::Local,
                url: local.ws_url(run),
            });
        }
        if let (Some(relay), Some(token)) = (&self.relay, token) {
            out.push(ConnectCandidate {
                kind: CandidateKind::Relay,
                url: relay.client_ws_url(token),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_event_and_cmd_round_trip_with_tagged_json() {
        let join = HostEvent::Join { client: 7 };
        let j = serde_json::to_string(&join).unwrap();
        assert_eq!(j, r#"{"type":"join","client":7}"#);
        assert_eq!(serde_json::from_str::<HostEvent>(&j).unwrap(), join);

        let to = HostCmd::To { client: 7, data: "frame".into() };
        let t = serde_json::to_string(&to).unwrap();
        assert_eq!(serde_json::from_str::<HostCmd>(&t).unwrap(), to);

        // A notify command carries title/body and no client id.
        let notify = HostCmd::Notify { title: "darkrun · r".into(), body: "Build needs you.".into() };
        let n = serde_json::to_string(&notify).unwrap();
        assert!(n.contains(r#""type":"notify""#));
        assert!(!n.contains("client"));
        assert_eq!(serde_json::from_str::<HostCmd>(&n).unwrap(), notify);
    }

    #[test]
    fn client_and_server_frames_round_trip() {
        let hello = ClientFrame::Hello { last_seq: Some(42) };
        let h = serde_json::to_string(&hello).unwrap();
        assert!(h.contains(r#""t":"hello""#));
        assert_eq!(serde_json::from_str::<ClientFrame>(&h).unwrap(), hello);

        // Ping carries no fields beyond its tag.
        assert_eq!(serde_json::to_string(&ClientFrame::Ping).unwrap(), r#"{"t":"ping"}"#);

        let cmd = ClientFrame::Cmd {
            instance: "i1".into(),
            id: "c1".into(),
            command: ClientCommand::Advance { run: "r".into() },
        };
        let c = serde_json::to_string(&cmd).unwrap();
        assert!(c.contains(r#""instance":"i1""#));
        assert_eq!(serde_json::from_str::<ClientFrame>(&c).unwrap(), cmd);

        // A pre-`instance` frame (no `instance` field) degrades to the empty
        // instance rather than failing to parse.
        let legacy = r#"{"t":"cmd","id":"c1","command":{"kind":"advance","run":"r"}}"#;
        assert_eq!(
            serde_json::from_str::<ClientFrame>(legacy).unwrap(),
            ClientFrame::Cmd {
                instance: String::new(),
                id: "c1".into(),
                command: ClientCommand::Advance { run: "r".into() },
            }
        );

        let snap = ServerFrame::Snapshot { seq: 1, payload: serde_json::json!({"x": 1}) };
        let s = serde_json::to_string(&snap).unwrap();
        assert_eq!(serde_json::from_str::<ServerFrame>(&s).unwrap(), snap);

        let ack = ServerFrame::Ack { id: "c1".into(), ok: true, error: None };
        let a = serde_json::to_string(&ack).unwrap();
        // A successful ack omits the error field.
        assert!(!a.contains("error"));
        assert_eq!(serde_json::from_str::<ServerFrame>(&a).unwrap(), ack);
    }

    #[test]
    fn client_commands_are_tagged_by_kind() {
        let fb = ClientCommand::Feedback {
            run: "r".into(),
            station: "build".into(),
            body: "fix this".into(),
        };
        let j = serde_json::to_string(&fb).unwrap();
        assert!(j.contains(r#""kind":"feedback""#));
        assert_eq!(serde_json::from_str::<ClientCommand>(&j).unwrap(), fb);
    }

    #[test]
    fn decide_command_round_trips_and_is_kind_tagged() {
        // Approve carries no note — the field is skipped on the wire.
        let approve = ClientCommand::Decide {
            session: "s1".into(),
            decision: "approved".into(),
            note: None,
        };
        let a = serde_json::to_string(&approve).unwrap();
        assert!(a.contains(r#""kind":"decide""#));
        assert!(!a.contains("note"), "an approve omits the empty note");
        assert_eq!(serde_json::from_str::<ClientCommand>(&a).unwrap(), approve);

        // Request-changes ships the reviewer note.
        let changes = ClientCommand::Decide {
            session: "s1".into(),
            decision: "changes_requested".into(),
            note: Some("fix the header".into()),
        };
        let c = serde_json::to_string(&changes).unwrap();
        assert!(c.contains("fix the header"));
        assert_eq!(serde_json::from_str::<ClientCommand>(&c).unwrap(), changes);
    }

    #[test]
    fn direction_and_picker_commands_round_trip() {
        let dir = ClientCommand::Direction {
            session: "d1".into(),
            archetype: "bold".into(),
        };
        let d = serde_json::to_string(&dir).unwrap();
        assert!(d.contains(r#""kind":"direction""#));
        assert_eq!(serde_json::from_str::<ClientCommand>(&d).unwrap(), dir);

        let pick = ClientCommand::Picker {
            session: "p1".into(),
            option: "quick".into(),
        };
        let p = serde_json::to_string(&pick).unwrap();
        assert!(p.contains(r#""kind":"picker""#));
        assert_eq!(serde_json::from_str::<ClientCommand>(&p).unwrap(), pick);
    }

    #[test]
    fn retry_policy_backs_off_exponentially_then_gives_up() {
        let p = RetryPolicy {
            base: Duration::from_secs(1),
            cap: Duration::from_secs(8),
            max_attempts: 5,
        };
        // attempts 1..4 resend with a doubling backoff, clamped to the cap.
        assert_eq!(p.step(1), RetryStep::Resend { backoff: Duration::from_secs(1) });
        assert_eq!(p.step(2), RetryStep::Resend { backoff: Duration::from_secs(2) });
        assert_eq!(p.step(3), RetryStep::Resend { backoff: Duration::from_secs(4) });
        // 8s (would be 8s) hits the cap exactly.
        assert_eq!(p.step(4), RetryStep::Resend { backoff: Duration::from_secs(8) });
        // The 5th send reaches the cap → give up (no 6th send).
        assert_eq!(p.step(5), RetryStep::GiveUp);
        assert_eq!(p.step(6), RetryStep::GiveUp);
        // The degenerate zero-attempts case gives up rather than resending.
        assert_eq!(p.step(0), RetryStep::GiveUp);
    }

    #[test]
    fn retry_policy_backoff_never_exceeds_the_cap() {
        // A tiny cap clamps every backoff, and a huge attempt can't overflow.
        let p = RetryPolicy {
            base: Duration::from_millis(500),
            cap: Duration::from_secs(2),
            max_attempts: 100,
        };
        for attempts in 1..20u32 {
            if let RetryStep::Resend { backoff } = p.step(attempts) {
                assert!(backoff <= p.cap, "backoff {backoff:?} exceeded cap");
            } else {
                panic!("attempts {attempts} under the cap must resend");
            }
        }
        assert_eq!(p.step(100), RetryStep::GiveUp);
    }

    #[test]
    fn connect_candidates_prefer_local_then_relay() {
        let both = Reachability {
            local: Some(LocalCandidate { host: "127.0.0.1".into(), port: 4317 }),
            relay: Some(RelayCandidate {
                url: "wss://relay.darkrun.ai/".into(), // trailing slash trimmed
                session: "sess-1".into(),
            }),
        };
        let cands = both.connect_candidates("my-run", Some("tok"));
        assert_eq!(cands.len(), 2);
        // Local first, no token, direct WS URL.
        assert_eq!(cands[0].kind, CandidateKind::Local);
        assert_eq!(cands[0].url, "ws://127.0.0.1:4317/ws/session/my-run");
        // Relay second, with the token.
        assert_eq!(cands[1].kind, CandidateKind::Relay);
        assert_eq!(
            cands[1].url,
            "wss://relay.darkrun.ai/relay/client/sess-1?token=tok"
        );
    }

    #[test]
    fn connect_candidates_drop_relay_without_a_token() {
        let r = Reachability {
            local: Some(LocalCandidate { host: "127.0.0.1".into(), port: 9 }),
            relay: Some(RelayCandidate { url: "wss://r".into(), session: "s".into() }),
        };
        // No token (not logged in) → only the local candidate is offered.
        let cands = r.connect_candidates("run", None);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].kind, CandidateKind::Local);

        // Relay-only reachability with no token → nothing connectable.
        let relay_only = Reachability {
            local: None,
            relay: Some(RelayCandidate { url: "wss://r".into(), session: "s".into() }),
        };
        assert!(relay_only.connect_candidates("run", None).is_empty());
    }

    #[test]
    fn reachability_prefers_local_and_omits_empty() {
        // Empty reachability serializes to `{}` (both candidates skipped).
        assert_eq!(serde_json::to_string(&Reachability::default()).unwrap(), "{}");

        let both = Reachability {
            local: Some(LocalCandidate { host: "127.0.0.1".into(), port: 4317 }),
            relay: Some(RelayCandidate {
                url: "wss://relay.darkrun.ai".into(),
                session: "sess-1".into(),
            }),
        };
        let round: Reachability =
            serde_json::from_str(&serde_json::to_string(&both).unwrap()).unwrap();
        assert_eq!(round, both);
    }
}
