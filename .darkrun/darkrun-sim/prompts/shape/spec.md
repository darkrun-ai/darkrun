
> **Run** `darkrun-sim` · **Station** `shape` · **Phase** `spec`

> Eliminates: _expensive-structural-reversal_


# Spec — `shape`

You are opening station **shape**. Its job is to eliminate a whole class of risk: **expensive-structural-reversal**. Nothing downstream is allowed to proceed until that risk is named and bounded here.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`design.md`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (3): `surface`, `architecture`, `risk`


**Workers** (5): `designer` → `visual_designer` → `spiker` → `pressure_tester` → `resolver`


**Reviewers** (3): `fit`, `reversibility`, `simplicity`


Spec runs **elaboration and discovery in tandem** — they are NOT two sequential
steps. The moment the station opens, kick off both at once: dispatch the explorers
in parallel *while* you frame the problem. They sharpen each other. Only once both
have landed do you decompose.


## Keep or drop — decide at arrival, before any work

This station is **optional** for this run. Before you elaborate anything, judge whether its risk class — **expensive-structural-reversal** — actually applies here. If it plainly doesn't (the run is too small to carry the risk, or an upstream artifact already bounds it), drop the station now with `darkrun_station_drop` and the next `darkrun_tick` advances to the following station. The decision is only available **now**: once elaboration or units exist the station has started, and a started station can only be reset, never dropped. Keeping it is the default — drop only when you can say in one sentence why the risk doesn't apply.


## elaborate — frame the problem (concurrently with discovery)

State plainly what this station must achieve to kill **expensive-structural-reversal**: the intent, the inputs it inherits from upstream, and the boundary of what is explicitly *out of scope* so later phases don't drift into it. This is the frame the explorers work against — but do NOT wait on a finished frame to start them; the frame and the exploration are written in parallel and inform each other.

## discover — run the explorers in parallel (concurrently with elaboration)

Dispatch **all** explorers (`surface`, `architecture`, `risk`) **at once, in parallel** — one subagent each, fanned out concurrently, never one-after-another. Explorers don't build — they surface unknowns, constraints, prior art, and traps. They run alongside your framing; neither blocks the other.


**Project knowledge (priors from earlier runs)** — build on these, don't re-discover them:

- **deadlock-escalate-is-stranded-verdict** — `crates/darkrun-mcp/src/deadlock.rs` is the engine's cross-tick wheel-spin guard (the predecessor's HALT_THRESHOLD ported forward — this bug class is a scar, not a hypothetical). After 4 same-signature no-progress ticks, or a two-signature A↔B churn over ≥8 ticks, `run_tick` swaps the wedged action for `RunAction::Escalate { reason }`. External-await actions are exempt; per-run history lives in `.darkrun/<slug>/deadlock.json` and resets after STALE_AGE_SECS=3600, so a bounded sim run must complete inside the hour window. For any stranded-agent/protocol-fidelity test, `Escalate` is the machine-readable red verdict — key pass/fail off it instead of inventing new stall detection. Note its limit: it catches the ENGINE refusing to advance; only a zero-knowledge agent in the seat extends it to catch prompts that never taught the agent what to stamp.

