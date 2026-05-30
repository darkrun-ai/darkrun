//! Integration tests for the `/preview` fixture page: the representative
//! Question and Direction payloads it renders, and the route registration that
//! makes it reachable + listed for the static generator.

use darkrun_api::common::SessionStatus;
use darkrun_api::proof::Surface;
use darkrun_api::session::{ViewArtifactKind, ViewStatus};
use darkrun_ui::components::proof_panel::ProofMetricKind;
use darkrun_ui::view::{ArtifactKind, VitalVerdict};
use darkrun_site::pages::preview::{
    proof_to_view, sample_bench_proof, sample_direction, sample_question, sample_view,
    sample_web_proof,
};
use darkrun_site::route::Route;

// ---------------------------------------------------------------------------
// route registration
// ---------------------------------------------------------------------------

#[test]
fn preview_route_is_listed_in_all_paths() {
    assert!(
        Route::all_paths().iter().any(|p| p == "/preview"),
        "the /preview fixture must be in Route::all_paths so the generator pre-renders it"
    );
}

#[test]
fn preview_path_is_unique() {
    let count = Route::all_paths().iter().filter(|p| *p == "/preview").count();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// question fixture
// ---------------------------------------------------------------------------

#[test]
fn question_fixture_is_a_well_formed_pending_question() {
    let q = sample_question();
    assert_eq!(q.session_id, "preview-question");
    assert_eq!(q.status, SessionStatus::Pending);
    assert!(!q.prompt.is_empty());
    assert!(q.title.is_some());
    assert!(q.context.is_some());
    // It is unanswered so the view renders the interactive (not read-only) shell.
    assert!(q.answer.is_none());
}

#[test]
fn question_fixture_has_at_least_two_distinct_options() {
    let q = sample_question();
    assert!(q.options.len() >= 2);
    let mut ids: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "option ids must be unique");
}

#[test]
fn question_fixture_options_carry_labels() {
    for o in sample_question().options {
        assert!(!o.id.is_empty());
        assert!(!o.label.is_empty());
    }
}

#[test]
fn question_fixture_is_single_select_by_default() {
    assert!(!sample_question().multi_select);
}

// ---------------------------------------------------------------------------
// direction fixture
// ---------------------------------------------------------------------------

#[test]
fn direction_fixture_is_a_well_formed_pending_direction() {
    let d = sample_direction();
    assert_eq!(d.session_id, "preview-direction");
    assert_eq!(d.status, SessionStatus::Pending);
    assert!(!d.prompt.is_empty());
    assert!(d.title.is_some());
}

#[test]
fn direction_fixture_has_distinct_archetypes_with_descriptions() {
    let d = sample_direction();
    assert!(d.archetypes.len() >= 2);
    for a in &d.archetypes {
        assert!(!a.id.is_empty());
        assert!(!a.label.is_empty());
        assert!(!a.description.is_empty(), "archetype {} needs a description", a.id);
    }
    let mut ids: Vec<&str> = d.archetypes.iter().map(|a| a.id.as_str()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "archetype ids must be unique");
}

#[test]
fn direction_fixture_chooses_an_existing_archetype() {
    let d = sample_direction();
    let chosen = d.chosen_archetype.as_deref().expect("a chosen archetype");
    assert!(
        d.archetypes.iter().any(|a| a.id == chosen),
        "chosen_archetype {chosen} must be one of the archetypes"
    );
}

#[test]
fn direction_fixture_pins_are_normalized_and_noted() {
    let d = sample_direction();
    let ann = d.annotations.as_ref().expect("annotations present");
    assert!(!ann.pins.is_empty(), "fixture should show pins for the screenshot");
    for p in &ann.pins {
        assert!((0.0..=1.0).contains(&p.x), "pin x {} out of range", p.x);
        assert!((0.0..=1.0).contains(&p.y), "pin y {} out of range", p.y);
        assert!(!p.note.is_empty());
    }
}

// ---------------------------------------------------------------------------
// view fixture (the artifact browser)
// ---------------------------------------------------------------------------

#[test]
fn view_fixture_is_an_open_run_scoped_browser() {
    let v = sample_view();
    assert_eq!(v.session_id, "preview-view");
    assert_eq!(v.status, ViewStatus::Open);
    assert!(!v.run_slug.is_empty());
    assert!(v.artifacts.len() >= 3, "browser should show several artifacts");
}

#[test]
fn view_fixture_covers_every_artifact_kind_path() {
    let v = sample_view();
    // The browser exercises each specialized viewer: a screenshot (reviewable),
    // markdown, json, and an opaque file.
    let kinds: Vec<ViewArtifactKind> = v.artifacts.iter().map(|a| a.kind).collect();
    assert!(kinds.contains(&ViewArtifactKind::Screenshot));
    assert!(kinds.contains(&ViewArtifactKind::Markdown));
    assert!(kinds.contains(&ViewArtifactKind::Json));
    assert!(kinds.contains(&ViewArtifactKind::File));
}

#[test]
fn view_fixture_focuses_an_existing_artifact() {
    let v = sample_view();
    let focus = v.artifact.as_deref().expect("a focused artifact");
    assert!(
        v.artifacts.iter().any(|a| a.id == focus),
        "focused artifact {focus} must be in the list"
    );
}

#[test]
fn view_fixture_screenshot_is_the_only_reviewable_kind() {
    let v = sample_view();
    let reviewable: Vec<&str> = v
        .artifacts
        .iter()
        .filter(|a| matches!(a.kind, ViewArtifactKind::Screenshot))
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(reviewable.len(), 1, "exactly one screenshot to review");
    // And the UI kind agrees it's reviewable.
    assert!(ArtifactKind::Screenshot.is_reviewable());
    assert!(!ArtifactKind::Json.is_reviewable());
}

// ---------------------------------------------------------------------------
// proof fixtures (the objective NUMBERS)
// ---------------------------------------------------------------------------

#[test]
fn web_proof_fixture_routes_to_the_web_block() {
    let proof = sample_web_proof();
    assert_eq!(proof.surface, Surface::WebUi);
    assert!(proof.block_matches_surface());
    let view = proof_to_view(&proof);
    assert_eq!(view.kind, ProofMetricKind::Web);
    assert_eq!(view.surface, "web_ui");
    // Vitals are present, ordered, classified, and formatted.
    assert!(!view.vitals.is_empty());
    assert_eq!(view.vitals[0].key, "lcp");
    assert_eq!(view.vitals[0].verdict, VitalVerdict::Good);
    // Audits carry a failing entry so the panel shows a fail chip.
    assert!(view.audits.iter().any(|a| !a.pass), "fixture should fail an audit");
    assert!(view.bench.is_empty());
}

#[test]
fn bench_proof_fixture_routes_to_the_bench_block() {
    let proof = sample_bench_proof();
    assert_eq!(proof.surface, Surface::Library);
    assert!(proof.block_matches_surface());
    let view = proof_to_view(&proof);
    assert_eq!(view.kind, ProofMetricKind::Bench);
    let labels: Vec<&str> = view.bench.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, vec!["p50", "p95", "p99", "throughput", "samples"]);
    // Throughput + samples are abbreviated/grouped for display.
    assert_eq!(view.bench[3].display, "48.5k ops/s");
    assert_eq!(view.bench[4].display, "100,000");
    assert!(view.vitals.is_empty());
}
