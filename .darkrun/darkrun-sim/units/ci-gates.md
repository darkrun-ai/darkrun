---
name: sim-fidelity CI job — fixture regenerate-diff, red-on-Escalate, darkrun-site wasm clippy
unit_type: feature
status: pending
depends_on:
- sim-spine
- replay-route
worker: ''
model: sonnet
station: build
inputs:
- crates/darkrun-sim/fixtures/dark-core.json
outputs:
- .github/workflows/ci.yml
reviews:
  correctness:
    at: 2026-07-12T06:11:51.306714+00:00
quality_gates:
- name: workflow-parses
  command: python3 -c "import yaml;d=yaml.safe_load(open('.github/workflows/ci.yml'));assert 'sim-fidelity' in d['jobs']"
- name: job-steps-present
  command: sh -c 'grep -q "committed_fixture_matches_regeneration" .github/workflows/ci.yml && grep -q "escalate_scenario_is_detected_red" .github/workflows/ci.yml && grep -q "clippy -p darkrun-site --target wasm32-unknown-unknown" .github/workflows/ci.yml'
- name: local-dry-run
  command: sh -c 'a=$(cargo test -p darkrun-sim committed_fixture_matches_regeneration 2>&1) && echo "$a" | grep -qE "[1-9][0-9]* passed" && b=$(cargo test -p darkrun-sim escalate_scenario_is_detected_red 2>&1) && echo "$b" | grep -qE "[1-9][0-9]* passed"'
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
