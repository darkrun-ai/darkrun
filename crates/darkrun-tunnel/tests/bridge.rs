//! End-to-end: relay + a fake local engine + the connector + a client. Proves a
//! remote client joins, reads the snapshot into the live session on connect, and
//! that a command crosses the tunnel into a local REST write and is acked.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message as AxMsg, WebSocket, WebSocketUpgrade};
use axum::extract::Path;
use axum::routing::{get, post};
use axum::Router;
use darkrun_api::tunnel::{ClientFrame, ServerFrame};
use darkrun_web::{relay_router, DevTokenAuth, Relay, RelayState};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as TMsg;

/// A fake engine server: a `/ws/session/{run}` that pushes a snapshot on connect,
/// and `/api/advance/{run}` that signals when hit.
async fn fake_engine(advance_tx: mpsc::UnboundedSender<String>) -> String {
    let app = Router::new()
        .route(
            "/ws/session/{run}",
            get(|ws: WebSocketUpgrade| async move {
                ws.on_upgrade(|mut socket: WebSocket| async move {
                    // The local server pushes a snapshot immediately on connect.
                    let _ = socket
                        .send(AxMsg::Text(r#"{"station":"frame","phase":"review"}"#.into()))
                        .await;
                    // Keep the socket open so the subscription stays live.
                    while socket.recv().await.is_some() {}
                })
            }),
        )
        .route(
            "/api/advance/{run}",
            post(|Path(run): Path<String>| async move {
                let _ = advance_tx.send(run);
                "ok"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Stand up the relay; returns its `127.0.0.1:port` authority.
async fn spawn_relay() -> String {
    let state = RelayState::new(Arc::new(Relay::new()), Arc::new(DevTokenAuth));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, relay_router(state)).await.unwrap();
    });
    addr.to_string()
}

#[tokio::test]
async fn client_reads_snapshot_then_a_command_hits_the_local_engine() {
    let (adv_tx, mut adv_rx) = mpsc::unbounded_channel::<String>();
    let local_http_base = fake_engine(adv_tx).await;
    let relay = spawn_relay().await;

    // The host connector dials the relay and bridges to the fake engine.
    let cfg = darkrun_tunnel::ConnectorConfig {
        relay_host_url: format!("ws://{relay}/relay/host/sess1?token=acct"),
        local_http_base,
        run: "run1".into(),
        reconnect: Duration::from_millis(100),
    };
    let connector = tokio::spawn(darkrun_tunnel::run(cfg));

    // A client attaches. Retry until the connector has registered as host (so the
    // attach succeeds) AND the first frame (the snapshot) arrives.
    let mut client = None;
    'outer: for _ in 0..50 {
        let Ok((mut sock, _)) =
            connect_async(format!("ws://{relay}/relay/client/sess1?token=acct")).await
        else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        // Greet (optional in the protocol; the snapshot flows from Join either way).
        let hello = serde_json::to_string(&ClientFrame::Hello { last_seq: None }).unwrap();
        let _ = sock.send(TMsg::Text(hello.into())).await;
        // Expect a snapshot within a short window; else the host wasn't up yet.
        match tokio::time::timeout(Duration::from_millis(500), sock.next()).await {
            Ok(Some(Ok(TMsg::Text(t)))) => {
                let frame: ServerFrame = serde_json::from_str(&t).unwrap();
                assert!(
                    matches!(frame, ServerFrame::Snapshot { .. }),
                    "first frame must be the snapshot, got {frame:?}"
                );
                client = Some(sock);
                break 'outer;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    let mut client = client.expect("client should attach and receive a snapshot");

    // Send an advance command; the connector translates it to POST /api/advance/run1.
    let cmd = serde_json::to_string(&ClientFrame::Cmd {
        instance: "tab-1".into(),
        id: "c1".into(),
        command: darkrun_api::tunnel::ClientCommand::Advance { run: "run1".into() },
    })
    .unwrap();
    client.send(TMsg::Text(cmd.into())).await.unwrap();

    // The local engine's advance endpoint is hit…
    let hit = tokio::time::timeout(Duration::from_secs(2), adv_rx.recv())
        .await
        .expect("advance should reach the local engine")
        .unwrap();
    assert_eq!(hit, "run1");

    // …and the client gets an ack for the command.
    let mut acked = false;
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_millis(500), client.next()).await {
            Ok(Some(Ok(TMsg::Text(t)))) => {
                if let Ok(ServerFrame::Ack { id, ok, .. }) = serde_json::from_str::<ServerFrame>(&t)
                {
                    assert_eq!(id, "c1");
                    assert!(ok, "advance ack should be ok");
                    acked = true;
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(acked, "client should receive an ack for the advance command");

    connector.abort();
}
