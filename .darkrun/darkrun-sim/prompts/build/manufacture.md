
> **Run** `darkrun-sim` · **Station** `build` · **Phase** `manufacture`

> Eliminates: _implementation-defects_


# Manufacture — `build`

This is the build floor. You run the **Pass loop** — _Plan → Make → Challenge → Resolve_ — over the wave-ready Units. The current beat is **test_author**, on model **sonnet**.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`code`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `reuse`, `integration_point`


**Workers** (4): `test_author` → `builder` → `self_reviewer` → `reconciler`


**Reviewers** (2): `correctness`, `maintainability`


## This wave


Dispatch the **test_author** beat in parallel across these wave-ready Units:

- `sim-spine`




## Each Unit's spec — the contract the beat works against

The subagent you dispatch for a Unit gets **no context beyond what you hand it**. Pass the Unit's spec below into its dispatch verbatim — the completion criteria with their verify commands, the declared paths, and the scope boundary are the contract the beat is judged against.

### `sim-spine` — World + provider + transcript spine in crates/darkrun-sim, harness rebuild, committed fixture

- **inputs:** `frame.md`, `crates/darkrun-core/src/sim_fixture.rs`


- **outputs:** `crates/darkrun-sim/src/world.rs`, `crates/darkrun-sim/src/provider.rs`, `crates/darkrun-sim/src/transcript.rs`, `crates/darkrun-sim/fixtures/dark-core.json`


- **quality gates:** crate-tests — `cargo test -p darkrun-sim` · sim-clippy — `cargo clippy -p darkrun-sim --all-targets -- -D warnings` · seam-greps — `sh -c '! grep -n "[^_]run_tick(" crates/darkrun-sim/src/harness.rs crates/darkrun-sim/src/world.rs && ! grep -n "[.]action" crates/darkrun-sim/src/provider.rs && ! grep -n "fn capture_to_seal" crates/darkrun-sim/src/harness.rs && grep -c "fn capture_to_seal" crates/darkrun-sim/src/scenarios.rs | grep -qx 1 && test "$(grep -c "[.]action" crates/darkrun-sim/src/world.rs)" = "$(sed -n "/^fn grade_tick/,/^}/p" crates/darkrun-sim/src/world.rs | grep -c "[.]action")" && test "$(grep -c "[.]action" crates/darkrun-sim/src/world.rs)" -ge 1'` · fixture-committed — `sh -c 'test -s crates/darkrun-sim/fixtures/dark-core.json && python3 -c "import json;d=json.load(open(\"crates/darkrun-sim/fixtures/dark-core.json\"));assert d[\"schema_version\"]==1 and d[\"outcome\"]==\"sealed\" and len(d[\"ticks\"])>0 and len(d[\"units\"])>0"'`


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




## Each Unit has its own worktree — work in it

Every wave Unit is isolated on its own branch + worktree, forked off the station branch. Run that Unit's beat **inside its worktree** so its diff never tangles with another Unit's in-flight work; the manager lands each Unit back onto the station branch when it locks. Do **not** commit a Unit's work to the station branch yourself.

- `sim-spine` → `/Users/jwaldrip/dev/src/github.com/jwaldrip/darkrun/.claude/worktrees/wiggly-gathering-spark/.darkrun/worktrees/darkrun-sim/units/build/sim-spine` (branch `darkrun/darkrun-sim/units/build/sim-spine`)





## The Pass loop — make → challenge → resolve

The Pass loop is adversarial on purpose: a single confident pass is exactly where LLM output is most often confidently wrong, so a second pass red-teams the first before anything locks.

- **make** — the worker produces the Unit's output against its completion criteria. Build the real thing, not a sketch.
- **challenge** — a second pass attacks what make produced: edge cases, missing handling, lazy assumptions. Assume the first pass was optimistic.
- **resolve** — reconcile make and challenge into a Unit that satisfies its completion criteria with the challenges answered.


**Reject routing.** Workers carry a pass-loop role: `self_reviewer` = verify, . A `build` worker produces and repairs; a `verify` worker only judges; a `plan` worker only designs. When a beat **rejects**, bounce back to the **nearest preceding `build` worker** (pass it as `next_worker` to `darkrun_unit_iterate`) — skip `verify`/`plan` beats on the way back, since they can't fix. An `advance` rolls forward to the next worker in order.



