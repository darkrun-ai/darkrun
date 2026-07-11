# Frame: darkrun-sim — protocol-fidelity simulator

darkrun-sim is a protocol-fidelity simulator: it puts a deliberately dumb, zero-privileged-knowledge agent in the driver's seat of the real darkrun engine, driving `darkrun-mcp` in-process the way a Claude Code agent would, acting on nothing but the rendered prompt text the engine hands back each tick.

It exists to answer one question the existing e2e suite cannot: given only what a real agent would read, does the engine's prompt corpus alone carry enough information to complete a Run without getting stuck?

A red run has one definition: the engine's own deadlock guard fired `RunAction::Escalate` (`crates/darkrun-mcp/src/deadlock.rs`) because the zero-knowledge agent's prompt-driven actions produced no progress.

## The gap this closes

The e2e suite already drives the real engine end to end, but with privileged knowledge the corpus itself never grants to an agent. `Harness::tick` in `crates/darkrun-e2e/tests/common/mod.rs` calls `run_tick` and returns a `TickResult`; every helper built on it — `walk_station_to_checkpoint`, `complete_station`, `run_to_seal` — pattern-matches only the structured `TickResult.action` enum (`RunAction::Spec`, `RunAction::Manufacture`, `RunAction::Checkpoint`, `RunAction::UserGate`, and so on) to decide what to do next, and stamps exactly what the cursor is checking for: `elaborate_seal` clears a held Spec, `run_review_stamp` satisfies the whole-run review hold, direct `complete_unit` writes flip a unit's status. Nowhere in that file, nor in the suites built on it, is `TickResult.prompt` read. A green e2e run proves the manager's cursor terminates deterministically when driven by code that already knows its shape in advance. It proves nothing about whether the rendered prompt text — the only thing a real Claude Code agent ever sees — actually communicates what to do.

`crates/darkrun-mcp/src/deadlock.rs` is the mechanism that would catch a real agent's confusion: it fingerprints each tick's action against run progress (station, unit counts, completed counts, Pass sum, drift, feedback) and, once the same no-progress signature repeats past `HALT_THRESHOLD` (4 ticks) or two signatures churn across an 8-of-10-tick window, swaps the wedged action for `RunAction::Escalate` — exempting legitimate external-await actions (`UserGate`, `Checkpoint`, `PendingSeal`, `ExternalReviewRequested`, `Sealed`, `MergeConflict`, an already-fired `Escalate`, `FeedbackQuestion`) from the count. A stranded zero-knowledge agent — one that cannot infer what a tick wants beyond what the prompt says — trips this within roughly five ticks of going off the rails. `Escalate` is the sim's entire red-verdict vocabulary; darkrun-sim invents no separate stall detector, no timeout, no heuristic of its own. If the engine doesn't escalate and the agent completed the run, that is the sim's green.

## Goal (load-bearing)

The measured property is protocol fidelity, not build quality. Concretely:

- **Red** means the engine's own deadlock guard fired `RunAction::Escalate` because the zero-knowledge agent's prompt-driven actions produced no progress. This is the sim's entire red-verdict vocabulary for this run's scope. A subtler followability failure — an off-prompt action the engine rejects short of a full escalate, the kind `enforce_unit_scope` (`crates/darkrun-mcp/src/units.rs`) would flag by firing `darkrun.unit.scope_violation` — is not detectable in the sim's default world: that mechanism diffs git branches, and it returns `Ok(())` unconditionally when `Git::open` fails, which it always does against the bare `tempfile::TempDir` this run uses (Engine seams: "No git repo required"). No git-backed world exists in this run's scope, so darkrun-sim does not claim to detect non-Escalate followability failures; it measures Escalate-class breakdowns only.
- **Green** means "protocol flowed": every tick's rendered prompt communicated what the agent needed to do, the agent's action satisfied it, and the manager's cursor advanced the Run to `Sealed` (or as far as the scripted scenario runs) without the deadlock guard firing `Escalate`.
- Green is never "it compiles," "the tests pass," or "the produced spec/code is good." The scripted Worker's own output quality — whether a `frame.md` it writes is well-argued, whether generated code is idiomatic — is not scored by darkrun-sim at all. A run can be sim-green while producing artifacts a human reviewer would reject; that gap is a real gap, but it belongs to a different kind of verification than protocol fidelity.
- Green in Phase 1 proves the harness spine, the world, the transcript substrate, and the replay pipeline work end to end — it does not prove prompt followability. The scripted provider (locked decision 2) receives each tick's `.prompt` but does not condition its decision on that text, so a scripted-green run cannot show that a real reader of the prompt would have found its way through. That proof is deferred to the real dumb-model recorder named in locked decision 2; until that provider exists and drives the same trait, "Phase 1 green" means the harness works, not that the prompt corpus is followable.

## Locked decisions

Locked 2026-07-02 via `darkrun_question` sessions q-01/q-02. Each carries its re-entry trigger where one exists.

