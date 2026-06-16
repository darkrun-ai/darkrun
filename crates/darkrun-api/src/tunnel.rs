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
//!    - writes are [`ClientFrame::Cmd`] with a client-generated `id`; the host
//!      [`ServerFrame::Ack`]s it. The client retries unacked commands on
//!      reconnect, and the `id` makes them idempotent (at-least-once delivery +
//!      dedup = exactly-once effect).
//!
//! [`Reachability`] expresses the **local-vs-remote** choice: a host publishes a
//! local candidate (loopback/LAN, tried first) and/or the relay candidate
//! (remote fallback). The same app protocol rides either transport, so the
//! client just races local-first then relay — the ICE-lite of our relay model.

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
        /// Idempotency / ack key (client-generated, unique per command).
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
            id: "c1".into(),
            command: ClientCommand::Advance { run: "r".into() },
        };
        let c = serde_json::to_string(&cmd).unwrap();
        assert_eq!(serde_json::from_str::<ClientFrame>(&c).unwrap(), cmd);

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
