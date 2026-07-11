
> **Run** `darkrun-sim` · **Station** `frame` · **Phase** `manufacture`

> Eliminates: _wrong-thing_


# Manufacture — `frame`

This is the build floor. You run the **Pass loop** — _Plan → Make → Challenge → Resolve_ — over the wave-ready Units. The current beat is **framer**, on model **sonnet**.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`frame.md`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `context`, `value`


**Workers** (3): `framer` → `challenger` → `distiller`


**Reviewers** (2): `value`, `feasibility`


## This wave


Dispatch the **framer** beat in parallel across these wave-ready Units:

- `author-frame`




## Each Unit's spec — the contract the beat works against

The subagent you dispatch for a Unit gets **no context beyond what you hand it**. Pass the Unit's spec below into its dispatch verbatim — the completion criteria with their verify commands, the declared paths, and the scope boundary are the contract the beat is judged against.

### `author-frame` — Author frame.md — the locked frame for darkrun-sim


- **outputs:** `frame/frame.md`


- **quality gates:** artifact-nonempty — `test -s .darkrun/darkrun-sim/frame/frame.md` · citations-resolve — `sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/frame/frame.md | sort -u | wc -l); test "$n" -ge 8 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/frame/frame.md | sort -u | xargs -I{} test -e {}'`


# Unit: author-frame

## Goal

Write `frame.md` — the frame station's locked artifact for Run `darkrun-sim` — at `.darkrun/darkrun-sim/frame/frame.md` (relative to the worktree root you execute in). darkrun-sim is a **protocol-fidelity simulator**: it puts a deliberately dumb, zero-privileged-knowledge agent in the driver's seat of the real darkrun engine to prove the engine's rendered prompts alone are followable. This document is the scope boundary every downstream station (specify, shape, build, prove, harden) inherits; ambiguity here becomes wrong work there.

## Facts to build from (verified 2026-07-02 against this workspace; do not re-litigate, do cite)

**The gap.** The e2e suite drives the REAL engine but with privileged knowledge: `run_to_seal` in `crates/darkrun-e2e/tests/common/mod.rs` loops `run_tick`, matches only the structured `TickResult.action` variant, and stamps exactly what the cursor checks (`elaborate_seal`, `run_review_stamp`, direct unit completion). It never reads `TickResult.prompt`. So today's green proves the cursor terminates deterministically — not that a real agent, given only the rendered prompt, would know what to do next.

**The red signal.** `crates/darkrun-mcp/src/deadlock.rs` swaps a wedged action for `RunAction::Escalate` after 4 same-signature no-progress ticks (or an A↔B churn over ≥8 ticks); external-await actions exempt; history resets after 3600s. A stranded zero-knowledge agent triggers it within ~5 ticks. `Escalate` IS the sim's machine-readable red verdict — the sim invents no stall detection of its own.

**The consumption seams (the ONLY coupling surface).** `darkrun-mcp` is a lib crate; drive it in-process exactly as e2e does: `StateStore::new(tempdir)` → `run_start(...)` → loop `run_tick_with_hosting(store, slug, &no-op Hosting)` — never plain `run_tick`, which resolves `ApiHosting` and can touch the network. The sim acts on `.prompt` only, never `.action`. For a Claude-Code-modeled agent the raw rendered prompt is byte-identical to production (`darkrun_harness::adapt_instructions` is the identity for the Claude Code cap set). Every tick already persists the rendered prompt to `.darkrun/<slug>/prompts/<scope>/<tag>.md` plus `action-log.jsonl` and `events.jsonl` — the ready-made transcript substrate. The prompt corpus (`plugin/prompts/`) contains NO machine-parseable dispatch markup; subagent dispatch is prose the agent follows itself, so the sim builds no dispatch-block parser. No git repo is required for protocol fidelity (tick git ops are best-effort no-ops on a bare tempdir).

**The site substrate.** `web/site` is a client-side Dioxus wasm SPA. Record/replay-without-live-engine is its existing pattern: `web/site/src/pages/preview.rs` renders fixture payloads with an explicit no-live-feed banner; `/browse` reads a repo's committed `.darkrun/` tree and re-derives state client-side via darkrun-core (wasm-capable). The replay player is a new Route variant modeled on `/preview`, built from shared darkrun-ui components. `web/app` is the separate live-relay app and is excluded.

**Operator decisions (locked 2026-07-02, via darkrun_question sessions q-01/q-02):**
1. **Dark-mode spine first; scripted operator-sim later.** v1 fixtures drive `dark`-mode runs (pre-elaborated, linear, no per-station collaboration hold). A scripted operator-sim (answers questions, decides checkpoints, seals specs) is a deferred later phase; solo/team-mode gate-surfacing coverage arrives with it. Re-entry trigger: spine + replay player merged and green in CI.
2. **Scripted provider only; real-model recorder deferred.** The provider is a pluggable trait from day one, and the scripted (deterministic, no-LLM) provider — today's privileged e2e moves formalized behind the trait — is the only implementation this run ships. Picking a real dumb-model recorder (API or local) is deferred. Re-entry trigger: the replay page is live on the site with a committed scripted fixture.
3. **Placement:** new workspace crate `crates/darkrun-sim` consuming darkrun-mcp as a library (the darkrun-e2e pattern); replay player in `web/site`.
4. In-browser/wasm LLM execution is dead permanently: no wasm LLM target, zero LLM code in the repo, no live engine or model per website visitor — record locally, commit fixtures, replay statically.

## Required document structure