1. **Dark-mode spine first; scripted operator-sim later.** v1 fixtures drive `dark`-mode runs only: pre-elaborated, linear, no per-station collaboration hold. A scripted operator-sim that answers `darkrun_question` calls, decides checkpoints, and seals specs is a deferred later phase; solo/team-mode gate-surfacing coverage (the interactive holds `Harness::seal` and `Harness::decompose` clear in the e2e harness) arrives with it, not with v1. Re-entry trigger: the spine and the replay player are merged and green in CI.
2. **Scripted provider only; real-model recorder deferred.** The agent-side decision-making is a pluggable provider trait from the start. The only implementation this run ships is the scripted, deterministic, no-LLM provider: today's privileged e2e move sequences (decompose-then-seal, complete-then-tick) formalized behind that trait so the harness can be driven without a live model. The scripted provider receives each tick's `.prompt` as its trait input — the same input a real provider would get — but it does not parse or condition its decision on that text; it returns a pre-determined move regardless of what the prompt says. That means a scripted-green run validates the harness spine, the world, the transcript substrate, and the replay pipeline, but it does not validate prompt followability, because the provider's behavior does not depend on the prompt content at all. Selecting a real dumb-model recorder (hosted API or local model) whose decisions do depend on `.prompt` is deferred. Re-entry trigger: the replay page is live on the site with a committed scripted fixture.
3. **Placement.** A new workspace crate, `crates/darkrun-sim`, consumes `darkrun-mcp` as a library dependency: a path-dependency with no network and no subprocess, the same coupling shape `crates/darkrun-e2e` already proves out — though darkrun-e2e takes that dependency as a `[dev-dependencies]` edge on a test-only crate (its `[lib]` is intentionally empty; the coverage lives in `tests/`), while darkrun-sim needs a real `[dependencies]` edge, since the crate itself, not just a test suite, drives the engine. The workspace's `members` glob in `Cargo.toml` (`members = ["crates/*", "desktop", "web/site", "web/server", "web/app"]`) already admits any new `crates/*` entry without a workspace-file edit. The replay player lives in `web/site`, the existing client-side Dioxus wasm SPA.
4. **In-browser/wasm LLM execution is dead, permanently.** No wasm LLM inference target, zero LLM invocation code anywhere in the repo, no live engine and no live model reachable per website visitor. The only path from a run to the website is: record locally (in-process, on a developer machine or CI), commit the resulting fixture, replay it statically in the browser against pre-recorded data. This is not deferred — it will not be revisited.

## Engine seams

`darkrun-mcp` (`crates/darkrun-mcp/src/`) is a library crate, not a service; darkrun-sim drives it in-process exactly as `crates/darkrun-e2e/tests/common/mod.rs` does, and this is the only coupling surface the sim is permitted to use:

