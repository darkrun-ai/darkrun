---
name: SimFixture schema in darkrun-core — wasm-safe serde types per amended Contract 3
unit_type: feature
status: completed
depends_on: []
worker: reconciler
model: sonnet
station: build
inputs:
- spec.md
outputs:
- crates/darkrun-core/src/sim_fixture.rs
- Cargo.lock
- crates/darkrun-core/src/lib.rs
branch: darkrun/darkrun-sim/units/build/fixture-schema
started_at: 2026-07-19T21:22:51.279480+00:00
completed_at: 2026-07-19T21:27:01.982255+00:00
iterations:
- worker: test_author
  started_at: 2026-07-19T21:22:51.279480+00:00
  completed_at: 2026-07-19T21:22:51.279480+00:00
  result: advance
  note: 'Authored crates/darkrun-core/src/sim_fixture.rs complete: all five types + the const per amended Contract 3, snake_case tagging on FixtureOutcome with the wire-form doc comment, module registered alphabetically in lib.rs, and a 7-test inline suite covering the five required cases plus truncated-JSON Err and nested-Value byte-stability (commit 91d422d on the unit branch; also syncs the worktree''s stale Cargo.lock version strings — no new dependency edges). All six completion criteria verified passing: 7/7 tests, whole crate green (9 binaries), clippy -D warnings clean, grep criteria 6 and 6/1, Cargo.toml untouched. All three quality gates ran for real and recorded pass. Builder: no implementation gap remains — a data schema''s types are its interface and they shipped with the tests; re-verify and advance unless you find something the suite missed.'
- worker: builder
  started_at: 2026-07-19T21:23:05.093371+00:00
  completed_at: 2026-07-19T21:23:05.093371+00:00
  result: advance
  note: 'Builder beat performed directly: the test_author beat left zero implementation gap (a data schema''s types are its interface and shipped with the suite), so this beat re-verified rather than rebuilt. Re-ran in the unit worktree: cargo test -p darkrun-core sim_fixture (7 passed, 0 failed), cargo clippy -p darkrun-core --all-targets -- -D warnings (exit 0), and the no-new-deps gate (exit 0). Nothing to add or repair; no code changed this beat. Self_reviewer: attack the schema against the amended Contract 3 verbatim (field names, types, order), the wire-form guarantees (snake_case on FixtureOutcome — is the doc comment accurate about "sealed"/"escalated" shapes), the test suite''s honesty (do 5(a)-(e) each exist as real assertions, does the nested-Value test actually assert byte-for-byte re-serialization), and the lib.rs registration placement.'
- worker: self_reviewer
  started_at: 2026-07-19T21:26:24.964132+00:00
  completed_at: 2026-07-19T21:26:24.964132+00:00
  result: advance
  note: 'Verify beat clean — no defects. Independently confirmed: field-by-field contract fidelity for all five types + const (exact order, no drift); wire form verified empirically via a scratch probe crate outside the worktree (Sealed = "sealed", Escalated = {"escalated":{"reason":...}}) with the doc comment''s domain.rs precedent confirmed accurate; all 7 tests read and proven non-vacuous (whole-struct round-trip equality, +41 version drift, truly-empty degenerate case, is_err() on truncated JSON, byte-for-byte string equality on the nested-Value re-serialization); blast radius exactly sim_fixture.rs + one alphabetical lib.rs line + a pure version-string Cargo.lock sync with zero new package entries; all three gates re-run live (7 passed / clippy clean / no-new-deps exit 0) plus the whole crate green across 9 binaries; module imports only serde/serde_json (wasm-clean, zero use crate::). Reconciler: nothing to reconcile — final-verify and lock.'
- worker: reconciler
  started_at: 2026-07-19T21:26:51.433623+00:00
  completed_at: 2026-07-19T21:26:51.433623+00:00
  result: advance
  note: 'Reconcile beat: make and verify agree with zero findings to reconcile, so this terminal beat ran the final sweep — all 9 darkrun-core test binaries green in the unit worktree, tree clean at commit 91d422d. The unit satisfies every completion criterion and all three quality gates are recorded pass. Locking.'
reviews:
  correctness:
    at: 2026-07-12T06:11:51.306714+00:00
  maintainability:
    at: 2026-07-12T06:11:54.404623+00:00
quality_gates:
- name: schema-tests
  command: cargo test -p darkrun-core sim_fixture
- name: core-clippy
  command: cargo clippy -p darkrun-core --all-targets -- -D warnings
- name: no-new-deps
  command: sh -c '! git diff HEAD~1 -- crates/darkrun-core/Cargo.toml | grep -q "^+" || ! git diff HEAD~1 --name-only | grep -q crates/darkrun-core/Cargo.toml'
gate_results:
- name: schema-tests
  status: pass
  at: 2026-07-19T21:22:17.666854+00:00
  attempts: 1
  detail: 'cargo test -p darkrun-core sim_fixture: 7 passed, 0 failed in the unit worktree (commit 91d422d)'
- name: core-clippy
  status: pass
  at: 2026-07-19T21:22:28.078207+00:00
  attempts: 1
  detail: cargo clippy -p darkrun-core --all-targets -- -D warnings exited 0 in the unit worktree (commit 91d422d)
- name: no-new-deps
  status: pass
  at: 2026-07-19T21:22:36.798255+00:00
  attempts: 1
  detail: The unit commit does not touch crates/darkrun-core/Cargo.toml; gate command exited 0 in the unit worktree (commit 91d422d)
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
