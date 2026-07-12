---
name: SimFixture schema in darkrun-core — wasm-safe serde types per amended Contract 3
unit_type: feature
status: pending
depends_on: []
worker: ''
model: sonnet
station: build
inputs:
- spec.md
outputs:
- crates/darkrun-core/src/sim_fixture.rs
quality_gates:
- name: schema-tests
  command: cargo test -p darkrun-core sim_fixture
- name: core-clippy
  command: cargo clippy -p darkrun-core --all-targets -- -D warnings
- name: no-new-deps
  command: sh -c '! git diff HEAD~1 -- crates/darkrun-core/Cargo.toml | grep -q "^+" || ! git diff HEAD~1 --name-only | grep -q crates/darkrun-core/Cargo.toml'
---

# Unit: fixture-schema

## Goal

Create `crates/darkrun-core/src/sim_fixture.rs` — the wasm-safe serde schema the sim serializes to and the site deserializes from — exactly per the AMENDED Contract 3 of the locked spec (`.darkrun/darkrun-sim/specify/spec.md`, read it in full first; the fb-08 amendment added the `units` field and `FixtureUnit` struct). This is the only shared type surface between `crates/darkrun-sim` (writer) and `web/site` (reader); it lives in darkrun-core because that is the established wasm-clean home (its only native dep is cfg(unix)-gated).

## What to build

1. `pub const SIM_FIXTURE_SCHEMA_VERSION: u32 = 1;` — mirroring the `SCHEMA_VERSION` const convention in `crates/darkrun-core/src/state.rs` (no migrator chain; v1 only).
2. These types, exactly as Contract 3 declares them (field names, types, and doc comments carrying the normalization notes): `SimFixture { schema_version: u32, run_slug: String, factory: String, mode: String, outcome: FixtureOutcome, ticks: Vec<FixtureTick>, events: Vec<FixtureEvent>, units: Vec<FixtureUnit> }`; `FixtureOutcome { Sealed, Escalated { reason: String } }`; `FixtureTick { seq: u32, track: String, action_tag: String, station: Option<String>, prompt: Option<String> }`; `FixtureEvent { seq: u32, event: String, fields: serde_json::Value }`; `FixtureUnit { slug: String, station: String, depends_on: Vec<String>, status: String }`.
3. Derives: `Debug, Clone, PartialEq, Serialize, Deserialize` on every type (the domain.rs convention minus JsonSchema — Contract 3 requires BOTH Serialize and Deserialize, unlike darkrun-mcp's Serialize-only engine types). **Serde tagging (fb-09/fb-10 resolution):** `FixtureOutcome` carries `#[serde(rename_all = "snake_case")]` — the spec's Rust block is silent on tagging, and this crate's established idiom for every tagged enum in `crates/darkrun-core/src/domain.rs` (`Status`, `StationPhase`, `Surface`) is `rename_all = "snake_case"`, so `Sealed` serializes as `"sealed"` and `Escalated` as `{"escalated":{"reason":...}}`. Add a doc comment on the enum stating exactly this: the wire form is snake_case per the crate convention, and the sibling sim-spine unit's fixture gate asserts the `"sealed"` literal.
4. Register `pub mod sim_fixture;` in `crates/darkrun-core/src/lib.rs`, slotted alphabetically between `locks` and `state` in the module list. No crate-root re-export (consumers use the full path `darkrun_core::sim_fixture::SimFixture`).
5. Inline `#[cfg(test)] mod tests` (the deadlock.rs/state.rs convention — NOT a new tests/ file) covering: (a) a fully-populated SimFixture round-trips through `serde_json::to_string` then `from_str` and compares equal (`PartialEq`); (b) `FixtureOutcome::Escalated { reason }` and `Sealed` both round-trip distinctly AND `serde_json::to_value(FixtureOutcome::Sealed)` equals `json!("sealed")` — the wire-form assertion the sibling gate depends on; (c) a fixture serialized with a MISSING optional prompt (`None`) round-trips; (d) deserializing JSON whose `schema_version` differs from the const still parses (the version field is data, not a gate — the reader decides what to do with it); (e) an empty-units, empty-events fixture round-trips (the degenerate case).

## Success / failure / edge paths the criteria cover

Success: round-trip equality on a representative fixture, plus the `"sealed"` wire-form assertion. Failure: `from_str` on malformed JSON returns Err (assert one case: truncated JSON errors, no panic). Edge: `serde_json::Value` fields with nested objects survive the round trip byte-for-byte after re-serialization with the same serializer settings.

## Completion criteria (verify each from the unit worktree root)

1. Module compiles and its tests pass → `cargo test -p darkrun-core sim_fixture` exits 0 with at least 5 tests run (the summary line reports a nonzero passed count).
2. The whole crate stays green → `cargo test -p darkrun-core` exits 0.
3. Lint-clean → `cargo clippy -p darkrun-core --all-targets -- -D warnings` exits 0.
4. All five type names + the const present → `grep -c 'pub struct SimFixture\|pub enum FixtureOutcome\|pub struct FixtureTick\|pub struct FixtureEvent\|pub struct FixtureUnit\|pub const SIM_FIXTURE_SCHEMA_VERSION' crates/darkrun-core/src/sim_fixture.rs` reports 6.
5. Both derives on every type and the snake_case attribute present → `grep -c 'Deserialize' crates/darkrun-core/src/sim_fixture.rs` reports >= 5, and `grep -c 'rename_all = "snake_case"' crates/darkrun-core/src/sim_fixture.rs` reports >= 1.
6. No new dependencies → `git diff HEAD~1 --name-only` (your own commit) does not include `crates/darkrun-core/Cargo.toml`, and `grep -c 'use crate::' crates/darkrun-core/src/sim_fixture.rs` shows only std/serde/serde_json imports are external.

## Files touched

`crates/darkrun-core/src/sim_fixture.rs` (new), `crates/darkrun-core/src/lib.rs` (one `pub mod` line). Nothing else.

## Out of scope

No projector logic (that is the sim-spine unit's transcript module), no changes to darkrun-mcp or web/site, no Cargo.toml edits, no schema fields beyond Contract 3 as amended, no JsonSchema derive.
