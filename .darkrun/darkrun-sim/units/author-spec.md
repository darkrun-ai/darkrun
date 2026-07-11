---
name: Author spec.md — testable acceptance criteria, contracts, and edge cases for darkrun-sim
unit_type: doc
status: pending
depends_on: []
worker: ''
model: opus
station: specify
inputs:
- frame.md
outputs:
- specify/spec.md
reviews:
  completeness:
    at: 2026-07-11T17:21:33.548961+00:00
  testability:
    at: 2026-07-11T17:21:43.629013+00:00
quality_gates:
- name: artifact-nonempty
  command: test -s .darkrun/darkrun-sim/specify/spec.md
- name: citations-resolve
  command: sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | wc -l); test "$n" -ge 10 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | xargs -I{} test -e {}'
- name: acceptance-criteria-floor
  command: sh -c 'test $(grep -cE "^### AC-[0-9]+" .darkrun/darkrun-sim/specify/spec.md) -ge 12'
---

# Unit: author-spec

## Goal

Write `spec.md` — the specify station's locked artifact for Run `darkrun-sim` — at `.darkrun/darkrun-sim/specify/spec.md` (relative to the worktree root you execute in). It turns the locked frame (`.darkrun/darkrun-sim/frame/frame.md` — read it first; never contradict it) into an unambiguous contract: testable acceptance criteria, boundary contracts, and edge-case behavior definitions. This document becomes Prove's rubric — anything vague here is unprovable later. Every criterion must have a yes/no answer an independent party could check without asking the author what they meant.

## Operator decisions (locked 2026-07-11; fold in verbatim, do not re-litigate)

1. **Partition the existing crate.** `crates/darkrun-sim` today is a prompt-wording linter (SimAgent text classifier in `crates/darkrun-sim/src/agent.rs`, tool-name registry in `crates/darkrun-sim/src/tool_registry.rs`, corpus tests in `crates/darkrun-sim/tests/followability.rs`) whose harness violates the frame's seams (plain `run_tick` at `crates/darkrun-sim/src/harness.rs` lines 21 and 69; `.action`-driven walk at lines 150-196; solo mode at `crates/darkrun-sim/src/scenarios.rs` line 53). The spec must partition: the linter modules stay as a distinct, still-valuable fidelity check; the frame-compliant simulator arrives as new modules (world, provider, transcript) in the same crate; `harness.rs` is rebuilt onto the locked seams (`run_tick_with_hosting`, prompt-only, `Mode::Dark`). Extending the current harness in place is forbidden.
2. **Fixture schema lives in darkrun-core.** `web/site` can never depend on `darkrun-mcp` (unconditional nix/tokio/ureq/rmcp deps; not wasm-clean), and `TickResult`/`RunAction`/`Position` are Serialize-only. The replay fixture type is a new wasm-safe serde struct set in `crates/darkrun-core` (its only native dep is cfg(unix)-gated), serialized by the sim and deserialized by the site.
3. **Determinism by normalization at projection.** The transcript projector strips/canonicalizes volatile fields when emitting the fixture; CI diffs projected fixtures, never raw prompt bytes. Known volatile sources (all verified): the `verifier_nonce` minted from `Utc::now()` (`crates/darkrun-mcp/src/position.rs` mint_verifier_nonce) and printed literally by `plugin/prompts/phases/manufacture.md`; every `at:` rfc3339 timestamp in `action-log.jsonl`/`events.jsonl`; iteration timestamps in unit frontmatter.

Plus the frame's four locked decisions (dark-mode spine first; scripted provider only behind a pluggable trait; placement `crates/darkrun-sim` + `web/site`; in-browser LLM dead) with their re-entry triggers — restate, never weaken.

## Verified facts to specify against (from the station's explorers; cite, do not re-derive)

