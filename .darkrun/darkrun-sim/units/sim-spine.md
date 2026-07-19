---
name: World + provider + transcript spine in crates/darkrun-sim, harness rebuild, committed fixture
unit_type: feature
status: completed
depends_on:
- fixture-schema
worker: reconciler
model: opus
station: build
inputs:
- frame.md
- crates/darkrun-core/src/sim_fixture.rs
outputs:
- crates/darkrun-sim/src/world.rs
- crates/darkrun-sim/src/provider.rs
- crates/darkrun-sim/src/transcript.rs
- crates/darkrun-sim/fixtures/dark-core.json
- Cargo.lock
- crates/darkrun-core/src/lib.rs
- crates/darkrun-core/src/sim_fixture.rs
- crates/darkrun-sim/src/harness.rs
- crates/darkrun-sim/src/lib.rs
- crates/darkrun-sim/src/scenarios.rs
branch: darkrun/darkrun-sim/units/build/sim-spine
started_at: 2026-07-19T22:03:16.250880+00:00
completed_at: 2026-07-19T22:22:35.949786+00:00
iterations:
- worker: test_author
  started_at: 2026-07-19T22:03:16.250880+00:00
  completed_at: 2026-07-19T22:03:16.250880+00:00
  result: advance
  note: 'Authored the complete spine, fully green in one beat (commit 7430197, opus): provider.rs (Provider trait + ProviderMove + prompt-blind ScriptedProvider, zero .action tokens), world.rs (NoopHosting, tick-first World driver, grade_tick as the sole .action reader at column 0, per-tick StateStore target resolution, Mode::Dark enum direct, double-run_start guard, dark_core_script sized to the 6-station factory), transcript.rs (three-stream projector with Contract-3 normalization incl. tempdir-path collapsing, units captured once, fixture submodule), harness.rs rebuilt to the narrowed primitives, capture_to_seal relocated verbatim to scenarios.rs, and the REAL generated fixture (37 ticks, 6 units, outcome sealed). 29+3 tests pass incl. all five named tests and all edge tests; clippy clean; every seam-grep leg passes; all four quality gates recorded pass. Key mechanics for downstream: the green drive uses a uniform per-station 6-move block (AdvanceStation x2, CompleteWave, AdvanceStation x3) — checkpoint-tick AdvanceStation harmlessly seeds the next station; dark mode has no RunReview hold and no proof-escalate (surface None). Builder: nothing red remains — re-verify and advance unless something the suite missed surfaces.'
- worker: builder
  started_at: 2026-07-19T22:03:30.475493+00:00
  completed_at: 2026-07-19T22:03:30.475493+00:00
  result: advance
  note: 'Builder beat performed directly: the test_author beat delivered the complete spine green, so this beat re-verified rather than rebuilt — all four declared gates re-ran verbatim in the unit worktree and passed (29+3 tests / clippy clean / every seam-grep leg / fixture parse with 37 ticks, 6 units, sealed). No code changed this beat. Self_reviewer: attack the spine adversarially — the prompt-blindness claim (does anything in the decision path leak prompt or action-derived data), the transcript''s normalization completeness (regenerate-twice byte-equality could pass while a volatile field survives in BOTH runs by coincidence of ordering — check the normalization rules against the actual captured prompt text), the verbatim-ness of the capture_to_seal relocation (diff it against the pre-rebuild harness.rs from git history), the honesty of the edge tests (each spec edge case asserted, not just present), and whether the dark_core_script''s uniform 6-move block is engine-behavior-coupled in a way that a future factory change silently breaks (is that documented).'
- worker: self_reviewer
  started_at: 2026-07-19T22:11:34.069693+00:00
  completed_at: 2026-07-19T22:11:34.069693+00:00
  result: reject
  note: 'REJECT — architecture sound (prompt-blindness, relocation verbatim-ness, normalization, six of eight edge tests, all four gates verified clean) but three coverage gaps against the spec''s own Check clauses: (1) AC-10''s empty-prompts-dir edge has ZERO coverage — project() is only ever called on Sealed runs; no test drives an escalate-inducing provider through project() asserting ticks match the actual action-log count with no padding and no panic; (2) AC-10''s stale-content-fixture edge is neither tested nor documented-as-deferred — needs a darkrun-sim-scope self-containment test (assert the fixture JSON requires no darkrun_content resolution) or an explicit deferral with a named trigger like STALE_AGE_SECS got; (3) AC-6''s required SECOND test is missing — seed the same action tag/station pair on two different ticks (fix-feedback rework loop) and assert both DISTINCT prompt texts survive at their tick indices (the existing clobber-sentinel test is a proxy, not the demanded scenario). Also fix lib.rs line 45''s dead doc link (harness::Harness::capture_to_seal -> scenarios::capture_to_seal). Everything else is solid — do not touch it.'