Author these sections, in this order, in darkrun factory vocabulary (Factory > Station > Unit > Pass; Run; Worker; Checkpoint; manager):

1. `# Frame: darkrun-sim — protocol-fidelity simulator` — two-paragraph overview: what it is, the one-sentence red-run definition.
2. `## The gap this closes` — the privileged-e2e story above, with citations.
3. `## Goal (load-bearing)` — protocol fidelity, not build quality. A red run means a zero-knowledge agent got stranded (engine `Escalate`) or a prompt was unfollowable. A green software-factory run means "protocol flowed," never "it compiles."
4. `## Locked decisions` — the four operator decisions above, each with its re-entry trigger where one exists.
5. `## Engine seams` — the consumption seams above, stated as the only permitted coupling surface.
6. `## Out of scope` — at minimum: build-quality assertions on produced artifacts; re-implementing engine mechanics (scheduling, pools, dispatch parsing) in the harness; live engine or model per website visitor; `web/app` integration; solo/team-mode gate simulation (deferred, trigger named); real-model provider selection (deferred, trigger named); in-browser LLM execution (dead).
7. `## Build order (dependency-sequenced)` — Phase 1: world + transcript spine (in-process engine over a scratch StateStore, scripted provider behind the provider trait, dark-mode runs, transcript as an agent-scoped event log projected from the persisted prompts + action-log + events). Phase 2: replay player in `web/site` (new Route, panels over the transcript, validated against a committed scripted fixture). Phase 3: site/CI consumption (committed fixture, CI regeneration, red-on-`Escalate` assertion). Each phase independently demoable. NO time estimates anywhere — sequence only.
8. `## Evidence` — the explorer-verified files this frame stands on, as a path list.

## Completion criteria (verify each from the worktree root before finishing)

1. Artifact exists and is substantive → `test -s .darkrun/darkrun-sim/frame/frame.md` exits 0.
2. All eight required sections present → `grep -c '^#' .darkrun/darkrun-sim/frame/frame.md` reports ≥ 8.
3. At least 8 distinct extension-bearing repo paths are cited AND every one resolves in the worktree → `sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/frame/frame.md | sort -u | wc -l); test "$n" -ge 8 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/frame/frame.md | sort -u | xargs -I{} test -e {}'` exits 0. Consequence: name planned-but-nonexistent artifacts (the new crate, the new route) as directories or prose (`crates/darkrun-sim`), never as file paths with extensions.
4. Predecessor brand vocabulary absent → `grep -ci haiku .darkrun/darkrun-sim/frame/frame.md` reports 0. Use factory vocabulary throughout; "the predecessor" is the only permitted way to reference prior art.
5. No hedging or placeholders: no `TBD`, `should`, `probably`, `might`, `etc.` as a scope boundary — every boundary names its checkable condition.

## Files touched

`.darkrun/darkrun-sim/frame/frame.md` only. This is a document-class station: touch no repo source files.

## Out of scope for this unit

No crate scaffolding, no site code, no CI changes, no edits to any other `.darkrun/` file, no re-opening of the operator's locked decisions.




## Each Unit has its own worktree — work in it

Every wave Unit is isolated on its own branch + worktree, forked off the station branch. Run that Unit's beat **inside its worktree** so its diff never tangles with another Unit's in-flight work; the manager lands each Unit back onto the station branch when it locks. Do **not** commit a Unit's work to the station branch yourself.

- `author-frame` → `/Users/jwaldrip/dev/src/github.com/jwaldrip/darkrun/.claude/worktrees/wiggly-gathering-spark/.darkrun/worktrees/darkrun-sim/units/frame/author-frame` (branch `darkrun/darkrun-sim/units/frame/author-frame`)





## The Pass loop — make → challenge → resolve

The Pass loop is adversarial on purpose: a single confident pass is exactly where LLM output is most often confidently wrong, so a second pass red-teams the first before anything locks.

- **make** — the worker produces the Unit's output against its completion criteria. Build the real thing, not a sketch.
- **challenge** — a second pass attacks what make produced: edge cases, missing handling, lazy assumptions. Assume the first pass was optimistic.
- **resolve** — reconcile make and challenge into a Unit that satisfies its completion criteria with the challenges answered.




**Quality-gate verifier nonce.** This dispatch carries a one-time verifier token: **`5b652c354ef96d24f5eb9eb70f98d11d99351f355fe517ab99b73c2a799487f5`**. When you record a quality gate with `darkrun_quality_gate_record`, pass it as `nonce`. The engine refuses a gate result without the matching token — so a gate is only ever recorded as part of a real verification dispatch, never self-certified. Run the gate's command for real, then record the actual outcome with this nonce.


Run **only the `framer` beat** this tick. When the beat finishes, **record it** with `darkrun_unit_iterate` — pass the `worker`, the `result` (`advance` or `reject`), and a `note`: on advance, what you did and what the next worker needs to know; on reject, why you bounced it (a reject without a reason is refused). That note becomes the next beat's handoff above. Then call `darkrun_tick`; the manager advances the loop or releases the next wave. A Unit is locked only after Resolve and its completion criteria pass.

A Unit gets a **bounded pass budget** — the manager escalates a Unit that can't converge within it to the operator rather than grinding forever. Don't paper over a stuck Unit to dodge the escalation; a Unit that needs more passes than the budget allows is a signal the spec, the scope, or the approach is wrong, and that's the operator's call to make.



## Done when

The `framer` beat is complete for every Unit in this wave and its output is recorded. Then call `darkrun_tick`.

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