- **Start:** `StateStore::new(tempdir)` over a scratch `tempfile::TempDir`, then `run_start(&store, slug, factory, title, mode, "full")` — the same call `Harness::start_with` makes.
- **Advance:** loop `run_tick_with_hosting(store, slug, &hosting)` (`crates/darkrun-mcp/src/position.rs`), never the plain `run_tick`, which internally resolves `ApiHosting` (`crates/darkrun-mcp/src/hosting.rs`) and can reach out over the network for PR/merge operations. The sim supplies its own no-op `Hosting` implementation (mirroring the `Stub` pattern already used in `hosting.rs`'s own test module) so no tick ever attempts a real API call.
- **Read only `.prompt`.** The sim's agent acts strictly on `TickResult.prompt` — the rendered instruction text — never on `TickResult.action`. Consulting `.action` to decide what to do would silently re-inject the privileged knowledge the sim exists to remove; `.action` may only be read after the fact, to grade whether the agent's prompt-driven behavior lines up with what the engine actually needed.
- **Harness fidelity is exact for Claude Code.** `darkrun_harness::adapt_instructions` (`crates/darkrun-harness/src/lib.rs`) returns the prompt text unmodified whenever `caps.harness.is_claude_code()` is true — the function's first branch is a literal no-op for that harness. A Claude-Code-modeled sim agent therefore reads byte-identical prompt text to what a real Claude Code session would receive; there is no adapter-introduced drift to account for.
- **The transcript substrate already exists.** Every tick persists its rendered prompt to `.darkrun/<slug>/prompts/<station>/<tag>.md` (`crates/darkrun-mcp/src/position.rs`), appends a resolved-action entry to `action-log.jsonl`, and appends a lifecycle event to `events.jsonl` (`crates/darkrun-mcp/src/events.rs`, `crates/darkrun-core/src/state.rs`). darkrun-sim's transcript is a projection over these three streams, not a new persistence format.
- **No dispatch-block parser.** The prompt corpus (`plugin/prompts/`) contains no machine-parseable dispatch markup — `plugin/prompts/phases/manufacture.md` instructs "Dispatch the **{{ worker }}** beat in parallel across these wave-ready Units" as rendered prose inside a Markdown template; subagent dispatch is something the agent reading the prompt decides to do itself. The sim therefore builds no parser or structured-dispatch extractor; a scripted provider that never spawns real subagents is faithful to the same prompt a subagent-capable agent reads.
- **No git repo required.** A bare `tempfile::TempDir` with no `.git` is a legitimate substrate: the lifecycle's git operations are best-effort no-ops off that path (`crates/darkrun-mcp/src/position.rs`, the non-git branch of the land/PR flow), so the absence of a repository does not by itself change what the agent is told to do.

## Out of scope

- Build-quality assertions on anything the scripted Worker produces (spec prose quality, generated-code correctness, artifact completeness beyond what a tick's completion criteria check).
- Re-implementing engine mechanics inside the harness: no independent scheduler, no unit-pool logic, no dispatch-block parser, no drift or feedback state machine duplicated outside `darkrun-mcp`. If the engine already computes it, the sim calls into the engine for it.
- A live engine or a live model reachable per website visitor — locked decision 4, not deferred, will not be revisited.
- `web/app` integration. `web/app` is the separate live-relay application (Firebase auth, remote session control) and is explicitly excluded from darkrun-sim's scope; the replay surface lives only in `web/site`.
- Solo/team-mode gate simulation (a scripted operator-sim answering questions and deciding checkpoints). Deferred per locked decision 1. Re-entry trigger: the dark-mode spine and replay player are merged and green in CI.
- Real-model provider selection (which hosted API or local model would play the zero-knowledge agent for real, rather than via the scripted provider). Deferred per locked decision 2. Re-entry trigger: the replay page is live on the site with a committed scripted fixture.
- In-browser LLM execution of any kind. Dead permanently per locked decision 4; there is no re-entry trigger because this will not be revisited.
- Reopening any of the four locked operator decisions above.

## Build order (dependency-sequenced)

**Phase 1: world + transcript spine.** Stand up the new `crates/darkrun-sim` crate, consuming `darkrun-mcp` as a library exactly per the engine seams above: a scratch `StateStore` in a tempdir, `run_start` into `dark` mode, a loop over `run_tick_with_hosting` against a no-op `Hosting` impl. Define the provider trait and its scripted implementation (locked decision 2) — the only thing the loop calls to turn a rendered `.prompt` into the next engine-facing action. Build the transcript as an agent-scoped event log projected from the three already-persisted streams (`prompts/<station>/<tag>.md`, `action-log.jsonl`, `events.jsonl`), not a new format. Demoable independently: run a scripted dark-mode scenario to `Sealed` (or to a deliberately induced `Escalate`) entirely from the command line, with the transcript written to disk.

**Phase 2: replay player in `web/site`.** A new Route variant (alongside the existing `/browse` and `/preview` entries in `web/site/src/route.rs`) that renders Phase 1's transcript, modeled directly on the fixture-rendering pattern in `web/site/src/pages/preview.rs` (static payload, explicit no-live-feed banner, no submit/annotation wiring) rather than `web/site/src/pages/browse.rs`'s live-repo-fetch pattern, since darkrun-sim never has a live engine to point at (locked decision 4). Panels are built from the shared `darkrun-ui` component set (`crates/darkrun-ui/src/lib.rs`), the same renderer-agnostic Dioxus components `/browse` and `/preview` already consume, so the replay view matches the site's existing visual language. Demoable independently: load the replay route against one committed scripted fixture from Phase 1 and see the transcript render, with no engine process running.

**Phase 3: site/CI consumption.** Commit a scripted fixture (Phase 1's output) into the repo for the replay player (Phase 2) to load statically. Wire a CI job that regenerates the fixture from the scripted provider and fails the build if regeneration diverges from the committed copy or if the run's transcript contains an `Escalate` — the red-on-`Escalate` assertion is the only pass/fail gate this phase adds. Demoable independently: a CI run that regenerates and validates the fixture, plus the site build consuming the committed copy, both green without a human in the loop.

No time estimates anywhere in this build order — Phase 1 must exist before Phase 2 has a transcript to render, and Phase 2 must exist before Phase 3 has a replay surface to validate against; sequencing here is dependency order only.

## Evidence

Files read and verified in this worktree as the basis for every claim above:

- `crates/darkrun-e2e/tests/common/mod.rs`
- `crates/darkrun-e2e/Cargo.toml`
- `crates/darkrun-mcp/src/deadlock.rs`
- `crates/darkrun-mcp/src/units.rs`
- `crates/darkrun-mcp/src/position.rs`
- `crates/darkrun-mcp/src/hosting.rs`
- `crates/darkrun-mcp/src/events.rs`
- `crates/darkrun-core/src/state.rs`
- `crates/darkrun-harness/src/lib.rs`
- `crates/darkrun-ui/src/lib.rs`
- `web/site/src/pages/preview.rs`
- `web/site/src/pages/browse.rs`
- `web/site/src/route.rs`
- `plugin/prompts/phases/manufacture.md`
- `Cargo.toml` (workspace root, `members` glob)