- worker: builder
  started_at: 2026-07-19T22:19:18.818664+00:00
  completed_at: 2026-07-19T22:19:18.818664+00:00
  result: advance
  note: 'Repair beat closed all three review findings plus the doc link (commit ed2adc1): (1) escalate_run_projects_without_padding_unreached_stations drives the AlwaysAdvance inducer through project() and asserts no panic, ticks.len() == actual action-log line count with no padding, Escalated outcome, and the unreached specify station has no prompts dir and no projected tick; (2) committed_fixture_is_self_contained parses the committed fixture standalone and asserts every display label is a non-empty literal and every prompt is fully rendered text with no unresolved template delimiters; (3) distinct_prompts_for_a_recurring_tag_station_survive forces two spec@frame ticks with different content, asserts both distinct prompts survive at their own indices while the clobbered on-disk file matches only the last — proving in-memory capture; (4) lib.rs doc link fixed to scenarios::capture_to_seal. 32+3 tests pass (was 29+3), clippy clean, every seam-grep leg passes, fixture unchanged (additive tests; committed_fixture_matches_regeneration stayed green). One ride-along: a 13-line deletion of .darkrun/darkrun-sim/drift-witness.log (engine bookkeeping staged concurrently by the live run; the working-tree copy is intact and the engine rewrites it each tick). Self_reviewer: verify the three findings closed as demanded and nothing regressed.'
- worker: self_reviewer
  started_at: 2026-07-19T22:22:00.313023+00:00
  completed_at: 2026-07-19T22:22:00.313023+00:00
  result: advance
  note: 'Re-verify clean — ADVANCE. All three findings genuinely closed in ed2adc1, each read and re-run: the escalate-projection test is the first to exercise project() on a non-Sealed run and asserts no padding, Escalated outcome, and no tick/prompts-dir for the unreached station; the self-containment test makes real assertions (non-empty labels, no unrendered {{ }}/{% %} delimiters), not a vacuous parse; the recurring tag/station test honestly forces two spec@frame ticks with distinct content and proves tick 1''s prompt survives only via in-memory capture (on-disk file equals p1, not p0). Doc link fixed. All four gates re-run verbatim against the new commit: 32+3 tests, clippy clean, seam-greps exit 0, fixture byte-unchanged (diff empty). Commit touches exactly lib.rs + transcript.rs + the engine''s drift-witness.log ride-along (inspected: plain telemetry paths, not source). Reconciler: nothing to reconcile — final sweep and lock.'
- worker: reconciler
  started_at: 2026-07-19T22:22:27.688315+00:00
  completed_at: 2026-07-19T22:22:27.688315+00:00
  result: advance
  note: 'Reconcile beat: make, repair, and verify converged — the reject round''s three coverage gaps closed and independently re-verified, nothing left to reconcile. Final sweep in the unit worktree: all 3 test binaries green (32 lib + 3 followability + 0 doc), tree clean apart from the engine''s live drift-witness bookkeeping, tip ec090d0 (engine checkpoint atop ed2adc1). All four quality gates recorded pass. Locking.'
reviews:
  correctness:
    at: 2026-07-12T06:11:51.306714+00:00
  maintainability:
    at: 2026-07-12T06:11:54.404623+00:00
quality_gates:
- name: crate-tests
  command: cargo test -p darkrun-sim
- name: sim-clippy
  command: cargo clippy -p darkrun-sim --all-targets -- -D warnings
