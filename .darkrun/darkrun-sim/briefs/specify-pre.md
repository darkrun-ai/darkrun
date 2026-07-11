---
station: specify
phase: pre
created_at: 2026-07-11T17:22:25.108620+00:00
---
# Specify spec — reviewed and ready for the pre-execution gate

**What this station does:** turns the locked frame into an unambiguous, testable contract. One Unit (`author-spec`, opus, document-class) writes the locked artifact `specify/spec.md` carrying: >= 12 numbered acceptance criteria each with a literal check command and a build-phase assignment; six closed contracts (provider trait, vendored NoopHosting, the wasm-safe fixture schema in darkrun-core, the crate module map after partition, the fixture's committed path + embedding mechanism, and the replay Route contract with its all_paths() registration, darkrun-ui component composition, no-live-feed banner, and no-fetch rule); required behavior for every verified engine edge; and the CI gate shapes (regenerate-and-diff on the NORMALIZED projection, plus the inverted red-on-Escalate negative scenario).

**Discovery findings folded in (contract + edge_case explorers, both grounded file:line):** the existing `crates/darkrun-sim` is a prompt-wording linter whose harness violates the frame's seams (plain `run_tick`, `.action`-driven walk, solo mode); the `verifier_nonce` is wall-clock-minted and printed into every Manufacture prompt, so raw-byte fixture diffing can never be green; engine types are Serialize-only and darkrun-mcp is not wasm-clean; action-log and events journals are not 1:1; no CI job builds web/site for wasm32. All recorded as project knowledge (`existing-darkrun-sim-crate-is-a-prompt-linter`, `fixture-determinism-traps`, `wasm-boundary-for-fixture-types`).

**Operator decisions folded in (2026-07-11, this session):** (1) partition the existing crate — linter stays, frame-compliant world/provider/transcript modules arrive alongside, harness rebuilt onto the locked seams; (2) fixture schema lives in darkrun-core; (3) fixture determinism by normalization at projection, no engine changes.

**Review record — both lenses signed off after one fix round:**
- **testability — filed fb-04 (high), then stamped after the fix.** Finding: completion criterion 2's `grep -c '^#' >= 6` was inflatable by the 12+ AC-n H3 subheadings (verified empirically: a spec missing two required sections scored 16); criterion 6's ban list ended in an open "etc." with no command. Fix verified by dry-run: the new exact ordered-headings diff check passes the good file and fails missing-section, out-of-order, and extra-heading variants; the closed six-token hedge scan and the time-estimate scan each hit and clear correctly.
- **completeness — filed fb-05 (high), then stamped after the fix.** Finding: the Phase 2 replay route/UI contract (all_paths() registration, darkrun-ui component reuse, banner, no-fetch) had no explicit demand — a builder-guesses hole, since a route absent from all_paths() renders in dev but never reaches the static wasm export Phase 3's CI gate validates. Fix verified by diff: the AC minimum list gained the four demands with named check methods, and Contracts is now a closed six-item list whose item 6 is the Route contract. All other coverage rows re-checked intact; no contradictions.

Both findings resolved as addressed (fb-04, fb-05) with the remediation recorded on each. Review stamps on record: testability + completeness (kind: review) on `author-spec`.

**Next:** your pre-execution gate. Approving dispatches the `author-spec` Unit through the specify workers (spec_writer → adversary → tightener), with reviewers testability + completeness again at Audit.
