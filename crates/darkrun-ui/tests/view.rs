//! Integration tests for the artifact-browser + proof-panel public surface — the
//! prop-data builders ([`ArtifactEntry`], [`ProofView`], [`VitalMetric`]) and the
//! pure view logic ([`ArtifactKind`], vital classification + formatting) the
//! [`ViewArtifacts`] and [`ProofPanel`] components drive.
//!
//! These exercise the crate through its `prelude` without instantiating a
//! renderer — the same renderer-free contract as `flow.rs`, `logic.rs`, and
//! `session_views.rs`.

use darkrun_ui::prelude::*;

// ===========================================================================
// ArtifactEntry builder
// ===========================================================================

#[test]
fn artifact_entry_minimal_has_no_extras() {
    let a = ArtifactEntry::new("a1", "out/home.png", ArtifactKind::Screenshot, "Home");
    assert_eq!(a.id, "a1");
    assert_eq!(a.path, "out/home.png");
    assert_eq!(a.kind, ArtifactKind::Screenshot);
    assert_eq!(a.label, "Home");
    assert!(a.thumbnail_url.is_none());
    assert!(a.url.is_none());
    assert!(a.body.is_none());
}

#[test]
fn artifact_entry_builders_attach_fields() {
    let a = ArtifactEntry::new("a", "p", ArtifactKind::Markdown, "Doc")
        .with_thumbnail("t.png")
        .with_url("/fetch/p")
        .with_body("# heading");
    assert_eq!(a.thumbnail_url.as_deref(), Some("t.png"));
    assert_eq!(a.url.as_deref(), Some("/fetch/p"));
    assert_eq!(a.body.as_deref(), Some("# heading"));
}

#[test]
fn artifact_entry_is_clone_eq() {
    let a = ArtifactEntry::new("a", "p", ArtifactKind::Json, "J").with_body("{}");
    assert_eq!(a.clone(), a);
    assert_ne!(
        ArtifactEntry::new("a", "p", ArtifactKind::Json, "J"),
        ArtifactEntry::new("b", "p", ArtifactKind::Json, "J"),
    );
}

// ===========================================================================
// ArtifactKind classification
// ===========================================================================

#[test]
fn artifact_kind_pictorial_and_reviewable_split() {
    // Pictorial = image | screenshot; reviewable = screenshot only.
    assert!(ArtifactKind::Image.is_pictorial());
    assert!(ArtifactKind::Screenshot.is_pictorial());
    assert!(!ArtifactKind::File.is_pictorial());

    assert!(ArtifactKind::Screenshot.is_reviewable());
    assert!(!ArtifactKind::Image.is_reviewable());
    assert!(!ArtifactKind::Markdown.is_reviewable());
}

#[test]
fn artifact_kind_labels_are_unique() {
    let labels = [
        ArtifactKind::File.label(),
        ArtifactKind::Image.label(),
        ArtifactKind::Screenshot.label(),
        ArtifactKind::Markdown.label(),
        ArtifactKind::Json.label(),
    ];
    let mut sorted = labels.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), labels.len());
}

// ===========================================================================
// Web-vital classification + formatting (the proof NUMBERS)
// ===========================================================================

#[test]
fn vitals_classify_against_core_web_vitals_thresholds() {
    assert_eq!(classify_vital("lcp", 1800.0), VitalVerdict::Good);
    assert_eq!(classify_vital("lcp", 3200.0), VitalVerdict::NeedsImprovement);
    assert_eq!(classify_vital("lcp", 5000.0), VitalVerdict::Poor);
    assert_eq!(classify_vital("cls", 0.05), VitalVerdict::Good);
    assert_eq!(classify_vital("unknown-metric", 1.0), VitalVerdict::Unknown);
}

#[test]
fn vital_values_format_with_units() {
    assert_eq!(format_vital("lcp", 1250.0), "1.25 s");
    assert_eq!(format_vital("ttfb", 420.0), "420 ms");
    assert_eq!(format_vital("cls", 0.012), "0.012");
    assert_eq!(vital_label("inp"), "INP");
}

#[test]
fn bench_values_format() {
    assert_eq!(format_latency_ms(0.8), "1 ms"); // sub-ms rounds
    assert_eq!(format_latency_ms(12.0), "12 ms");
    assert_eq!(format_latency_ms(1500.0), "1.50 s");
    assert_eq!(format_throughput(48_500.0), "48.5k ops/s");
    assert_eq!(format_throughput(3_100_000.0), "3.10M ops/s");
    assert_eq!(format_samples(125_000), "125,000");
}

// ===========================================================================
// ProofView / VitalMetric / AuditRow / BenchStat prop data
// ===========================================================================

#[test]
fn proof_view_web_carries_vitals_audits_and_screenshot() {
    let proof = ProofView {
        surface: "web_ui".to_string(),
        kind: ProofMetricKind::Web,
        vitals: vec![VitalMetric {
            key: "lcp".to_string(),
            value: 1200.0,
            display: format_vital("lcp", 1200.0),
            verdict: classify_vital("lcp", 1200.0),
        }],
        audits: vec![
            AuditRow { name: "contrast".to_string(), value: "4.8:1".to_string(), pass: true },
            AuditRow { name: "touch-target".to_string(), value: "40px".to_string(), pass: false },
        ],
        screenshot_url: Some("/shot/home.png".to_string()),
        bench: Vec::new(),
        block_matches_surface: true,
    };
    assert_eq!(proof.kind, ProofMetricKind::Web);
    assert_eq!(proof.vitals[0].verdict, VitalVerdict::Good);
    assert_eq!(proof.vitals[0].display, "1.20 s");
    assert!(!proof.audits[1].pass);
    assert!(proof.screenshot_url.is_some());
    // Clone + Eq hold for the projection.
    assert_eq!(proof.clone(), proof);
}

#[test]
fn proof_view_bench_carries_percentiles() {
    let proof = ProofView {
        surface: "library".to_string(),
        kind: ProofMetricKind::Bench,
        vitals: Vec::new(),
        audits: Vec::new(),
        screenshot_url: None,
        bench: vec![
            BenchStat { label: "p50".to_string(), display: format_latency_ms(0.5) },
            BenchStat { label: "p99".to_string(), display: format_latency_ms(2.0) },
            BenchStat { label: "throughput".to_string(), display: format_throughput(50_000.0) },
        ],
        block_matches_surface: true,
    };
    assert_eq!(proof.kind, ProofMetricKind::Bench);
    assert_eq!(proof.bench.len(), 3);
    assert_eq!(proof.bench[2].display, "50.0k ops/s");
    assert!(proof.vitals.is_empty());
}

#[test]
fn proof_view_terminal_is_snapshot_only() {
    let proof = ProofView {
        surface: "cli".to_string(),
        kind: ProofMetricKind::Terminal,
        vitals: Vec::new(),
        audits: Vec::new(),
        screenshot_url: Some("/snap/run.png".to_string()),
        bench: Vec::new(),
        block_matches_surface: true,
    };
    assert_eq!(proof.kind, ProofMetricKind::Terminal);
    assert!(proof.vitals.is_empty() && proof.bench.is_empty());
    assert!(proof.screenshot_url.is_some());
}