- Consumption surface: `StateStore::new(repo_root)` (`crates/darkrun-core/src/state.rs`), `run_start(store, slug, factory, title, mode, size)` and `run_tick_with_hosting<H: Hosting>` (`crates/darkrun-mcp/src/position.rs`), `TickResult { run, position, action, prompt }`. `Hosting` needs exactly three non-defaulted methods for a no-op impl: `available()`, `open_draft()`, `merge_state()` (`crates/darkrun-mcp/src/hosting.rs`); darkrun-mcp exports no public stub, so the sim vendors its own `NoopHosting`.
- Transcript substrate: `StateStore::write_prompt` persists the CURRENT prompt per station/tag (overwritten, not append-only) to `.darkrun/<slug>/prompts/<scope>/<tag>.md`; `action-log.jsonl` lines are `{at, track, action, station}`; `events.jsonl` lines are `{at, event, run, ...fields}`; the two streams are NOT 1:1 (`darkrun.run.created` and `darkrun.station.dropped` have no action-log counterpart) — the spec must state the projector's explicit merge/ordering rule and that the fixture carries its own `schema_version`.
- Engine edges needing DEFINED behavior (each verified in-repo): a scripted move the engine rejects (`elaborate_seal` → InvalidInput when station not active; `checkpoint_decide` → NoActiveStation); a script exhausted before `Sealed`; `RunAction::FeedbackQuestion` firing in dark mode (mode-independent); `render_prompt` returning None for an unmapped action tag (never fires today — assert corpus-wide); the deadlock guard's `STALE_AGE_SECS=3600` history reset (`crates/darkrun-mcp/src/deadlock.rs`) silently zeroing the no-progress counter on wall-clock-slow runs; `save_wip` and `enforce_unit_scope` both no-op on the bare TempDir world (verified); repeated post-`Sealed` ticks return `Sealed` forever without escalating.
- Red verdict: `RunAction::Escalate` (deadlock guard: 4 same-signature no-progress ticks, or A-B churn over >=8 of 10, exemptions per `is_exempt`). The spec must ALSO classify the non-Escalate failure shapes above as red-verdict material with named transcript markers, or explicitly assign them to harness-failure (panic) status — no shape may be left undefined.
- Replay surface: new Route variant in `web/site/src/route.rs` AND an entry in its `all_paths()` static-render list (a route absent from `all_paths()` compiles and renders in dev while never reaching the static wasm export), modeled on `/preview` (`web/site/src/pages/preview.rs`: hardcoded payload, no fetch, explicit no-live-feed banner), composed from darkrun-ui prelude components (`StationStrip`, `StationPipeline` in `crates/darkrun-ui/src/components/pipeline.rs`, `UnitGraph` in `crates/darkrun-ui/src/graph/view.rs`). Fixture embedding precedent: `include_str!` (`web/site/src/content.rs`). CI gap: no workflow builds web/site for wasm32 today (`.github/workflows/ci.yml` wasm job scopes `-p darkrun-app` only) — the spec must state the Phase 3 CI gate shape (new job or extended job, regenerate-project-diff, plus the inverted red-on-Escalate negative scenario).

## Required document structure (in this order)

