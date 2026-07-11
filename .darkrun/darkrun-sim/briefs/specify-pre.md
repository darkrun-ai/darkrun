---
station: specify
phase: pre
created_at: 2026-07-11T17:11:53.791234+00:00
---
# Specify spec — reviewed and ready for the pre-execution gate

**What this station does:** turns the locked frame into an unambiguous, testable contract. One Unit (`author-spec`, opus, document-class) writes the locked artifact `specify/spec.md` carrying: >= 12 numbered acceptance criteria each with a literal check command and a build-phase assignment, the boundary contracts (provider trait, vendored NoopHosting, the wasm-safe fixture schema in darkrun-core, the crate module map after partition), required behavior for every verified engine edge (rejected scripted move, exhausted script, dark-mode FeedbackQuestion, render_prompt None, the 3600s deadlock-history reset, double run_start, post-Sealed ticks), and the CI gate shapes (regenerate-and-diff on the NORMALIZED projection, plus the inverted red-on-Escalate negative scenario).

**Discovery findings folded in (contract + edge_case explorers, both grounded file:line):**
- The existing `crates/darkrun-sim` is a prompt-wording linter, not the simulator: its harness calls plain `run_tick` (the network-reaching path the frame forbids), decides moves by pattern-matching `TickResult.action` (the exact privileged-knowledge shape the frame condemns), and drives solo mode, not dark.
- The `verifier_nonce` is minted from wall-clock time and printed literally into every Manufacture prompt — raw-byte fixture diffing can never be green in CI.
- `TickResult`/`RunAction` are Serialize-only and darkrun-mcp is not wasm-clean, so the site can never round-trip engine types; `action-log.jsonl` and `events.jsonl` are not 1:1 (run-created and station-dropped events have no action-log counterpart).
- No CI job builds web/site for wasm32 today; Phase 3's gate needs a defined home.
All three durable facts are recorded in project knowledge (`existing-darkrun-sim-crate-is-a-prompt-linter`, `fixture-determinism-traps`, `wasm-boundary-for-fixture-types`).

**Operator decisions folded in (2026-07-11, this session):** (1) partition the existing crate — linter modules stay as a distinct fidelity check, the frame-compliant world/provider/transcript modules arrive alongside, harness rebuilt onto the locked seams; (2) the fixture schema lives in darkrun-core (the established wasm-clean home); (3) fixture determinism by normalization at projection — volatile fields stripped when emitting, CI diffs projections, no engine changes.

**Next:** your pre-execution gate. Approving dispatches the `author-spec` Unit through the specify workers (spec_writer → adversary → tightener), with reviewers testability + completeness at Audit.