- name: seam-greps
  command: sh -c '! grep -n "[^_]run_tick(" crates/darkrun-sim/src/harness.rs crates/darkrun-sim/src/world.rs && ! grep -n "[.]action" crates/darkrun-sim/src/provider.rs && ! grep -n "fn capture_to_seal" crates/darkrun-sim/src/harness.rs && grep -c "fn capture_to_seal" crates/darkrun-sim/src/scenarios.rs | grep -qx 1 && test "$(grep -c "[.]action" crates/darkrun-sim/src/world.rs)" = "$(sed -n "/^fn grade_tick/,/^}/p" crates/darkrun-sim/src/world.rs | grep -c "[.]action")" && test "$(grep -c "[.]action" crates/darkrun-sim/src/world.rs)" -ge 1'
- name: fixture-committed
  command: sh -c 'test -s crates/darkrun-sim/fixtures/dark-core.json && python3 -c "import json;d=json.load(open(\"crates/darkrun-sim/fixtures/dark-core.json\"));assert d[\"schema_version\"]==1 and d[\"outcome\"]==\"sealed\" and len(d[\"ticks\"])>0 and len(d[\"units\"])>0"'
gate_results:
- name: crate-tests
  status: pass
  at: 2026-07-19T22:02:35.154888+00:00
  attempts: 1
  detail: 'cargo test -p darkrun-sim: 29 passed lib + 3 passed followability, 0 failed, 1 ignored (fixture regenerator) in the unit worktree (commit 7430197)'
- name: sim-clippy
  status: pass
  at: 2026-07-19T22:02:42.509328+00:00
  attempts: 1
  detail: cargo clippy -p darkrun-sim --all-targets -- -D warnings exited 0 in the unit worktree (commit 7430197)
- name: seam-greps
  status: pass
  at: 2026-07-19T22:02:52.975534+00:00
  attempts: 1
  detail: 'Gate command ran verbatim, exit 0: no bare run_tick( in harness/world; zero .action in provider.rs; world.rs .action count 1 == grade_tick body count 1 (>= 1); capture_to_seal 0 in harness.rs, exactly 1 in scenarios.rs (commit 7430197)'
- name: fixture-committed
  status: pass
  at: 2026-07-19T22:03:01.892343+00:00
  attempts: 1
  detail: 'fixtures/dark-core.json parses: schema_version 1, outcome "sealed", 37 ticks, 6 units; gate command exited 0 (commit 7430197)'
---

# Unit: sim-spine

## Goal

Build the frame-compliant protocol-fidelity spine inside `crates/darkrun-sim`: the `world`, `provider`, and `transcript` modules, the rebuilt `harness.rs`, the adjusted `scenarios.rs`, and the committed deterministic fixture `crates/darkrun-sim/fixtures/dark-core.json`. THE CONTRACT IS THE LOCKED SPEC — read `.darkrun/darkrun-sim/specify/spec.md` in full before writing anything (ACs 1-11 and 15 are yours; Contracts 1-5; all nine Edge cases), plus `.darkrun/darkrun-sim/frame/frame.md` (the seams). The existing crate is a prompt-wording LINTER whose harness violates the seams — you are partitioning, not extending: `agent.rs`, `tool_registry.rs`, and `tests/followability.rs` stay byte-untouched.

## Module-by-module (Contract 4's map, verified against the real crate)

