//! Host connector — durably bridges the **relay** to the engine's **local
//! HTTP/WS** server, so a remote client reaches a live run exactly as the desktop
//! does on loopback.
//!
//! The host dials OUTBOUND to the relay's host endpoint and parks a WebSocket
//! (NAT-proof). The relay then drives it with [`HostEvent`]s:
//!
//! - **`Join{client}`** → open that client its OWN local `/ws/session/{run}`
//!   subscription. The local server pushes a snapshot first, so the client reads
//!   into the live session on connect; each local push is wrapped as a
//!   [`ServerFrame`] (first → `Snapshot`, then `Update`) and routed back with
//!   [`HostCmd::To`].
//! - **`Leave{client}`** → tear that client's subscription down.
//! - **`Msg{client,data}`** → a [`ClientFrame`]: `Ping` → `Pong`; `Cmd` →
//!   translate the [`ClientCommand`] into the engine's local REST write and
//!   `Ack` it.
//!
//! Durability: the relay connection runs under [`run`], which reconnects with a
//! fixed backoff after any drop; a periodic WS ping detects a dead peer. On
//! reconnect every client re-attaches and gets a fresh snapshot, so no state is
//! lost. Command acks + client-side retry (the protocol's idempotent ids) make
//! writes exactly-once in effect.

use std::collections::HashMap;
use std::time::Duration;

use darkrun_api::tunnel::{ClientCommand, ClientFrame, HostCmd, HostEvent, Seq, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// How the connector reaches the relay and the local engine server.
#[derive(Debug, Clone)]
pub struct ConnectorConfig {
    /// The fully-formed relay host URL, incl. the session + token query
    /// (`wss://relay/relay/host/{session}?token=...`).
    pub relay_host_url: String,
    /// The local engine HTTP base (`http://127.0.0.1:{port}`); the WS base is
    /// derived by swapping the scheme.
    pub local_http_base: String,
    /// The run slug each remote client subscribes to.
    pub run: String,
    /// Backoff between relay reconnect attempts.
    pub reconnect: Duration,
}

impl ConnectorConfig {
    /// The local WS subscription URL for this run (`ws://…/ws/session/{run}`).
    fn local_ws_url(&self) -> String {
        let ws_base = self
            .local_http_base
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1);
        format!("{ws_base}/ws/session/{}", self.run)
    }
}

/// A local HTTP write the connector issues on the engine's loopback server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalRequest {
    method: Method,
    path: String,
    body: Option<Value>,
}

/// Map a [`ClientCommand`] to the engine's local REST write — the tunnel
/// equivalent of the desktop's REST calls. Pure, so it's unit-tested directly.
fn command_request(cmd: &ClientCommand) -> LocalRequest {
    match cmd {
        ClientCommand::Advance { run } => LocalRequest {
            method: Method::POST,
            path: format!("/api/advance/{run}"),
            body: None,
        },
        ClientCommand::Answer { session, answer } => LocalRequest {
            method: Method::POST,
            path: format!("/question/{session}/answer"),
            body: Some(answer.clone()),
        },
        ClientCommand::Feedback { run, station, body } => LocalRequest {
            method: Method::POST,
            path: format!("/api/feedback/{run}/{station}"),
            body: Some(serde_json::json!({ "body": body })),
        },
    }
}

/// Wrap a raw local push (a serialized review payload) into a [`ServerFrame`].
/// The first push of a (re)connection is the snapshot; the rest are updates.
/// Pure, so it's unit-tested directly.
fn wrap_payload(seq: Seq, raw: &str, first: bool) -> ServerFrame {
    let payload = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    if first {
        ServerFrame::Snapshot { seq, payload }
    } else {
        ServerFrame::Update { seq, payload }
    }
}

/// Serialize a [`ServerFrame`] addressed to `client` into the relay envelope
/// message, queued on `out`.
fn route_to_client(client: u64, frame: &ServerFrame, out: &mpsc::UnboundedSender<Message>) {
    if let Ok(data) = serde_json::to_string(frame) {
        if let Ok(json) = serde_json::to_string(&HostCmd::To { client, data }) {
            let _ = out.send(Message::Text(json.into()));
        }
    }
}

/// Run the host connector forever: connect to the relay, serve until the
/// connection drops, then reconnect after the configured backoff. Returns only
/// if the task is cancelled by the caller.
pub async fn run(cfg: ConnectorConfig) {
    let http = reqwest::Client::new();
    loop {
        if let Err(e) = serve_once(&cfg, &http).await {
            tracing::warn!("darkrun-tunnel: relay connection ended: {e}");
        }
        tokio::time::sleep(cfg.reconnect).await;
    }
}

/// One relay connection's lifetime: register, then bridge clients until the
/// socket closes or errors.
async fn serve_once(cfg: &ConnectorConfig, http: &reqwest::Client) -> Result<(), String> {
    let (ws, _) = connect_async(&cfg.relay_host_url)
        .await
        .map_err(|e| format!("dialing relay: {e}"))?;
    let (mut sink, mut stream) = ws.split();

    // One writer drains the outbound queue to the relay sink, so every task
    // (client subscriptions + command acks + the pinger) shares one sink safely.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Heartbeat: a periodic WS ping surfaces a dead relay fast (the read loop
    // then errors and we reconnect).
    let ping_tx = out_tx.clone();
    let pinger = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(20));
        tick.tick().await; // the first tick is immediate
        loop {
            tick.tick().await;
            if ping_tx.send(Message::Ping(Vec::new().into())).is_err() {
                break;
            }
        }
    });

    let mut clients: HashMap<u64, JoinHandle<()>> = HashMap::new();
    let result = read_loop(cfg, http, &mut stream, &out_tx, &mut clients).await;

    // Tear everything down — clients re-attach (with fresh snapshots) on reconnect.
    for (_, handle) in clients {
        handle.abort();
    }
    writer.abort();
    pinger.abort();
    result
}

