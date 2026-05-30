//! Decoding of the view / visual-review / proof session frames the feed pushes,
//! the domain->UI mapping for each, and the live `submit_output_review` HTTP
//! round-trip over loopback.

use darkrun_api::proof::Surface;
use darkrun_api::session::{SessionPayload, ViewArtifactKind};
use darkrun_api::{OutputReviewRequest, VisualReviewAnnotations, VisualReviewPin};
use darkrun_desktop::map;
use darkrun_desktop::wire::{submit_output_review, ConnConfig};
use darkrun_ui::components::proof_panel::ProofMetricKind;
use darkrun_ui::view::{ArtifactKind, VitalVerdict};
use serde_json::json;

fn decode(text: &str) -> Option<SessionPayload> {
    serde_json::from_str::<SessionPayload>(text).ok()
}

// ---------------------------------------------------------------------------
// frame decoding
// ---------------------------------------------------------------------------

#[test]
fn decodes_view_frame_with_artifacts() {
    let frame = json!({
        "session_type": "view",
        "session_id": "v1",
        "status": "open",
        "run_slug": "my-run",
        "mode": "viewer",
        "artifacts": [
            { "id": "shot", "path": "out/home.png", "kind": "screenshot", "label": "Home" },
            { "id": "doc", "path": "out/spec.md", "kind": "markdown", "label": "Spec" }
        ],
        "artifact": "shot"
    })
    .to_string();
    let p = decode(&frame).expect("view frame should decode");
    assert_eq!(p.session_type(), "view");
    match p {
        SessionPayload::View(v) => {
            assert_eq!(v.run_slug, "my-run");
            assert_eq!(v.artifacts.len(), 2);
            // The focused-artifact deep link resolves.
            let focused = v.artifact_by_id("shot").expect("focused artifact present");
            assert_eq!(focused.kind, ViewArtifactKind::Screenshot);
            // The mapping boundary produces reviewable entries for the screenshot.
            let entries = map::artifact_entries(&v.artifacts);
            assert_eq!(entries.len(), 2);
            assert!(entries[0].kind.is_reviewable());
            assert!(!entries[1].kind.is_reviewable());
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn decodes_visual_review_frame() {
    let frame = json!({
        "session_type": "visual_review",
        "session_id": "vr1",
        "status": "pending",
        "run_slug": "my-run",
        "artifact_id": "shot",
        "artifact_path": "out/home.png",
        "screenshot_url": "/shot/home.png",
        "prompt": "Review the home output."
    })
    .to_string();
    let p = decode(&frame).expect("visual_review frame should decode");
    assert_eq!(p.session_type(), "visual_review");
    match p {
        SessionPayload::VisualReview(vr) => {
            assert_eq!(vr.artifact_id.as_deref(), Some("shot"));
            assert_eq!(vr.screenshot_url.as_deref(), Some("/shot/home.png"));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn decodes_proof_frame_web_surface() {
    let frame = json!({
        "session_type": "proof",
        "session_id": "p1",
        "status": "pending",
        "run_slug": "my-run",
        "proof": {
            "surface": "web_ui",
            "web": {
                "vitals": { "lcp": 1200.0, "cls": 0.02 },
                "audits": [{ "name": "contrast", "value": "4.8:1", "pass": true }],
                "screenshot_url": "/shot.png"
            }
        }
    })
    .to_string();
    let p = decode(&frame).expect("proof frame should decode");
    assert_eq!(p.session_type(), "proof");
    match p {
        SessionPayload::Proof(pr) => {
            assert_eq!(pr.proof.surface, Surface::WebUi);
            let view = map::proof_view(&pr.proof);
            assert_eq!(view.kind, ProofMetricKind::Web);
            assert!(view.block_matches_surface);
            // lcp classifies good and formats in seconds.
            let lcp = view.vitals.iter().find(|v| v.key == "lcp").expect("lcp present");
            assert_eq!(lcp.verdict, VitalVerdict::Good);
            assert_eq!(lcp.display, "1.20 s");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn decodes_proof_frame_bench_surface() {
    let frame = json!({
        "session_type": "proof",
        "session_id": "p2",
        "status": "pending",
        "proof": {
            "surface": "library",
            "bench": { "p50": 0.5, "p99": 2.0, "throughput": 50000.0, "samples": 1000 }
        }
    })
    .to_string();
    match decode(&frame).expect("decode") {
        SessionPayload::Proof(pr) => {
            let view = map::proof_view(&pr.proof);
            assert_eq!(view.kind, ProofMetricKind::Bench);
            assert!(view.vitals.is_empty());
            assert!(view.bench.iter().any(|b| b.label == "throughput"));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn artifact_kind_maps_each_variant() {
    assert_eq!(map::artifact_kind(ViewArtifactKind::File), ArtifactKind::File);
    assert_eq!(map::artifact_kind(ViewArtifactKind::Json), ArtifactKind::Json);
    assert_eq!(
        map::artifact_kind(ViewArtifactKind::Screenshot),
        ArtifactKind::Screenshot
    );
}

// ---------------------------------------------------------------------------
// submit_output_review: live loopback HTTP round-trip
// ---------------------------------------------------------------------------

async fn one_shot_server(response: &'static [u8]) -> (ConnConfig, tokio::task::JoinHandle<Vec<u8>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        loop {
            let n = sock.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(hdr_end) = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4) {
                let head = String::from_utf8_lossy(&buf[..hdr_end]);
                let content_len = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("Content-Length:")
                            .or_else(|| l.strip_prefix("content-length:"))
                    })
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= hdr_end + content_len {
                    break;
                }
            }
        }
        sock.write_all(response).await.unwrap();
        sock.flush().await.unwrap();
        buf
    });
    let cfg = ConnConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        session_id: "sess".to_string(),
    };
    (cfg, handle)
}

fn review_request() -> OutputReviewRequest {
    OutputReviewRequest {
        annotations: VisualReviewAnnotations {
            pins: vec![VisualReviewPin { x: 0.5, y: 0.25, note: "button too small".into() }],
            comments: vec!["fix the header".into()],
        },
        title: Some("home review".into()),
    }
}

#[tokio::test]
async fn submit_output_review_posts_to_annotate_route() {
    let (cfg, handle) =
        one_shot_server(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n").await;
    let res = submit_output_review(&cfg, &review_request()).await;
    assert!(res.is_ok(), "expected Ok, got {res:?}");
    let req = handle.await.unwrap();
    let req = String::from_utf8_lossy(&req);
    assert!(
        req.starts_with("POST /visual-review/sess/annotate HTTP/1.1\r\n"),
        "{req}"
    );
    assert!(req.contains(r#""note":"button too small""#), "{req}");
    assert!(req.contains(r#""fix the header""#), "{req}");
}

#[tokio::test]
async fn submit_output_review_status_err_on_500() {
    let (cfg, handle) =
        one_shot_server(b"HTTP/1.1 500 Internal Server Error\r\n\r\n").await;
    let res = submit_output_review(&cfg, &review_request()).await;
    assert!(res.is_err());
    handle.await.unwrap();
}