1. `# Spec: darkrun-sim — protocol-fidelity simulator` — one-paragraph contract overview naming what this spec makes checkable.
2. `## Acceptance criteria` — numbered `### AC-1` … `### AC-n` (n >= 12), each: one yes/no claim, the literal command or observable that checks it, and which build phase (1/2/3 from the frame) it lands in. Cover at minimum: world construction (bare TempDir, dark mode, no network), provider trait shape + scripted impl's prompt-blindness, prompt-only seam (zero `.action` reads in the driving path — checkable via grep over the new modules), transcript projection (three streams, merge rule, schema_version, normalization), fixture determinism (regenerate-twice byte-equal), red-on-Escalate detection, defined behavior for every non-Escalate failure shape, linter partition intact (existing followability tests still pass), replay route registered in BOTH the `web/site` Route enum and its `all_paths()` static-render list (checkable via grep on `web/site/src/route.rs`), replay page composed from the darkrun-ui prelude components (`StationStrip`, `StationPipeline`, `UnitGraph`) with an explicit no-live-feed banner and zero network fetch in the new page (checkable via grep for `gloo` / `remote::` / `fetch` over the new page module), replay route renders the committed fixture, CI gates (regenerate-diff green job + negative Escalate scenario), wasm boundary honored (web/site gains no darkrun-mcp edge — checkable via `cargo tree`).
3. `## Contracts` — six contracts: (1) the provider trait (name, method signatures, what the scripted impl may and may not condition on); (2) `NoopHosting` (the three methods and their return values); (3) the darkrun-core fixture schema (struct names, fields, `schema_version` field, normalization rules enumerated); (4) the crate module map after partition (which files exist, which are rebuilt, which are untouched); (5) the fixture file's committed path and embedding mechanism; (6) the replay Route contract (variant name, URL pattern, its `all_paths()` entry, the darkrun-ui components it composes, the no-live-feed banner, and the no-fetch rule).
4. `## Edge cases` — every edge listed above with its REQUIRED behavior (not options): rejected move, exhausted script, dark-mode FeedbackQuestion, render_prompt None, STALE_AGE reset, double run_start, post-Sealed ticks, empty prompts dir for unreached phases, fixture referencing content the site build no longer embeds.
5. `## Out of scope` — inherit the frame's exclusions; add: no engine-code changes for determinism (decision 3), no second sim crate (decision 1), no web/app work, no real-model provider.
6. `## Evidence` — path list of every file this spec's claims stand on.

## Completion criteria (verify each from the worktree root before finishing)

1. Artifact exists and is substantive → `test -s .darkrun/darkrun-sim/specify/spec.md` exits 0.
2. Exactly the six required H1/H2 headings, in the required order, with no extra H1/H2 headings → `sh -c 't=$(mktemp); grep -E "^##? " .darkrun/darkrun-sim/specify/spec.md > "$t"; printf "%s\n" "# Spec: darkrun-sim — protocol-fidelity simulator" "## Acceptance criteria" "## Contracts" "## Edge cases" "## Out of scope" "## Evidence" | diff - "$t"; r=$?; rm -f "$t"; exit $r'` exits 0 (AC subheadings are H3 `###` and are deliberately excluded by the `^##? ` pattern).
3. At least 12 acceptance criteria → `grep -cE '^### AC-[0-9]+' .darkrun/darkrun-sim/specify/spec.md` reports >= 12.
4. At least 10 distinct extension-bearing repo paths cited AND every one resolves → `sh -c 'n=$(grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | wc -l); test "$n" -ge 10 && grep -oE "(crates|web|plugin|desktop)/[A-Za-z0-9_./-]+[.](rs|md|toml|mjs|json)" .darkrun/darkrun-sim/specify/spec.md | sort -u | xargs -I{} test -e {}'` exits 0. Name planned-but-nonexistent artifacts (new modules, the fixture file, the new route) as directories or prose, never as extension-bearing paths.
5. Predecessor brand vocabulary absent → `grep -ci haiku .darkrun/darkrun-sim/specify/spec.md` reports 0.
6. No hedging or placeholder tokens; the banned token set is exactly {TBD, should, probably, might, seems, etc} → `grep -niE '\bTBD\b|\bshould\b|\bprobably\b|\bmight\b|\bseems\b|\betc\b' .darkrun/darkrun-sim/specify/spec.md` outputs nothing (exits 1). No time estimates → `grep -niE '\bweek(s)?\b|\bday(s)?\b|\bhour(s)?\b|\bmonth(s)?\b|timeline|deadline' .darkrun/darkrun-sim/specify/spec.md` outputs nothing (exits 1). Sequence only.

## Files touched

`.darkrun/darkrun-sim/specify/spec.md` only. Document-class station: touch no repo source files.

## Out of scope for this unit

No crate scaffolding, no site code, no CI changes, no edits to any other `.darkrun/` file, no re-opening of the frame's or today's operator decisions.
