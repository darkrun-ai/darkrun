---
name: Author frame.md — the locked frame for darkrun-sim
unit_type: doc
status: in_progress
depends_on: []
worker: challenger
model: opus
station: frame
outputs:
- frame/frame.md
branch: darkrun/darkrun-sim/units/frame/author-frame
started_at: 2026-07-11T05:10:13.062545+00:00
iterations:
- worker: framer
  started_at: 2026-07-11T05:10:13.062545+00:00
  completed_at: 2026-07-11T05:10:13.062545+00:00
  result: advance
  note: 'Authored .darkrun/darkrun-sim/frame/frame.md in the unit worktree (commit 497d606 on darkrun/darkrun-sim/units/frame/author-frame). All 8 required sections present in order; 12 distinct extension-bearing repo paths cited, every one verified to resolve; zero predecessor-brand hits; no hedging/placeholder tokens (grep for TBD/should/probably/might/etc./seems/looks like returned nothing). Both declared quality gates (artifact-nonempty, citations-resolve) ran for real and recorded pass. The frame fixes red strictly to engine RunAction::Escalate (crates/darkrun-mcp/src/deadlock.rs) plus unfollowable-prompt failures; locks the four operator decisions with re-entry triggers; names the darkrun-mcp in-process seam (StateStore -> run_start -> run_tick_with_hosting, prompt-only, never .action) as the sole coupling surface; and sequences three phases (world+transcript spine, web/site replay player, site/CI consumption) with no time estimates. Challenger: attack citation accuracy against actual file contents (framer skimmed load-bearing files but grounding depth varies), the completeness of Out of scope against the unit spec''s minimum list, and whether every locked decision carries its re-entry trigger where the spec names one.'
reviews:
  feasibility:
    at: 2026-07-03T01:24:16.145824+00:00
  value:
    at: 2026-07-03T01:23:42.340439+00:00
quality_gates:
- name: artifact-nonempty
  command: test -s .darkrun/darkrun-sim/frame/frame.md
- name: citations-resolve
  command: sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/frame/frame.md | sort -u | wc -l); test "$n" -ge 8 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/frame/frame.md | sort -u | xargs -I{} test -e {}'
gate_results:
- name: artifact-nonempty
  status: pass
  at: 2026-07-11T05:09:49.804723+00:00
  attempts: 1
  detail: test -s .darkrun/darkrun-sim/frame/frame.md exited 0 in the unit worktree
- name: citations-resolve
  status: pass
  at: 2026-07-11T05:09:59.770137+00:00
  attempts: 1
  detail: 12 distinct extension-bearing repo paths cited; count >= 8 and every path resolves in the unit worktree (command exited 0)
---

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