/// Drive the relay event stream until it ends.
async fn read_loop(
    cfg: &ConnectorConfig,
    http: &reqwest::Client,
    stream: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
              + Unpin),
    out_tx: &mpsc::UnboundedSender<Message>,
    clients: &mut HashMap<u64, JoinHandle<()>>,
) -> Result<(), String> {
    while let Some(msg) = stream.next().await {
        let msg = msg.map_err(|e| format!("relay read: {e}"))?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = out_tx.send(Message::Pong(p));
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(event) = serde_json::from_str::<HostEvent>(&text) else {
            continue;
        };
        match event {
            HostEvent::Join { client } => {
                let handle =
                    spawn_client_subscription(client, cfg.local_ws_url(), out_tx.clone());
                if let Some(old) = clients.insert(client, handle) {
                    old.abort();
                }
            }
            HostEvent::Leave { client } => {
                if let Some(handle) = clients.remove(&client) {
                    handle.abort();
                }
            }
            HostEvent::Msg { client, data } => {
                handle_client_frame(client, &data, &cfg.local_http_base, http.clone(), out_tx.clone());
            }
        }
    }
    Ok(())
}

/// Open a client's local session subscription and forward each push back to it,
/// wrapped as a [`ServerFrame`]. The local server's first push is the snapshot.
fn spawn_client_subscription(
    client: u64,
    local_ws_url: String,
    out: mpsc::UnboundedSender<Message>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok((ws, _)) = connect_async(&local_ws_url).await else {
            return;
        };
        let (_sink, mut stream) = ws.split();
        let mut seq: Seq = 0;
        let mut first = true;
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(t) = msg {
                let frame = wrap_payload(seq, &t, first);
                first = false;
                seq = seq.saturating_add(1);
                route_to_client(client, &frame, &out);
            }
        }
    })
}

/// Handle one [`ClientFrame`] from a client: heartbeat or a write command.
fn handle_client_frame(
    client: u64,
    data: &str,
    local_http_base: &str,
    http: reqwest::Client,
    out: mpsc::UnboundedSender<Message>,
) {
    let Ok(frame) = serde_json::from_str::<ClientFrame>(data) else {
        return;
    };
    match frame {
        ClientFrame::Ping => route_to_client(client, &ServerFrame::Pong, &out),
        // The subscription opened on Join already streams the snapshot.
        ClientFrame::Hello { .. } => {}
        ClientFrame::Cmd { id, command } => {
            let base = local_http_base.to_string();
            tokio::spawn(async move {
                let ack = match exec_command(&http, &base, &command).await {
                    Ok(()) => ServerFrame::Ack { id, ok: true, error: None },
                    Err(e) => ServerFrame::Ack { id, ok: false, error: Some(e) },
                };
                route_to_client(client, &ack, &out);
            });
        }
    }
}

/// Issue a command's local REST write against the engine's loopback server.
async fn exec_command(
    http: &reqwest::Client,
    base: &str,
    command: &ClientCommand,
) -> Result<(), String> {
    let req = command_request(command);
    let url = format!("{base}{}", req.path);
    let mut builder = http.request(req.method, &url);
    if let Some(body) = req.body {
        builder = builder.json(&body);
    }
    let resp = builder.send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("local server returned {}", resp.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ws_url_swaps_scheme_and_targets_the_run() {
        let cfg = ConnectorConfig {
            relay_host_url: "wss://relay/relay/host/s?token=t".into(),
            local_http_base: "http://127.0.0.1:4317".into(),
            run: "quiet-canyon".into(),
            reconnect: Duration::from_secs(1),
        };
        assert_eq!(cfg.local_ws_url(), "ws://127.0.0.1:4317/ws/session/quiet-canyon");
    }

    #[test]
    fn command_request_maps_each_command_to_its_local_write() {
        let adv = command_request(&ClientCommand::Advance { run: "r".into() });
        assert_eq!(adv.method, Method::POST);
        assert_eq!(adv.path, "/api/advance/r");
        assert_eq!(adv.body, None);

        let ans = command_request(&ClientCommand::Answer {
            session: "sess".into(),
            answer: serde_json::json!({"choice": "a"}),
        });
        assert_eq!(ans.path, "/question/sess/answer");
        assert_eq!(ans.body, Some(serde_json::json!({"choice": "a"})));

        let fb = command_request(&ClientCommand::Feedback {
            run: "r".into(),
            station: "build".into(),
            body: "fix".into(),
        });
        assert_eq!(fb.path, "/api/feedback/r/build");
        assert_eq!(fb.body, Some(serde_json::json!({"body": "fix"})));
    }

    #[test]
    fn wrap_payload_first_is_snapshot_then_updates() {
        let snap = wrap_payload(0, r#"{"station":"frame"}"#, true);
        assert_eq!(
            snap,
            ServerFrame::Snapshot { seq: 0, payload: serde_json::json!({"station": "frame"}) }
        );
        let upd = wrap_payload(1, r#"{"station":"build"}"#, false);
        assert_eq!(
            upd,
            ServerFrame::Update { seq: 1, payload: serde_json::json!({"station": "build"}) }
        );
        // Non-JSON payloads degrade to null rather than dropping the frame.
        assert_eq!(
            wrap_payload(2, "not json", false),
            ServerFrame::Update { seq: 2, payload: Value::Null }
        );
    }
}