- **existing-darkrun-sim-crate-is-a-prompt-linter** — crates/darkrun-sim already exists on run-main but is a DIFFERENT tool than the protocol-fidelity simulator the darkrun-sim frame locks: it is a prompt-wording linter. SimAgent::read (src/agent.rs:153) is a pure text classifier over darkrun_* tool tokens; harness.rs drives a privileged walk that calls plain run_tick (src/harness.rs:21,69 — the network-reaching path the frame forbids), pattern-matches TickResult.action to decide moves (src/harness.rs:150-196 — the exact privileged-knowledge shape the frame condemns), and runs solo mode not dark (src/scenarios.rs:53). tests/followability.rs is a static corpus scan (every reachable prompt names only registered tools, via tool_registry.rs's include_str! parse of #[tool(name=...)] attributes — the rmcp tool-list accessor is crate-private). Its Cargo.toml already takes darkrun-core + darkrun-mcp as real [dependencies]. Any frame-compliant simulator work must NOT silently extend harness.rs in place; the .action-reading and run_tick violations would be perpetuated. The linter half (agent.rs, tool_registry.rs, followability tests) is independently valuable and CI-green today.

- **fixture-determinism-traps** — Recording a darkrun engine transcript for byte-diff CI regeneration hits these confirmed nondeterminism/alignment traps: (1) verifier_nonce — mint_verifier_nonce (crates/darkrun-mcp/src/position.rs:3039-3043) hashes slug+station+Utc::now() on every Manufacture entry and the Manufacture template prints it literally (plugin/prompts/phases/manufacture.md:72-74), so rendered Manufacture prompts differ byte-for-byte on every regeneration — any fixture-diff gate must freeze the clock or normalize the nonce out before diffing. (2) events.jsonl is NOT 1:1 with action-log.jsonl — darkrun.run.created (position.rs:3245) and darkrun.station.dropped (position.rs:959) plus emits in runs.rs:251 and units.rs:485,493 have no action-log counterpart; a transcript projector needs an explicit merge rule, not cardinality assumptions. (3) journal lines carry no schema_version stamp (append_action_log, position.rs:2612-2624) — fixtures recorded at engine version N have no drift signal for a replayer built at N+1. (4) deadlock history resets after STALE_AGE_SECS=3600 (deadlock.rs:44,161-165) — a wall-clock-slow run silently zeroes its no-progress counter (false-green risk for stall tests). (5) Absolute worktree paths do NOT leak into prompts on a bare tempdir world — worktree context is gated behind git_backed_station (position.rs:2922-2937), false when Git::open fails.

- **sim-prompt-surface-contract** — The engine's followability surface for a zero-knowledge (sim) agent is `TickResult { run, position, action, prompt }` from darkrun-mcp's position module. `.prompt` is the rendered markdown the agent reads; `.action` is the structured variant the privileged e2e driver reads (`crates/darkrun-e2e/tests/common/mod.rs::run_to_seal` never touches `.prompt` — exactly why e2e green proves cursor termination, not followability). A protocol-fidelity consumer must act on `.prompt` only. darkrun-mcp is a lib crate (binary lives in darkrun-cli), so drive ticks in-process: `StateStore::new(dir)` → `run_start(...)` → loop `run_tick_with_hosting(store, slug, &NoopHosting)` — plain `run_tick` resolves ApiHosting and can touch network in discrete mode. For a Claude-Code-modeled agent the raw rendered prompt is byte-identical to production (`darkrun_harness::adapt_instructions` is the identity for the Claude Code cap set); other cap sets append harness notes. Every tick also persists the rendered prompt under `.darkrun/<slug>/prompts/<scope>/<tag>.md` (`StateStore::write_prompt`/`read_prompts`), alongside `action-log.jsonl` and `events.jsonl` — a ready-made transcript/replay substrate.

- **site-replay-substrate** — web/site is a client-side Dioxus wasm SPA (dioxus-router; darkrun-site-gen emits SEO artifacts only — it is NOT a pre-rendered SSG). Record/replay-without-a-live-engine is its established architecture: `/preview` renders real darkrun-api session payload fixtures with an explicit no-live-feed banner; `/browse` fetches a repo's committed `.darkrun/` tree over CORS HTTP and re-derives state client-side via darkrun-core (which compiles to wasm); the statusline demo embeds an offline ANSI→HTML snapshot. There is no shared normalized statusline state type — the CLI renders ANSI inline from StateStore (`crates/darkrun-cli/src/statusline.rs`), so a web statusline is a new projection best built from shared darkrun-ui components (the station strip / phase pipeline / unit DAG that `/browse` draws), not the CLI renderer. A replay player belongs in web/site as a new Route variant modeled on `/preview`; web/app (app.darkrun.ai) is the separate live-relay wasm app and is NOT where replay belongs.

- **subagent-dispatch-is-prose** — The prompt corpus (`plugin/prompts/`, embedded by darkrun-prompts with a project `.darkrun/prompts/` → plugin-root → embedded override cascade) contains NO machine-parseable subagent dispatch markup — no `<subagent>`/`<dispatch>`/relay blocks anywhere. Manufacture prompts instruct in prose: dispatch the worker beat in parallel across wave-ready Units and pass each Unit's spec verbatim into the dispatch; the agent spawns subagents itself. Pool/scheduling behavior is prompt prose, not structured blocks — so any sim or tooling must NOT build a dispatch-block parser; followability of the prose IS the surface under test. Action→template mapping is `darkrun_prompts::template_key_for_action` (23 keys); a bare tempdir with no overrides resolves deterministically to the embedded corpus.

- **wasm-boundary-for-fixture-types** — web/site (crate darkrun-site) depends on darkrun-ui, darkrun-api, darkrun-content, darkrun-core — never darkrun-mcp (unconditional nix/tokio/ureq/rmcp deps make it and anything depending on it, including crates/darkrun-sim, non-wasm). TickResult/RunAction/Position are Serialize-ONLY (no Deserialize, position.rs:69,243,252), so a replay fixture cannot round-trip engine types into the site. Any recorded-transcript payload the site replays needs a hand-rolled wasm-safe serde schema living in a wasm-clean crate (darkrun-core is the established home: its only native dep nix is cfg(unix)-gated for domain types). Site fixture-embedding precedents: include_str! (web/site/src/content.rs, 18+ call sites) and rust_embed (crates/darkrun-content/src/loader.rs:18-30); no JSON/transcript asset precedent exists yet. CI gap: no workflow builds web/site for wasm32 (ci.yml's wasm-app job scopes -p darkrun-app only); a site-consuming feature has no CI gate until deploy-web.yml runs.



When discovery surfaces a durable project fact worth carrying into **future** runs — a constraint, prior art, a convention, a trap — persist it with **`darkrun_knowledge_record`** (`topic` + `body`). That's the project's shared memory; re-recording a topic updates it. Keep it project-level (cross-run truths), not this run's transient details.

## decompose — once elaboration + discovery have both landed

Turn the framed, explored problem into the smallest set of independently completable **Units** that, together, kill the risk above. A Unit's **body is the spec the executing subagent works from — it gets no other context**. A one-line body is a slug, not a definition; the work that comes back from a thin Unit is thin.

Write every Unit with `darkrun_unit_create`, with the full anatomy:

- **`body`** — the real definition, in markdown:
  - the goal: what this Unit produces and why it exists in this station,
  - **completion criteria, EACH paired with the literal command that verifies it.** Inspect the project's manifest (`Cargo.toml` / `package.json` / `pyproject.toml` / `go.mod` …) *during decompose* and write commands against THIS project's actual stack — never a placeholder.
    - Good: "all endpoints return correct status codes (200/400/401/404)" → `cargo test -p api contracts` exits 0.
    - Bad: "API works correctly", "tests are written" — no check, no criterion.
  - for build-class Units: the **success path, the failure path, and the edge cases** the criteria must cover,
  - for knowledge/document Units: substantive criteria — what claims the artifact must ground, with sources,
  - the **files touched** (so review knows the blast radius),
  - what is explicitly **out of scope** (so the Unit doesn't sprawl).
- **`depends_on`** — every cross-Unit prerequisite, DECLARED, never left in prose. The wave scheduler sequences **only** on `depends_on`; a dependency mentioned in the body but not declared is invisible — the Unit gets co-scheduled with its own prerequisite and handed inputs that don't exist yet. A body that says "stub it until unit-X lands" is the symptom of a missing `depends_on` edge: declare the edge instead of writing the stub.
- **`inputs` / `outputs`** — the paths consumed and produced. A sibling-produced input path requires that sibling in `depends_on`.
- **`quality_gates`** — executable `{name, command}` checks proving the criteria. Required for any Unit that declares outputs. Each gate must pass **in the Unit's own isolated worktree at the time it runs** — a gate that needs a sibling's unmerged code, with no `depends_on` edge to order it, is not a gate, it's a Unit scheduled to fail. Circular gates (zero-match `! grep`, prose substrings against the Unit's own output) are rejected.
- **`model`** — match the tier to the risk: `opus` for architectural, cascading-failure, or deepest-reasoning work, `sonnet` (default) for known patterns plus judgment, `haiku` only for purely mechanical edits.


There are no Units yet. You are creating them.





## Collaborate with the operator — required before this spec locks

This run is in a **collaborative mode**, and the station will not advance to Review until you have actually involved the operator in shaping the spec. Do not author the whole spec solo and surface it only at the gate — bring the operator in *now*, while the frame is still soft:

- Surface the open framing questions and the consequential choices to the operator with `darkrun_question` (a decision) or `darkrun_direction` (a direction to steer), and fold their answers into the spec.
- When the spec genuinely reflects that collaboration, call **`darkrun_elaborate_seal`** for this station — that clears the hold and the next tick advances to Review.

If you advance without involving the operator, the station stays in Spec; a stalled, non-collaborative Spec escalates to the operator rather than slipping past them. (`dark` mode pre-elaborates once up front and doesn't gate here.)


## Done when

The spec names the risk, lists Units with testable completion criteria and dependencies, marks what's out of scope, the operator has been involved and `darkrun_elaborate_seal` is called, and it's written to the station's spec artifact. Then call `darkrun_tick`.

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