**Quality-gate verifier nonce.** This dispatch carries a one-time verifier token: **`affa8c653e282c3d71173554591ff479c8e28a574a322901d86efb62a18c486d`**. When you record a quality gate with `darkrun_quality_gate_record`, pass it as `nonce`. The engine refuses a gate result without the matching token — so a gate is only ever recorded as part of a real verification dispatch, never self-certified. Run the gate's command for real, then record the actual outcome with this nonce.


Run **only the `test_author` beat** this tick. When the beat finishes, **record it** with `darkrun_unit_iterate` — pass the `worker`, the `result` (`advance` or `reject`), and a `note`: on advance, what you did and what the next worker needs to know; on reject, why you bounced it (a reject without a reason is refused). That note becomes the next beat's handoff above. Then call `darkrun_tick`; the manager advances the loop or releases the next wave. A Unit is locked only after Resolve and its completion criteria pass.

A Unit gets a **bounded pass budget** — the manager escalates a Unit that can't converge within it to the operator rather than grinding forever. Don't paper over a stuck Unit to dodge the escalation; a Unit that needs more passes than the budget allows is a signal the spec, the scope, or the approach is wrong, and that's the operator's call to make.



## Done when

The `test_author` beat is complete for every Unit in this wave and its output is recorded. Then call `darkrun_tick`.

---

# Provider contracts in effect

The project configures external-system providers whose behavior contracts apply to this phase. Follow them alongside the instructions above.

# Git Provider — Behavior Contract

darkrun is always git-backed when a `.git/` directory is present. This contract is **always active** in git environments — no settings activation needed.

## What you, the agent, must do

- Never run `git checkout`, `git merge`, `git branch -d`, or create branches manually during run operations. The engine owns branch topology, merge semantics, worktree creation, and station-branch enforcement.
- Commit substantive work (unit body edits, artifact writes, source changes) before calling `darkrun_tick` — the pre-tick clean-tree gate blocks the tick on loose agent work and hands the file list back. The engine commits its own `.darkrun/` state on every tick; it does NOT author your commits.
- **Never pair a VCS issue-closing keyword with a feedback id.** GitHub and GitLab parse `Closes`/`Fixes`/`Resolves`/`Implements` followed by an issue-shaped token as an external-issue closing reference — `Fixes fb-07` in a commit message or PR description renders a phantom closing link for a finding that is not a ticket. Use neutral phrasing — `addresses fb-07`, `per fb-07` — never a closing verb.
- Treat `git push` failures as non-fatal — the engine retries on the next tick. Don't block on a transient remote outage.
- If a station's gate is `external`, the engine watches for the PR merge signal. Don't flip frontmatter to fake the signal — the human's merge IS the decision.

## Branch architecture (read-only fact you operate against)

- **Run branch** `darkrun/<slug>/main` is the durable record. The engine commits state changes here and pushes on every tick (commit early, push often). The run's **delivery draft PR** opens against the project's default branch at run start and the engine flips it ready-for-review at seal.
- **Station branches** `darkrun/<slug>/<station>` accumulate station-scope work, synced downstream and landed by the engine.
- **Unit worktree branches** `darkrun/<slug>/units/<station>/<unit>` isolate each unit's diff — local-only, landed back onto the station branch when the unit locks.

## external_refs handling

The delivery PR's URL is stamped on `run.md` as `external_refs.pr_url` with its draft/ready status in `external_refs.other.pr_status`. You don't write these fields manually — the engine does — but you can read them to surface PR state to the operator. In DISCRETE mode the engine also opens a per-station draft PR at the station's external gate (recorded on `Station.pr_ref`); merging it is the approval.

## Proof asset uploads

Runtime-verification proof (screenshots, transcripts) is regenerated every run — attach it durably with `darkrun_proof_attach`, which records it on the run's proof ledger and posts it to the station's change request when one exists. Keep uploads idempotent — replace a re-run's proof rather than stacking duplicates.

## Non-git environments

When `.git/` is absent the engine falls back to filesystem persistence: no commits, no pushes, no worktrees, and `external` gates degrade to `ask` (there's no structural merge signal to enforce them). All run operations still work; this contract simply doesn't apply.