1. **`src/provider.rs` (new).** The `Provider` trait + `ProviderMove` enum + the scripted implementation, exactly per Contract 1 (signatures verbatim from the spec). The scripted impl receives `prompt: Option<&str>` and NEVER conditions on it — the parameter exists to mirror the seam. Include the named test `scripted_provider_ignores_prompt_content` (drive the same script twice, once with real prompts, once with every prompt replaced by a fixed dummy string; assert the emitted `ProviderMove` sequences are identical). `ProviderMove::Stop` is legitimate ONLY after the world observed a terminal outcome (sealed or escalated) on the most recent tick — one rule, stated in the doc comment, matching the spec's exhausted-script edge case. Wording constraint (fb-10 resolution): provider.rs contains ZERO occurrences of the literal token `.action` anywhere, including doc comments — refer to "the structured action variant" in prose instead; the seam gate greps this file strictly.
2. **`src/world.rs` (new).** `NoopHosting` (vendored: `available() -> false`, `open_draft() -> None`, `merge_state() -> MergeState::Unknown` — import via `darkrun_mcp::hosting::{Hosting, MergeState, OpenRequest}`, NOT crate root). The world: own `tempfile::tempdir()` + `StateStore::new` (textually distinct from harness.rs's — AC-1 greps for it), `run_start(..., Mode::Dark, ...)` passing the ENUM VARIANT directly (never a string through `Mode::from_label` — AC-1), then the tick loop via `darkrun_mcp::position::run_tick_with_hosting` ONLY. Decisions come from `Provider::next_move`; target resolution (WHICH station/unit a move aims at) comes from per-tick `StateStore` reads (`read_state`/`read_units`) — the spec's positive boundary rule, with the named test `state_reads_resolve_targets_only` (corrupt non-identifier state fields; assert the move sequence is unchanged). NO function in the call graph rooted at the tick loop takes `&RunAction` or a value derived from matching its variants as DECISION input (AC-3's structural rule). **Grading confinement (fb-10/fb-11/fb-12 resolution):** AC-3 permits the world one grading step — every `.action` read in world.rs lives inside a single function declared at column 0 as exactly `fn grade_tick` (private free fn; its closing brace is a `^}` line; the seam gate extracts its body with `sed -n "/^fn grade_tick/,/^}/p"`, so the declaration must not be indented, `pub`-prefixed, or nested). `grade_tick` observes the returned tick for terminal outcomes (sealed/escalated per the deadlock guard) and feeds ONLY the loop's stop condition and the `FixtureOutcome` — never which `ProviderMove` comes next. Carry a doc comment ON `grade_tick` stating this confinement rule (the gate stops enforcing after the unit completes; the comment is the durable guard). Move EXECUTION uses the real darkrun-mcp entry points the current harness already uses (`elaborate_seal`, `checkpoint_decide`, `run_review_stamp` via `darkrun_mcp::position::*`, direct `store.write_unit` in the decompose helper — the sanctioned pattern). Every engine-rejected move (`McpError` return) maps to the spec's edge-case outcome — no `.expect()` on legitimate rejection paths. The named test `escalate_scenario_is_detected_red`: a provider that always returns `ProviderMove::AdvanceStation` at the current station and never `CompleteWave` — the engine's deadlock guard fires within its 4-tick threshold and the world reports `FixtureOutcome::Escalated { .. }` (this inducer never returns Stop, per the tightened AC-9).
3. **`src/transcript.rs` (new).** The projector: captures each `TickResult`'s prompt the moment `run_tick_with_hosting` returns it (prompts/*.md files are overwrite-on-reuse — never re-read from disk), merges the three streams (in-memory prompt capture + `action-log.jsonl` + `events.jsonl` read via `StateStore::read_journal`) under the spec's explicit ordering rule, normalizes per Contract 3's three rules (timestamps → `"<normalized>"`, verifier_nonce hash → `<nonce>`, deadlock.json never embedded), captures `units: Vec<FixtureUnit>` ONCE after the terminal tick via `read_units` (fb-08 amendment: slug/station/depends_on/status only), and emits a `darkrun_core::sim_fixture::SimFixture`. `FixtureOutcome` serializes snake_case (`"sealed"` / `{"escalated":{...}}`) — the fixture-schema unit pins `#[serde(rename_all = "snake_case")]` per the domain.rs idiom; the committed fixture's `outcome` field is therefore the literal `"sealed"`. Contains a `fixture` submodule so `cargo test -p darkrun-sim fixture::` matches (AC-7's filter). Include the named tests `regenerate_twice_is_byte_equal` (run the full dark scenario twice in fresh tempdirs; assert the two serialized fixtures are byte-identical) and `committed_fixture_matches_regeneration` (regenerate in-memory; assert equality with `include_str!("../fixtures/dark-core.json")` parsed — this is the test the CI job runs).
4. **`src/harness.rs` (REBUILT — fresh contents, not a patch).** Keeps exactly: `Harness::start` (same signature), `tick` (now `run_tick_with_hosting` against `crate::world::NoopHosting` — the only internal change), `seal`, `decide`, `render`, the `pub store`/`pub slug` fields, free `pub fn action_tag`, and `decompose_one`/`complete_units`/`seed_spec` promoted to `pub(crate)`. `capture_to_seal` is DELETED from this file.
5. **`src/scenarios.rs` (adjusted, ceiling stated).** Gains `pub(crate) fn capture_to_seal(harness: &Harness) -> BTreeMap<String, String>` — the walk loop relocated VERBATIM (same match arms, same `guard < 2000`), call site at line 54 changes from method to free-function form. `core_scenarios()` returns the SAME data. Nothing else changes.
6. **`src/lib.rs`**: register `pub mod world; pub mod provider; pub mod transcript;` alongside the existing modules.
7. **`fixtures/dark-core.json` (committed).** Generated by the green dark-mode scripted scenario (software factory, `Mode::Dark`, scripted provider to sealed), serialized via `serde_json::to_string_pretty`, committed. `outcome` must be the literal `"sealed"`, `ticks` non-empty, `units` non-empty.

## Success / failure / edge paths the tests must cover (the spec's nine edge cases, each with its REQUIRED outcome)

Rejected move → the outcome the spec's Edge cases section names (read it; no expect() panics on legitimate rejections). Exhausted script (Stop before terminal) → harness failure panic per the one Stop rule. Dark-mode FeedbackQuestion → the spec's named handling. render_prompt None → corpus-wide assertion that it never fires. Double run_start → the world's own guard per the spec. Post-Sealed ticks → loop stops on first terminal observation (via `grade_tick`); idempotence asserted. Empty prompts dir for unreached phases → treated as absent, never padded. STALE_AGE_SECS → named exemption, NO test targets it (`grep -rn STALE_AGE_SECS crates/darkrun-sim/` stays empty — AC-10's exception check). Stale-content fixture → fixture fully self-contained.

## Completion criteria (verify each from the unit worktree root)

1. `cargo test -p darkrun-sim` exits 0 — including the three UNTOUCHED followability tests (`git diff --name-only` for your commits shows no `tests/followability.rs`, no `src/agent.rs`, no `src/tool_registry.rs`).
2. The five named tests exist and pass with a NONZERO passed count each (a name filter matching zero tests exits 0 — assert the summary line reports >= 1 passed): `scripted_provider_ignores_prompt_content`, `state_reads_resolve_targets_only`, `regenerate_twice_is_byte_equal`, `escalate_scenario_is_detected_red`, `committed_fixture_matches_regeneration`.
3. `cargo test -p darkrun-sim fixture::` runs at least one test (AC-7's filter shape, nonzero passed).
4. Seam greps all hold — run the declared `seam-greps` gate command VERBATIM and confirm exit 0, then also verify each leg individually: no bare `run_tick(` in harness.rs/world.rs; ZERO `.action` occurrences in provider.rs (doc comments included); `grep -c '[.]action' crates/darkrun-sim/src/world.rs` equals `sed -n '/^fn grade_tick/,/^}/p' crates/darkrun-sim/src/world.rs | grep -c '[.]action'` and both are >= 1 (the confinement leg must be exercised, not vacuous — the gate's final clause enforces the >= 1); `fn capture_to_seal` absent from harness.rs and present exactly once in scenarios.rs.
5. `cargo clippy -p darkrun-sim --all-targets -- -D warnings` exits 0.
6. `crates/darkrun-sim/fixtures/dark-core.json` exists, parses, `schema_version == 1`, `outcome == "sealed"` (the snake_case wire form the fixture-schema unit pins), non-empty `ticks` and `units`.
7. `grep -rn 'STALE_AGE_SECS' crates/darkrun-sim/` returns nothing.

## Files touched

`crates/darkrun-sim/src/{world.rs,provider.rs,transcript.rs}` (new), `crates/darkrun-sim/src/{harness.rs,scenarios.rs,lib.rs}` (rebuilt/adjusted), `crates/darkrun-sim/fixtures/dark-core.json` (new). NOTHING else — no engine-crate edits, no web/site, no CI, no Cargo.toml changes (all needed deps already declared).

## Out of scope

No engine-code changes of any kind (determinism comes from normalization at projection — locked). No changes to agent.rs/tool_registry.rs/tests/followability.rs. No real-model provider. No web/site or CI work (sibling units own those).
