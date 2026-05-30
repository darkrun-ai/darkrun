//! `run_session_feed` end-to-end against a real loopback WebSocket server, plus
//! `FeedEvent` variant construction. Exercises the decode/skip/close/disconnect
//! branches of the feed loop.

use std::sync::{Arc, Mutex};

use darkrun_api::session::SessionPayload;
use darkrun_desktop::wire::{run_session_feed, ConnConfig, FeedEvent};
use futures_util::SinkExt;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Start a WS server that, on connect, sends `frames` then closes. Returns a
/// `ConnConfig` pointed at it with the given session id.
async fn ws_server(frames: Vec<Message>) -> (ConnConfig, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        for f in frames {
            // Stop early if the peer hung up.
            if ws.send(f).await.is_err() {
                return;
            }
        }
        let _ = ws.close(None).await;
    });
    let cfg = ConnConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        session_id: "feed".to_string(),
    };
    (cfg, handle)
}

fn review_frame(id: &str) -> Message {
    Message::Text(
        json!({
            "session_type": "review",
            "session_id": id,
            "status": "pending"
        })
        .to_string()
        .into(),
    )
}

fn collect_events(frames: Vec<Message>) -> Vec<FeedEvent> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let (cfg, handle) = ws_server(frames).await;
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        run_session_feed(&cfg, move |e| sink.lock().unwrap().push(e)).await;
        handle.await.unwrap();
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    })
}

#[test]
fn feed_delivers_payload_then_disconnect_on_close() {
    let events = collect_events(vec![review_frame("a")]);
    // One payload, then a Disconnected from the server-initiated close.
    assert!(events.len() >= 2, "events: {events:?}");
    match &events[0] {
        FeedEvent::Payload(p) => {
            assert_eq!(p.session_type(), "review");
            assert_eq!(p.session_id(), "a");
        }
        other => panic!("expected payload first, got {other:?}"),
    }
    assert!(
        matches!(events.last().unwrap(), FeedEvent::Disconnected(_)),
        "last event should be Disconnected"
    );
}

#[test]
fn feed_skips_non_session_text_frames() {
    let events = collect_events(vec![
        Message::Text(r#"{"type":"ping"}"#.into()),
        review_frame("real"),
        Message::Text("garbage".into()),
    ]);
    // Exactly one payload survives the skips.
    let payloads: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, FeedEvent::Payload(_)))
        .collect();
    assert_eq!(payloads.len(), 1, "events: {events:?}");
    if let FeedEvent::Payload(p) = payloads[0] {
        assert_eq!(p.session_id(), "real");
    }
}

#[test]
fn feed_delivers_multiple_payloads_in_order() {
    let events = collect_events(vec![
        review_frame("one"),
        review_frame("two"),
        review_frame("three"),
    ]);
    let ids: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            FeedEvent::Payload(p) => Some(p.session_id()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec!["one", "two", "three"]);
}

#[test]
fn feed_decodes_binary_session_frame() {
    let bytes = json!({
        "session_type": "review",
        "session_id": "bin",
        "status": "approved"
    })
    .to_string()
    .into_bytes();
    let events = collect_events(vec![Message::Binary(bytes.into())]);
    let payloads: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, FeedEvent::Payload(_)))
        .collect();
    assert_eq!(payloads.len(), 1);
    if let FeedEvent::Payload(p) = payloads[0] {
        assert_eq!(p.session_id(), "bin");
    }
}

#[test]
fn feed_ignores_malformed_binary_without_emitting() {
    let events = collect_events(vec![Message::Binary(b"not-json".to_vec().into())]);
    // No payload; the malformed binary frame is silently dropped, then the
    // server close produces a Disconnected.
    assert!(
        events.iter().all(|e| matches!(e, FeedEvent::Disconnected(_))),
        "events: {events:?}"
    );
}

#[test]
fn feed_close_reason_is_surfaced() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let events = rt.block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
            use tokio_tungstenite::tungstenite::protocol::CloseFrame;
            let _ = ws
                .close(Some(CloseFrame {
                    code: CloseCode::Away,
                    reason: "bye".into(),
                }))
                .await;
        });
        let cfg = ConnConfig {
            host: "127.0.0.1".to_string(),
            port: addr.port(),
            session_id: "x".to_string(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        run_session_feed(&cfg, move |e| sink.lock().unwrap().push(e)).await;
        handle.await.unwrap();
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    });
    let last = events.last().expect("at least one event");
    match last {
        FeedEvent::Disconnected(reason) => {
            // Carries the close code + reason text.
            assert!(reason.contains("bye"), "reason: {reason}");
            assert!(reason.starts_with("closed"), "reason: {reason}");
        }
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

#[test]
fn feed_disconnects_when_connect_fails() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let events = rt.block_on(async move {
        // Dead port: bind then drop.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let cfg = ConnConfig {
            host: "127.0.0.1".to_string(),
            port,
            session_id: "x".to_string(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        run_session_feed(&cfg, move |e| sink.lock().unwrap().push(e)).await;
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    });
    assert_eq!(events.len(), 1);
    match &events[0] {
        FeedEvent::Disconnected(reason) => {
            assert!(reason.contains("connect failed"), "reason: {reason}");
        }
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

// ---- FeedEvent construction / shape ----

#[test]
fn feed_event_payload_variant_holds_boxed_payload() {
    let frame = json!({
        "session_type": "review",
        "session_id": "fe",
        "status": "pending"
    })
    .to_string();
    let payload: SessionPayload = serde_json::from_str(&frame).unwrap();
    let ev = FeedEvent::Payload(Box::new(payload));
    match ev {
        FeedEvent::Payload(p) => assert_eq!(p.session_id(), "fe"),
        _ => panic!(),
    }
}

#[test]
fn feed_event_disconnected_carries_reason() {
    let ev = FeedEvent::Disconnected("stream ended".to_string());
    match ev {
        FeedEvent::Disconnected(r) => assert_eq!(r, "stream ended"),
        _ => panic!(),
    }
}

#[test]
fn feed_event_is_cloneable() {
    let ev = FeedEvent::Disconnected("x".to_string());
    let cloned = ev.clone();
    match cloned {
        FeedEvent::Disconnected(r) => assert_eq!(r, "x"),
        _ => panic!(),
    }
}
