---
name: sim-fidelity CI job — fixture regenerate-diff, red-on-Escalate, darkrun-site wasm clippy
unit_type: feature
status: completed
depends_on:
- sim-spine
- replay-route
worker: reconciler
model: sonnet
station: build
inputs:
- crates/darkrun-sim/fixtures/dark-core.json
outputs:
- .github/workflows/ci.yml
- Cargo.lock
- web/site/src/pages/mod.rs
- web/site/src/pages/replay.rs
- web/site/src/route.rs
- web/site/tests/routes.rs
branch: darkrun/darkrun-sim/units/build/ci-gates
started_at: 2026-07-19T22:55:25.592362+00:00
completed_at: 2026-07-19T23:01:33.758938+00:00
iterations:
- worker: test_author
  started_at: 2026-07-19T22:55:25.592362+00:00
  completed_at: 2026-07-19T22:55:25.592362+00:00
  result: advance
  note: 'Delivered the sim-fidelity job in .github/workflows/ci.yml (commit 55bbc33, 1 file, 37 insertions): 6 steps in order — checkout, dtolnay/rust-toolchain with wasm32-unknown-unknown target, Swatinem/rust-cache, cargo test -p darkrun-sim committed_fixture_matches_regeneration, cargo test -p darkrun-sim escalate_scenario_is_detected_red, cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings. Mirrors the wasm-app job''s action idioms, no new trigger config, zero continue-on-error. All five completion criteria verified for real: yaml parse assertion exit 0 (6 steps), grep count 3, soft-fail count 0, both sim tests 1 passed each locally plus site wasm clippy exit 0, and git show --stat HEAD confirms exactly one file in the commit. All three declared quality gates re-run verbatim by the manager and recorded pass with the station nonce. Worktree note for later beats: Cargo.lock drifted unstaged because the committed lock still pinned 0.12.0 against Cargo.toml 0.12.1 from prior branch commits — running the required cargo commands synced it; not authored, not committed, will be restored at reconcile. Builder: the YAML is declarative with no crate code — re-verify the criteria and confirm step ORDER matches the spec (tests before clippy) and the toolchain/cache action versions match the file''s existing pins exactly.'
- worker: builder
  started_at: 2026-07-19T22:55:47.037822+00:00
  completed_at: 2026-07-19T22:55:47.037822+00:00
  result: advance
  note: 'Re-verified 55bbc33 structurally rather than rebuilding: parsed the workflow with yaml and asserted the sim-fidelity run steps execute in spec order (committed_fixture_matches_regeneration, then escalate_scenario_is_detected_red, then the darkrun-site wasm clippy), all three uses pins (actions/checkout@v7, dtolnay/rust-toolchain@29eef33, Swatinem/rust-cache@e18b497) are reused verbatim from the file''s existing jobs (set-membership check, PIN MATCH True), and git diff HEAD~1 HEAD on ci.yml shows zero removed lines — the commit is pure addition, so the rust/cross-check/wasm-app jobs are untouched by construction. Gates already recorded green with the nonce. Nothing to change; no builder commit. Self-reviewer: attack the YAML semantics the greps cannot see — job-level defaults or missing working-directory, whether the cargo test name filters can go vacuous (0 tests matched still exits 0), runner choice vs the sibling jobs, cache key collisions, and whether anything in the job could soft-pass on a broken fixture.'
