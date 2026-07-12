---
name: /replay route in web/site — static fixture player from darkrun-ui components
unit_type: feature
status: pending
depends_on:
- fixture-schema
- sim-spine
worker: ''
model: sonnet
station: build
inputs:
- crates/darkrun-core/src/sim_fixture.rs
- crates/darkrun-sim/fixtures/dark-core.json
outputs:
- web/site/src/pages/replay.rs
reviews:
  correctness:
    at: 2026-07-12T06:11:51.306714+00:00
  maintainability:
    at: 2026-07-12T06:11:54.404623+00:00
quality_gates:
- name: site-tests
  command: cargo test -p darkrun-site
- name: site-wasm-clippy
  command: cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings
- name: route-greps
  command: sh -c 'grep -q "/replay" web/site/src/route.rs && grep -qE "StationStrip|StationPipeline|UnitGraph" web/site/src/pages/replay.rs && grep -qiE "no.live.feed|no live feed" web/site/src/pages/replay.rs && ! grep -nE "gloo|remote::|\.fetch\(" web/site/src/pages/replay.rs'
---

# Unit: replay-route

## Goal

Add the `/replay` route to `web/site` — a static, no-live-feed player that renders the committed fixture `crates/darkrun-sim/fixtures/dark-core.json` using darkrun-ui components. THE CONTRACT IS THE LOCKED SPEC: read `.darkrun/darkrun-sim/specify/spec.md` in full first — ACs 12-14 and Contract 6 are yours, including the fb-08 amendment (UnitGraph's nodes and edges come from the fixture's `units` field: each `FixtureUnit.slug` is a node, each `depends_on` entry an edge). Model the page on `/preview` (`web/site/src/pages/preview.rs`: hardcoded payload, zero fetch, explicit banner), NEVER on `/browse` (its `remote::fetch_*` live pattern is the banned shape).

## What to build

1. **`web/site/src/pages/replay.rs` (new).** A `#[component] pub fn Replay() -> Element` that: parses the fixture ONCE via `include_str!("../../../../crates/darkrun-sim/fixtures/dark-core.json")` + `serde_json::from_str::<darkrun_core::sim_fixture::SimFixture>` (path verified: four `..` from `web/site/src/pages/`); renders the no-live-feed banner whose wording follows `preview.rs` lines 88-93's `SectionHead` `lead` precedent ("…no live feed is attached." — the `ScaffoldNote` dashed-border container from `web/site/src/pages/review.rs` line 199 is an optional separate styling choice); composes all three prelude components against fixture-derived data: `StationStrip { stations: Vec<StationItem> }` from the distinct stations across `ticks` (status Done for stations before the last, Current for the last — derive from tick order), `StationPipeline { dots }` via `strip_for` from the final tick's phase-bearing action_tag mapped into `darkrun_ui::kinds::Phase` (the translation lives HERE; darkrun-ui is darkrun-core-free by design), and `UnitGraph { units, edges }` from the fixture's `units` field (`UnitGraphNode::new(slug, slug)` per unit; a `GraphEdge` per `depends_on` entry). Render the tick list itself (seq, action_tag, station, and the normalized prompt in a collapsed/pre block) so the transcript is inspectable. Handle the malformed-fixture edge per the spec's Edge cases section (a parse failure renders an error state, no wasm panic — `from_str` result matched, never unwrapped).
2. **`web/site/src/route.rs`**: add `#[route("/replay")] Replay {}` adjacent to the `/preview` variant inside the `#[layout(Shell)]` block; add `pub use pages::replay::Replay;` beside the existing page re-exports; add `"/replay".to_string()` to the STATIC vec in `Route::all_paths()` (next to `"/preview"` at line 124); extend the expected-paths array inside the `all_paths_covers_the_static_sections` test to include `/replay`.
3. **`web/site/src/pages/mod.rs`**: add `pub mod replay;` to the flat module list.

## Success / failure / edge paths

Success: the page compiles for wasm32, renders all three components plus the banner from the embedded fixture. Failure: malformed embedded JSON renders the error state (add a unit test parsing a truncated copy and asserting the error path value, not a panic). Edge: a fixture whose `units` is empty renders an empty-graph state without panicking (test it); a station name in the fixture that the site's embedded factory content no longer knows is rendered as plain text, never resolved against the content corpus (the spec's stale-content edge).

## Completion criteria (verify each from the unit worktree root)

1. `cargo test -p darkrun-site` exits 0 (includes the four route tests — `all_paths_are_unique_and_rooted` catches a malformed insertion — plus your new tests).
2. `cargo build -p darkrun-site --target wasm32-unknown-unknown` exits 0 (AC-15's check).
3. `cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings` exits 0.
4. Route registered in BOTH places → `grep -c '"/replay"' web/site/src/route.rs` reports >= 2 (the `#[route]` attribute and the `all_paths()` literal).
5. Components + banner + no-fetch → `grep -E 'StationStrip|StationPipeline|UnitGraph' web/site/src/pages/replay.rs` shows all three inside rsx; `grep -iE 'no.live.feed|no live feed' web/site/src/pages/replay.rs` matches; `grep -nE 'gloo|remote::|\.fetch\(' web/site/src/pages/replay.rs` returns nothing (AC-13's exact checks).
6. No new dependency edge → `cargo tree -p darkrun-site -e normal | grep -c darkrun-mcp` reports 0 (AC-14), and `git diff --name-only` for your commits does not include `web/site/Cargo.toml`.

## Files touched

`web/site/src/pages/replay.rs` (new), `web/site/src/route.rs`, `web/site/src/pages/mod.rs`. Nothing else.

## Out of scope

No CI changes (sibling unit), no fixture regeneration (the committed fixture is the sim-spine unit's output — consume it read-only), no darkrun-ui component changes, no web/app work, no fetch/live-feed capability of any kind.
