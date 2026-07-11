# Spec: darkrun-sim — protocol-fidelity simulator

This spec turns the locked frame into a contract an independent party can check without asking the author what they meant: every acceptance criterion below names a literal command or grep-able observable and a build phase (1, 2, or 3, per the frame's dependency-sequenced order); every contract names concrete struct/trait/field names; every edge case names a REQUIRED behavior, not a description of the problem. Nothing here is gradable on taste — "the harness spine works" is checked by a passing test, not a read of the code.

## Acceptance criteria

### AC-1: World construction is a bare, dark-mode `StateStore`

The new world driver starts a Run from a `tempfile::TempDir` that has no `.git` directory and no pre-seeded engine state, in `Mode::Dark`, via `StateStore::new` (`crates/darkrun-core/src/state.rs`) and `run_start` (`crates/darkrun-mcp/src/position.rs`) — never `Mode::from_label` on a string literal, so a typo cannot silently downgrade the mode.

Check: `grep -n 'tempfile::tempdir\|TempDir::new' crates/darkrun-sim/src/*.rs` shows the world module allocating its own tempdir (distinct from `harness.rs`'s existing `Harness::start`, which the linter partition keeps); `grep -n 'Mode::Dark' crates/darkrun-sim/src/*.rs` shows the world driver passing the `Mode::Dark` enum variant directly to `run_start`, not a string.

Phase: 1.

### AC-2: The driving loop calls `run_tick_with_hosting`, never `run_tick`

`run_tick` (`crates/darkrun-mcp/src/position.rs`) internally resolves `crate::hosting::ApiHosting`, which can reach the network; `run_tick_with_hosting<H: Hosting>` takes an injected client instead. The world's tick loop calls only the latter, against the sim's own `NoopHosting`. This is the exact violation named in the current `crates/darkrun-sim/src/harness.rs` (plain `run_tick` at lines 21 and 69) that the rebuild must not repeat.

Check: `grep -n 'run_tick(' <world module>` (the new `world` module under `crates/darkrun-sim/src/`, once it exists) returns nothing; `grep -n 'run_tick_with_hosting' <world module>` returns at least one match.

Phase: 1.

### AC-3: The driving path never reads `TickResult.action` to decide the next move

Per the frame's Engine seams ("Read only `.prompt`"), the code path that decides what to do next reads only `TickResult.prompt`; `.action` is read solely in a separably-named grading/projection path, after the move already executed.

Check: `grep -n '\.action' <world module> <provider module>` (the new `world` and `provider` modules under `crates/darkrun-sim/src/`, once they exist) shows every match either absent from the decision path, or confined to a function whose name signals post-hoc grading (e.g. containing `grade`, `project`, or `transcript`) — never inside the function that calls `Provider::next_move` or interprets its return value into a `run_tick_with_hosting` follow-up call.

Phase: 1.

### AC-4: `NoopHosting` performs zero I/O and satisfies the `Hosting` trait's three non-defaulted methods

`crates/darkrun-mcp/src/hosting.rs` declares `available`, `open_draft`, and `merge_state` with no default body; `is_draft`, `comment`, `review_comments`, and `mark_ready` default to no-op already. `darkrun-mcp` exports no public stub (only a private `Stub` inside `hosting.rs`'s own `#[cfg(test)] mod tests`), so the sim vendors its own `NoopHosting` in the `world` module: `available` returns the literal `false`, `open_draft` returns the literal `None`, `merge_state` returns the literal `MergeState::Unknown`.

Check: `grep -n 'impl Hosting for NoopHosting' -A 10 <world module>` shows the three literal return values above and no `ureq::`, `std::net::`, or `std::process::Command::new` token anywhere in the impl block; `grep -c 'ureq::\|std::net::\|Command::new' <world module>` reports 0.

Phase: 1.

### AC-5: The scripted provider is structurally prompt-blind

The `Provider` trait's decision method takes `prompt: Option<&str>` — mirroring `TickResult.prompt`'s type exactly, and structurally excluding `TickResult.action` from the interface (the method has no parameter through which the caller could pass it). The scripted implementation receives this parameter and never inspects it: no `if`, `match`, `.contains(`, or other conditional keyed on its content anywhere in the function body.

Check: `grep -n 'fn next_move' -A 5 <provider module>` (the new `provider` module under `crates/darkrun-sim/src/`) shows the parameter named `_prompt` (compiler-enforced unused) or, if named `prompt`, `grep -c 'prompt' <provider module>` (excluding the signature line and doc comments) reports 0 occurrences inside the scripted implementation's function bodies.

Phase: 1.

### AC-6: The transcript projector merges exactly three streams with a stated ordering rule

The fixture's `ticks` list is built from `action-log.jsonl` (`{at, track, action, station}`, one line per resolved action, chronological by append order — see Contract 3's merge rule), each entry's `prompt` field populated from that SAME tick's in-memory `TickResult.prompt` (captured the moment `run_tick_with_hosting` returns it), never re-read from `.darkrun/<slug>/prompts/<scope>/<tag>.md` after the run completes (`StateStore::write_prompt`, `crates/darkrun-core/src/state.rs`, overwrites that path per station/tag — a second occurrence of the same action tag silently clobbers the first). The fixture's `events` list is a separate, parallel projection of `events.jsonl` (`{at, event, run, ...fields}`, `crates/darkrun-mcp/src/events.rs`), not interleaved 1:1 with `ticks` (`darkrun.run.created` and `darkrun.station.dropped` fire with no `action-log.jsonl` counterpart).

Check: `cargo test -p darkrun-sim transcript::` includes a test asserting a regenerated fixture's `ticks.len()` equals the line count of that run's `action-log.jsonl` and `events.len()` equals the line count of `events.jsonl`; a second test seeds a scenario where the SAME action tag/station pair occurs on two different ticks (e.g. two `spec` ticks for the same station across a fix-feedback loop) and asserts both ticks' distinct prompt text survive in the fixture (proving the projector did not read the clobbered on-disk file).

Phase: 1.

### AC-7: Every fixture carries a `schema_version` and passes normalization

The fixture type's top-level `schema_version: u32` field is present on every regenerated fixture and equals the crate's own constant. No serialized fixture contains an RFC3339 timestamp pattern or the literal minted `verifier_nonce` hash (`crates/darkrun-mcp/src/position.rs`'s `mint_verifier_nonce`, seeded from `Utc::now()` and interpolated into the rendered prompt at `plugin/prompts/phases/manufacture.md` line 73's `{{ verifier_nonce }}`).

Check: `cargo test -p darkrun-sim fixture::` asserts `schema_version` is set; `grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}' <serialized fixture JSON>` returns nothing; a dedicated test captures the `verifier_nonce` minted for a scripted run's Manufacture dispatch and asserts the fixture's stored prompt text contains the fixed placeholder token instead of that value.

Phase: 1.

### AC-8: Regenerating the fixture twice is byte-identical

Running the scripted scenario to completion twice in the same process (or two separate processes) and projecting both into the fixture schema produces byte-identical serialized output, after normalization.

Check: `cargo test -p darkrun-sim regenerate_twice_is_byte_equal` runs the world+provider+transcript pipeline twice against two independent temp directories and asserts `serde_json::to_string(&fixture_a) == serde_json::to_string(&fixture_b)`.

Phase: 1.

### AC-9: An induced no-progress loop is detected as `Escalated`

A scripted scenario that deliberately never satisfies a `Spec` action's requirements (never decomposes a unit, never seals) trips the engine's own deadlock guard (`crates/darkrun-mcp/src/deadlock.rs`, `HALT_THRESHOLD = 4`) and the resulting `RunAction::Escalate` is captured as the fixture's `FixtureOutcome::Escalated` variant — the sim's entire red-verdict vocabulary, per the frame's Goal section.

Check: `cargo test -p darkrun-sim escalate_scenario_is_detected_red` feeds the world a provider that always returns `ProviderMove::Stop` from tick one and asserts the resulting fixture's `outcome` is `FixtureOutcome::Escalated { .. }` within the first `HALT_THRESHOLD + 1` ticks.

Phase: 1.

### AC-10: Every non-Escalate engine edge has a named, defined outcome

The edges enumerated in `## Edge cases` below (a rejected move, an exhausted script, a dark-mode `FeedbackQuestion`, a `render_prompt` `None`, the `STALE_AGE_SECS` reset, a double `run_start`, post-`Sealed` ticks, an empty prompts directory for an unreached station, a fixture referencing content the site no longer embeds) each resolve to one of exactly two named outcomes: a `FixtureOutcome` variant (a legitimate, transcript-visible result) or a harness failure (an `.expect()`/panic that fails the regenerating test loudly). No edge is left to silently pass or silently swallow.

Check: for each edge case named in `## Edge cases`, a `cargo test -p darkrun-sim` test exists asserting the specific outcome named there (a `FixtureOutcome` variant match, or `#[should_panic]`/`Result::is_err()` for the harness-failure class).

Phase: 1.

### AC-11: The prompt-wording linter partition survives the rebuild unweakened

`crates/darkrun-sim/tests/followability.rs` is not edited by this Run's build work, and its suite still passes after `harness.rs` is rebuilt onto the locked seams.

Check: `cargo test -p darkrun-sim --test followability` exits 0.

Phase: 1.

### AC-12: The replay route is registered in both the `Route` enum and `all_paths()`

`web/site/src/route.rs` gains a new `Routable` variant (modeled on the existing `Preview {}` entry) AND a matching literal string pushed into the `vec![...]` inside `Route::all_paths()` (the same function that already lists `"/preview".to_string()`). A route present only in the `#[derive(Routable)]` enum compiles and serves in the dev router while never reaching the static wasm export, since the site generator walks `all_paths()`, not the enum, to decide what to pre-render.

Check: `grep -n '#\[route("/replay")\]' web/site/src/route.rs` returns one match, and `grep -n '"/replay".to_string()' web/site/src/route.rs` returns one match inside the `all_paths()` function body (confirmed by `grep -n -A2 '"/replay"' web/site/src/route.rs` showing it inside the same `vec![` block as `"/preview".to_string()`, currently at `web/site/src/route.rs` line 124).

Phase: 2.

### AC-13: The replay page composes the darkrun-ui prelude components, bans no-live-feed, bans network

The new page module renders `StationStrip` (`crates/darkrun-ui/src/components/station_strip.rs`), `StationPipeline` (`crates/darkrun-ui/src/components/pipeline.rs`), and `UnitGraph` (`crates/darkrun-ui/src/graph/view.rs`) — all three re-exported by `darkrun_ui::prelude` (`crates/darkrun-ui/src/lib.rs`) — against data derived from the embedded fixture, displays an explicit no-live-feed banner (mirroring `web/site/src/pages/preview.rs`'s `ScaffoldNote` pattern and its "Preview only — no live feed is attached" wording), and performs zero network fetches.

Check: `grep -n 'StationStrip\|StationPipeline\|UnitGraph' <replay page module>` (the new page module under `web/site/src/pages/`, once it exists) shows all three names used inside an `rsx!` block; `grep -in 'no live feed\|no-live-feed' <replay page module>` returns at least one match; `grep -nE 'gloo|remote::|\.fetch\(' <replay page module>` returns nothing (contrast with `web/site/src/pages/browse.rs`, which does use `crate::remote::fetch_run_list`/`fetch_run_detail` for its live-repo pattern — the replay page must not).

Phase: 2.

### AC-14: web/site gains no `darkrun-mcp` dependency edge

Placing the fixture schema in `crates/darkrun-core` (whose only native, non-wasm dependency is `nix`, gated `[target.'cfg(unix)'.dependencies]`) rather than reusing `darkrun-mcp`'s `TickResult`/`RunAction`/`Position` types (which derive `Serialize` only, and whose crate carries unconditional `ureq`, `tokio`, `nix`, and `rmcp` dependencies) keeps `web/site` (package `darkrun-site`) wasm-clean.

Check: `cargo tree -p darkrun-site -e normal | grep -c darkrun-mcp` reports 0, both before and after this Run's changes land.

Phase: 2.

### AC-15: The replay route renders the committed fixture with no engine process running

Loading `/replay` in the built site displays the transcript of the one committed, Phase-1-produced fixture — no engine process, no MCP server, no `StateStore` reachable from the browser.

Check: `cargo build -p darkrun-site --target wasm32-unknown-unknown` succeeds with the fixture `include_str!`-embedded (mirroring `web/site/src/content.rs`'s existing `include_str!` pattern for markdown), and a local static-server load of `/replay` (or the site generator's pre-rendered HTML for that path) shows non-empty station/unit content sourced from the embedded JSON, not a loading spinner or fetch error state.

Phase: 3.

### AC-16: CI regenerates the fixture and gates on divergence and on Escalate

A CI job runs the darkrun-sim fixture-regeneration path and fails the build if the regenerated fixture differs from the committed copy, or if the regenerated run's `FixtureOutcome` is `Escalated` while the committed fixture's is `Sealed`. The gate has a real, exercised failure mode (not a check that can never turn red): AC-9's dedicated escalate-scenario test proves the same detection path the CI job depends on actually fires.

Check: `.github/workflows/ci.yml` (today's wasm job, named `wasm-app`, scopes `-p darkrun-app` only per its `cargo clippy -p darkrun-app --target wasm32-unknown-unknown` step) gains either a new job or an extension whose steps include running the regeneration entry point and a `diff` (or byte-equality assertion) against the committed fixture path, non-zero exit on mismatch; `cargo test -p darkrun-sim escalate_scenario_is_detected_red` (AC-9) passing is the proof the gate's Escalate branch is reachable, not dead code.

Phase: 3.

## Contracts

### Contract 1: the provider trait

```
pub trait Provider {
    /// One decision cycle. `prompt` is the CURRENT tick's `TickResult.prompt`
    /// (`None` for a tick with no rendered text). The signature carries no
    /// parameter for `TickResult.action` — the interface structurally
    /// withholds it, mirroring the frame's "read only `.prompt`" seam.
    fn next_move(&mut self, prompt: Option<&str>) -> ProviderMove;
}

pub enum ProviderMove {
    /// Seed the current station's wave (mirrors the existing linter's
    /// `Harness::seed_spec`) and clear its elaboration hold.
    AdvanceStation,
    /// Mark every unit in the current wave completed.
    CompleteWave,
    /// Approve the current hold (`UserGate`, a non-auto `Checkpoint`, or
    /// `ExternalReviewRequested`) with no feedback.
    Approve,
    /// Stamp every named run-level reviewer.
    StampRunReviewers,
    /// Nothing to do this call (a mid-wave `Noop`, or a terminal
    /// `Sealed`/`Escalate` the world has already observed).
    Stop,
}
```

The `Provider` trait's own method signature never receives a station or unit identifier: the `world` module resolves WHICH station/unit a `ProviderMove` targets using the same direct `StateStore` reads the existing linter's `Harness::active_station` already uses (a `read_state` call, not a `TickResult.action` read) — a channel the frame's Engine seams do not forbid, since the constraint is specifically about `TickResult.action`, not about all engine-state reads. The scripted implementation of `Provider` may condition `next_move`'s return value ONLY on its own private, internal state (an owned step counter or fixed move sequence); it MUST NOT parse, match on, or otherwise branch on the `prompt` parameter's content. `TickResult.action` is read by the `world` module only inside its post-hoc grading/projection path, after a `ProviderMove` has already been chosen and executed.

### Contract 2: `NoopHosting`

```
pub struct NoopHosting;

impl Hosting for NoopHosting {
    fn available(&self) -> bool { false }
    fn open_draft(&self, _req: &OpenRequest) -> Option<String> { None }
    fn merge_state(&self, _pr_ref: &str) -> MergeState { MergeState::Unknown }
}
```

`available() == false` means `resolve_discrete_gate` (`crates/darkrun-mcp/src/position.rs`) never attempts an `open_draft` call — it falls through to its own case 4 ("no hosting client: the `external` gate surfaces as `ExternalReviewRequested`"). Since `Mode::Dark`'s `opens_station_pr()` (`crates/darkrun-core/src/domain.rs`) is always `false`, the discrete-gate path is unreachable in this run's default dark-mode scenarios regardless; `NoopHosting` still implements all three non-defaulted trait methods because the trait bound on `run_tick_with_hosting<H: Hosting>` requires a complete `Hosting` impl to compile, exercised or not.

### Contract 3: the darkrun-core fixture schema

A new `sim_fixture` module is added under `crates/darkrun-core/src/` and declared with `pub mod sim_fixture;` in the existing `crates/darkrun-core/src/lib.rs`. Every type below derives both `Serialize` and `Deserialize` (unlike `darkrun-mcp`'s `TickResult`/`RunAction`/`Position`, which derive `Serialize` only) so the sim can write it and the site can read it without either crate depending on the other.

```
pub const SIM_FIXTURE_SCHEMA_VERSION: u32 = 1;

pub struct SimFixture {
    pub schema_version: u32,
    pub run_slug: String,
    pub factory: String,
    pub mode: String,              // "dark"
    pub outcome: FixtureOutcome,
    pub ticks: Vec<FixtureTick>,
    pub events: Vec<FixtureEvent>,
}

pub enum FixtureOutcome {
    Sealed,
    Escalated { reason: String },
}

pub struct FixtureTick {
    pub seq: u32,
    pub track: String,             // "run" | "feedback"
    pub action_tag: String,
    pub station: Option<String>,
    pub prompt: Option<String>,    // normalized
}

pub struct FixtureEvent {
    pub seq: u32,
    pub event: String,
    pub fields: serde_json::Value, // normalized
}
```

Normalization rules, applied before serialization (per the frame's "determinism by normalization at projection" decision):

1. Every RFC3339 timestamp value (`action-log.jsonl`'s `at`, `events.jsonl`'s `at`, `UnitFrontmatter.started_at`/`completed_at`, and `UnitIteration.started_at`/`completed_at` — all defined in `crates/darkrun-core/src/domain.rs` — wherever any of these surface inside a captured prompt body via a rendered `UnitSpecCard` or `Handoff`) is replaced with the fixed placeholder string `"<normalized>"`.
2. The minted `verifier_nonce` value (`mint_verifier_nonce`, `crates/darkrun-mcp/src/position.rs`, seeded from `Utc::now()`) is replaced, wherever its literal hash string appears inside a captured prompt body (per its `{{ verifier_nonce }}` interpolation in `plugin/prompts/phases/manufacture.md`), with the fixed placeholder token `<nonce>`.
3. `.darkrun/<slug>/deadlock.json` (`crates/darkrun-mcp/src/deadlock.rs`) is never embedded in the fixture at all; the deadlock guard's outcome is captured only as `FixtureOutcome::Escalated`, never as raw history-file content.

### Contract 4: the crate module map after partition

- Untouched (no behavioral change required by this Run): `crates/darkrun-sim/src/agent.rs`, `crates/darkrun-sim/src/tool_registry.rs`, `crates/darkrun-sim/tests/followability.rs`.
- Rebuilt (a fresh implementation replacing the current file's contents, not an incremental patch, per the operator's "extending the current harness in place is forbidden"): `crates/darkrun-sim/src/harness.rs`. Its role narrows from "drive a Run AND decide what to do next by matching `.action`" to "own the tempdir/`StateStore` and execute one `run_tick_with_hosting` call per invocation," exposing `.action` only for the linter's own post-hoc prompt-capture bookkeeping.
- Adjusted as needed to keep sourcing representative prompts from the rebuilt `harness.rs` without changing what it asserts: `crates/darkrun-sim/src/scenarios.rs` (today's solo-mode call at line 53 is a linter-only concern and stays out of this Run's dark-mode-first scope).
- New: three modules named `world`, `provider`, and `transcript`, added as files under `crates/darkrun-sim/src/`, each declared with a `pub mod` line added to the existing `crates/darkrun-sim/src/lib.rs`.
- New: the `sim_fixture` module under `crates/darkrun-core/src/` (Contract 3).

### Contract 5: the fixture file's committed path and embedding mechanism

The scripted dark-mode scenario's regenerated, normalized fixture is committed as a single JSON file under a new `fixtures/` directory inside the `darkrun-sim` crate, named `dark-core.json`. `web/site` embeds it at compile time via `include_str!`, the same mechanism `web/site/src/content.rs` already uses for its markdown corpus, then `serde_json::from_str::<darkrun_core::sim_fixture::SimFixture>(..)` deserializes it inside the new replay page's component body — no filesystem read at wasm runtime, no build-time code generation beyond the existing `include_str!` macro.

### Contract 6: the replay `Route` contract

- Variant name: `Replay {}`, added to the `Route` enum in `web/site/src/route.rs` alongside the existing `Preview {}` entry (both under the `#[layout(Shell)]` block).
- URL pattern: `#[route("/replay")]` — a static path with no dynamic segment, since Phase 2/3 ships exactly one committed fixture (mirrors `/preview`'s static route, not `/browse/:..rest`'s dynamic one).
- `all_paths()` entry: the literal `"/replay".to_string()` pushed into the `vec![...]` inside `Route::all_paths()` (`web/site/src/route.rs`), in the same static list that already contains `"/preview".to_string()`.
- darkrun-ui components composed: `StationStrip` (`crates/darkrun-ui/src/components/station_strip.rs`), `StationPipeline` (`crates/darkrun-ui/src/components/pipeline.rs`), `UnitGraph` (`crates/darkrun-ui/src/graph/view.rs`) — imported via `darkrun_ui::prelude::*`, the same glob import `web/site/src/pages/preview.rs` uses.
- No-live-feed banner: an explicit, visible banner stating no live engine backs the page, in the visual/textual style of `web/site/src/pages/preview.rs`'s "Preview only — no live feed is attached" copy (its `ScaffoldNote` component, imported from `crate::pages::review::ScaffoldNote`).
- No-fetch rule: the new page module contains no `gloo`, `remote::`, or `.fetch(` token — all data comes from the `include_str!`-embedded fixture (Contract 5), deserialized once at component-render time.

## Edge cases

### A scripted move the engine rejects (`elaborate_seal` → `InvalidInput`)

`elaborate_seal` (`crates/darkrun-mcp/src/position.rs`) returns `McpError::InvalidInput` when `state.stations.get_mut(station)` finds no entry — i.e. the named station is not active. REQUIRED: this is a harness failure, not a protocol-fidelity finding. The scripted `Provider`'s internal script never targets a station it has not itself observed as active via the `world` module's own `StateStore` reads (Contract 1); if the engine rejects a call anyway, the `world` module's call site does not swallow the `Result` — it `.expect()`s (or propagates a hard `Err` the test harness surfaces as a failed test), because a rejected mechanical move signals a bug in the driver's own bookkeeping, never a fact the deadlock guard or the transcript encodes as a scored outcome.

### An exhausted script (the scripted provider runs out of moves before `Sealed`)

REQUIRED: `ProviderMove::Stop` is a legitimate return value ONLY when the `world` module has already observed a terminal `RunAction` (`Sealed` or `Escalate`) on the most recent tick. If the `world` module calls `Provider::next_move` again after the scripted implementation's fixed move sequence is exhausted, and the run has NOT reached a terminal action, this is a harness failure: the call panics with a message naming the exhausted step count, failing the regenerating test loudly rather than looping forever or silently returning a no-op that would mask a real followability gap.

### `RunAction::FeedbackQuestion` firing in dark mode

REQUIRED, mode-independent, and a legitimate (non-error) outcome: `walk_feedback` (`crates/darkrun-mcp/src/position.rs`) is called unconditionally on every tick regardless of `Mode`, so a `feedback_question` action tag can surface in `Mode::Dark` exactly as it can in any other mode. The default scripted dark-mode scenario's own linear walk never files a feedback item, so `feedback_question` never naturally appears in the default green fixture (the committed `dark-core.json` under the crate's `fixtures/` directory, Contract 5). It is exercised only by a SEPARATE, dedicated scenario that seeds an open feedback question directly against the `StateStore` before ticking (mirroring the existing linter's own `Harness::render` direct-render approach at `crates/darkrun-sim/src/scenarios.rs` lines 64-78) and is captured as its own, separately-committed fixture — never folded into the default `Sealed` fixture's `ticks` list.

### `render_prompt` returning `None` for an unmapped action tag

REQUIRED: `render_prompt` (`crates/darkrun-mcp/src/position.rs`) returns `Ok(None)` only when `darkrun_prompts::template_key_for_action` (`crates/darkrun-prompts/src/lib.rs`) has no entry for the tag; every tag in `darkrun_prompts::ACTION_TAGS` currently maps. A `#[test]` in the `transcript` module asserts, corpus-wide, that no captured `FixtureTick.prompt` is ever `None` for a non-terminal action tag; if one appears, the test fails (a harness failure signaling a genuine prompt-corpus regression), rather than the projector silently omitting that tick.

### The deadlock guard's `STALE_AGE_SECS` reset

REQUIRED: `STALE_AGE_SECS = 3600` (`crates/darkrun-mcp/src/deadlock.rs`) means a `deadlock.json` history untouched for more than 3600 wall-clock seconds is treated as fresh, zeroing the no-progress counter. The sim's default scripted scenario completes in well under this window (a bounded, small number of in-process ticks with no artificial delay), so this reset path is explicitly OUT of Phase 1's coverage — not silently ignored, but documented here as a known, unexercised path: darkrun-sim does not fake a slow clock or inject a sleep to reach it. Re-entry trigger for adding coverage: a future scenario that deliberately holds the process past `STALE_AGE_SECS` before resuming a wedge (a real, not simulated, wall-clock reset).

### A double `run_start` for the same slug

`run_start`'s `store.write_run(&run)` (`crates/darkrun-core/src/state.rs`, `write_atomic`) and its `store.write_state` overwrite unconditionally — the function has no existence guard against a slug that already has a `run.md`. REQUIRED: the `world` module is its own guard. Each `World` instance owns exactly one `tempfile::TempDir` and calls `run_start` exactly once, on construction; a second construction against an already-used slug within the same process is prevented at the `world` module's own API surface (a `World::new` that consumes a fresh tempdir and asserts, on debug builds, that it has not already been called for that slug), not left to the engine's silent-overwrite behavior.

### Post-`Sealed` ticks

REQUIRED: once every station is locked and no run-level reviewers remain unsigned, `derive_position` (`crates/darkrun-mcp/src/position.rs`) returns `RunAction::Sealed` indefinitely on every subsequent tick — no further state mutation occurs, and `Sealed` is in `deadlock.rs`'s `is_exempt` list, so it never triggers a false-positive `Escalate`. The `world` module's tick loop stops issuing `run_tick_with_hosting` calls the FIRST time it observes `Sealed` — it never ticks past it operationally. A dedicated test still asserts the engine's own idempotence contract directly (one manual extra tick after `Sealed` still returns `Sealed`), documenting the behavior without the sim's normal operation depending on it.

### An empty `prompts/` directory for a station the scripted run never reached

`StateStore::write_prompt` (`crates/darkrun-core/src/state.rs`) creates `prompts/<scope>/` only on first write for that scope; a station an `Escalated` or otherwise short-circuited run never reaches has no directory at all. REQUIRED: the `transcript` module's projector treats a missing `prompts/<station>/` path as "zero prompts captured for that station," never an error; the fixture's `ticks` list is bounded exactly by however many lines `action-log.jsonl` actually recorded for that run — it is never padded with placeholder entries for stations the run did not reach.

### A fixture referencing content the site build no longer embeds

REQUIRED: the committed fixture (Contract 5) is fully self-contained — every display string the replay page needs (station names, factory title, unit labels) is embedded IN the fixture's own JSON at regeneration time, never looked up live against `darkrun_content::list_factories()`/`darkrun_content::load_validated` (`web/site/src/route.rs`'s own live-corpus pattern) at render time. A later edit to the live factory corpus (renaming or removing a station) can therefore never break `/replay`'s rendering, because the replay page never re-resolves the fixture's station/unit names against the live corpus.

## Out of scope

- Build-quality assertions on anything the scripted Worker produces (spec prose quality, generated-code correctness) — inherited from the frame's Out of scope.
- Re-implementing engine mechanics inside the harness: no independent scheduler, no unit-pool logic, no dispatch-block parser, no drift or feedback state machine duplicated outside `darkrun-mcp` — inherited from the frame.
- A live engine or a live model reachable per website visitor — locked decision 4, not deferred, will not be revisited.
- `web/app` integration (the separate live-relay application) — inherited from the frame; the replay surface lives only in `web/site`.
- Solo/team-mode gate simulation (a scripted operator-sim answering questions and deciding checkpoints) — deferred per the frame's locked decision 1.
- Real-model provider selection — deferred per the frame's locked decision 2.
- In-browser LLM execution of any kind — dead permanently per the frame's locked decision 4.
- Reopening any of the frame's four locked operator decisions, or this station's three operator decisions (crate partition, fixture-schema placement, determinism-by-normalization).
- Engine-code changes of any kind for determinism: normalization happens ONLY at the transcript projector (Contract 3); `crates/darkrun-mcp` and `crates/darkrun-core`'s existing `verifier_nonce`/timestamp-minting behavior is untouched.
- A second sim crate: the frame-compliant simulator's `world`/`provider`/`transcript` modules land inside the existing `crates/darkrun-sim`, alongside (not replacing) the prompt-wording linter.
- Any web or desktop app work beyond the single new `/replay` route and its page module in `web/site`.
- A real-model (non-scripted) provider implementation of any kind.

## Evidence

- `.darkrun/darkrun-sim/frame/frame.md` (read from the sibling main worktree; not present on this unit's own branch history — see the process note below)
- `crates/darkrun-sim/src/harness.rs`
- `crates/darkrun-sim/src/agent.rs`
- `crates/darkrun-sim/src/tool_registry.rs`
- `crates/darkrun-sim/src/scenarios.rs`
- `crates/darkrun-sim/src/lib.rs`
- `crates/darkrun-sim/tests/followability.rs`
- `crates/darkrun-sim/Cargo.toml`
- `crates/darkrun-mcp/src/position.rs`
- `crates/darkrun-mcp/src/hosting.rs`
- `crates/darkrun-mcp/src/deadlock.rs`
- `crates/darkrun-mcp/src/units.rs`
- `crates/darkrun-mcp/src/events.rs`
- `crates/darkrun-mcp/src/error.rs`
- `crates/darkrun-core/src/state.rs`
- `crates/darkrun-core/src/domain.rs`
- `crates/darkrun-core/src/lib.rs`
- `crates/darkrun-core/Cargo.toml`
- `crates/darkrun-prompts/src/lib.rs`
- `crates/darkrun-ui/src/lib.rs`
- `crates/darkrun-ui/src/components/pipeline.rs`
- `crates/darkrun-ui/src/components/station_strip.rs`
- `crates/darkrun-ui/src/graph/view.rs`
- `web/site/src/route.rs`
- `web/site/src/pages/preview.rs`
- `web/site/src/pages/browse.rs`
- `web/site/src/content.rs`
- `web/site/Cargo.toml`
- `Cargo.toml` (workspace root, `members` glob)
- `.github/workflows/ci.yml` (the `wasm-app` job's `-p darkrun-app` scope)
- `plugin/prompts/phases/manufacture.md` (the `{{ verifier_nonce }}` interpolation, line 73)

Process note: this unit's own branch (`darkrun/darkrun-sim/units/specify/author-spec`) diverged from `darkrun/darkrun-sim/main` at the same commit the frame station landed from, so `.darkrun/darkrun-sim/frame/frame.md` is not present in this branch's own git history. Its content was read directly from the sibling main worktree's working tree (a read-only filesystem read, no git operation against any branch other than this unit's own) and is quoted/paraphrased faithfully above; every source-code claim was independently re-verified by reading the cited files in THIS worktree.