- worker: self_reviewer
  started_at: 2026-07-19T23:01:08.443032+00:00
  completed_at: 2026-07-19T23:01:08.443032+00:00
  result: advance
  note: 'Adversarial verify-only pass: ADVANCE, all seven attack items PASS empirically. Key evidence: toolchain step carries both targets wasm32-unknown-unknown and components clippy identical to wasm-app; cache key sim-fidelity is distinct per the sibling pattern; both cargo-test filters resolve to exactly one real test each via -- --list (transcript::fixture::committed_fixture_matches_regeneration, world::tests::escalate_scenario_is_detected_red) so neither CI step can go vacuous; the divergence assertions are genuine (transcript.rs:298 assert_eq regenerated vs committed fixture; world.rs:394 assert matches Escalated from an AlwaysAdvance stranding provider); 55bbc33 is +37/-0 with the on: block unchanged; yaml parses and actionlint (on PATH) exits clean; the darkrun-site wasm clippy step is the real net-new coverage while the two sim tests are belt-and-suspenders over the rust job''s workspace test. Two cosmetic prose nits in the job comments (says FixtureOutcome where the test asserts the pre-projection WorldOutcome; overstates shared detection path) — non-blocking, comment-only. Reconciler: restore the unstaged Cargo.lock and drift-witness ride-alongs, confirm clean tree, push the unit branch.'
- worker: reconciler
  started_at: 2026-07-19T23:01:27.993387+00:00
  completed_at: 2026-07-19T23:01:27.993387+00:00
  result: advance
  note: 'Terminal sweep, nothing to fold: self-reviewer ADVANCE carried only two comment-prose nits (job comment names FixtureOutcome where the test asserts WorldOutcome; slightly overstated shared-detection wording) — cosmetic, left as-is rather than reopening a green unit for comment copy. Worktree verified clean at 1b415ab (the engine''s checkpoint commit atop 55bbc33 absorbed the Cargo.lock/drift-witness ride-alongs), branch darkrun/darkrun-sim/units/build/ci-gates pushed (up-to-date). Deliverable: the sim-fidelity job in .github/workflows/ci.yml, +37/-0, all three declared gates recorded pass with the station nonce, actionlint clean. Unit complete — this closes the build station''s fourth and final unit.'
reviews:
  correctness:
    at: 2026-07-12T06:11:51.306714+00:00
  maintainability:
    at: 2026-07-12T06:11:54.404623+00:00
quality_gates:
- name: workflow-parses
  command: python3 -c "import yaml;d=yaml.safe_load(open('.github/workflows/ci.yml'));assert 'sim-fidelity' in d['jobs']"
- name: job-steps-present
  command: sh -c 'grep -q "committed_fixture_matches_regeneration" .github/workflows/ci.yml && grep -q "escalate_scenario_is_detected_red" .github/workflows/ci.yml && grep -q "clippy -p darkrun-site --target wasm32-unknown-unknown" .github/workflows/ci.yml'
- name: local-dry-run
  command: sh -c 'a=$(cargo test -p darkrun-sim committed_fixture_matches_regeneration 2>&1) && echo "$a" | grep -qE "[1-9][0-9]* passed" && b=$(cargo test -p darkrun-sim escalate_scenario_is_detected_red 2>&1) && echo "$b" | grep -qE "[1-9][0-9]* passed"'
gate_results:
- name: workflow-parses
  status: pass
  at: 2026-07-19T22:54:58.284411+00:00
  attempts: 1
  detail: 'python3 yaml.safe_load assertion ran verbatim in the unit worktree at commit 55bbc33, exit 0: ''sim-fidelity'' present in jobs (6 steps)'
- name: job-steps-present
  status: pass
  at: 2026-07-19T22:55:05.325070+00:00
  attempts: 1
  detail: 'Compound grep sh ran verbatim in the unit worktree at 55bbc33, exit 0: all three step commands present in .github/workflows/ci.yml (committed_fixture_matches_regeneration, escalate_scenario_is_detected_red, clippy -p darkrun-site --target wasm32-unknown-unknown)'
- name: local-dry-run
  status: pass
  at: 2026-07-19T22:55:11.912559+00:00
  attempts: 1
  detail: 'Compound sh ran verbatim in the unit worktree at 55bbc33, exit 0: committed_fixture_matches_regeneration 1 passed, escalate_scenario_is_detected_red 1 passed (both matched the [1-9][0-9]* passed pattern)'
---

# Unit: ci-gates

## Goal

Add the **`sim-fidelity`** job to `.github/workflows/ci.yml` — the operator chose a new dedicated job over extending `wasm-app` (decision 2026-07-11). It wires AC-16's three gates into CI: the fixture regenerate-and-diff (via the `committed_fixture_matches_regeneration` test the sim-spine unit ships), the inverted red-on-Escalate negative scenario (`escalate_scenario_is_detected_red`), and the `darkrun-site` wasm32 clippy step that closes the pre-existing CI blind spot (today `wasm-app` scopes `-p darkrun-app` only; nothing gates `darkrun-site`'s wasm buildability). THE CONTRACT IS THE LOCKED SPEC: read `.darkrun/darkrun-sim/specify/spec.md` AC-15/AC-16 first.

## What to build

One new job in `.github/workflows/ci.yml`, mirroring the existing `wasm-app` job's structure (runner, checkout, toolchain via the same `dtolnay/rust-toolchain` action + `targets: wasm32-unknown-unknown`, the same Swatinem/rust-cache pattern the other jobs use — copy the repo's established step idioms, do not invent new action versions):

```yaml
sim-fidelity:
  # protocol-fidelity gates: the committed fixture must match regeneration,
  # the stranding scenario must go red, and the replay page must stay wasm-clean
```

Steps, in order: (1) checkout; (2) toolchain with wasm32 target; (3) cache; (4) `cargo test -p darkrun-sim committed_fixture_matches_regeneration` — fails when regeneration diverges from `crates/darkrun-sim/fixtures/dark-core.json`; (5) `cargo test -p darkrun-sim escalate_scenario_is_detected_red` — the negative scenario: a sim DESIGNED to strand must report `FixtureOutcome::Escalated`, so this test passing IS the red-detection proof (the inversion lives inside the test's assertion, the job just runs it); (6) `cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings`. The job triggers with the same `on:` conditions the `rust` job uses (no new trigger surface).

## Success / failure / edge paths

Success: the workflow parses, the job's three commands pass locally in the unit worktree (the sim-spine and replay-route units landed before this one — depends_on ordering guarantees their code is on the station branch this worktree forked from). Failure: any of the three commands failing locally means the SIBLING work is broken — bounce with a reject note rather than papering over. Edge: the job must not use `continue-on-error` anywhere (a soft-fail gate is not a gate — the rust-quality workflow's SARIF job is non-blocking by design; this one is blocking by design).

## Completion criteria (verify each from the unit worktree root)

1. Workflow parses and carries the job → `python3 -c "import yaml;d=yaml.safe_load(open('.github/workflows/ci.yml'));assert 'sim-fidelity' in d['jobs'] and len(d['jobs']['sim-fidelity']['steps'])>=6"` exits 0.
2. All three gate commands present verbatim → `grep -c 'committed_fixture_matches_regeneration\|escalate_scenario_is_detected_red\|clippy -p darkrun-site --target wasm32-unknown-unknown' .github/workflows/ci.yml` reports 3.
3. No soft-fail → `grep -A30 'sim-fidelity:' .github/workflows/ci.yml | grep -c 'continue-on-error'` reports 0.
4. The three commands pass locally: `cargo test -p darkrun-sim committed_fixture_matches_regeneration` exits 0; `cargo test -p darkrun-sim escalate_scenario_is_detected_red` exits 0; `cargo clippy -p darkrun-site --target wasm32-unknown-unknown -- -D warnings` exits 0.
5. Existing jobs untouched → `git diff HEAD~1 -- .github/workflows/ci.yml` (your commit) shows only additions under the new job key, zero modified lines in the `rust`, `cross-check`, or `wasm-app` jobs.

## Files touched

`.github/workflows/ci.yml` only.

## Out of scope

No edits to other workflows (deploy-web.yml already triggers on crates/** and web/** paths), no changes to any crate, no new GitHub Actions beyond the versions the file already pins, no trigger changes.
