
> **Run** `darkrun-sim` · **Station** `build` · **Phase** `manufacture`

> Eliminates: _implementation-defects_


# Manufacture — `build`

This is the build floor. You run the **Pass loop** — _Plan → Make → Challenge → Resolve_ — over the wave-ready Units. The current beat is **test_author**, on model **sonnet**.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`code`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `reuse`, `integration_point`


**Workers** (4): `test_author` → `builder` → `self_reviewer` → `reconciler`


**Reviewers** (2): `correctness`, `maintainability`


## This wave


Dispatch the **test_author** beat in parallel across these wave-ready Units:

- `ci-gates`




## Each Unit's spec — the contract the beat works against

The subagent you dispatch for a Unit gets **no context beyond what you hand it**. Pass the Unit's spec below into its dispatch verbatim — the completion criteria with their verify commands, the declared paths, and the scope boundary are the contract the beat is judged against.

### `ci-gates` — sim-fidelity CI job — fixture regenerate-diff, red-on-Escalate, darkrun-site wasm clippy

- **inputs:** `crates/darkrun-sim/fixtures/dark-core.json`


- **outputs:** `.github/workflows/ci.yml`


- **quality gates:** workflow-parses — `python3 -c "import yaml;d=yaml.safe_load(open('.github/workflows/ci.yml'));assert 'sim-fidelity' in d['jobs']"` · job-steps-present — `sh -c 'grep -q "committed_fixture_matches_regeneration" .github/workflows/ci.yml && grep -q "escalate_scenario_is_detected_red" .github/workflows/ci.yml && grep -q "clippy -p darkrun-site --target wasm32-unknown-unknown" .github/workflows/ci.yml'` · local-dry-run — `sh -c 'a=$(cargo test -p darkrun-sim committed_fixture_matches_regeneration 2>&1) && echo "$a" | grep -qE "[1-9][0-9]* passed" && b=$(cargo test -p darkrun-sim escalate_scenario_is_detected_red 2>&1) && echo "$b" | grep -qE "[1-9][0-9]* passed"'`


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




## Each Unit has its own worktree — work in it

Every wave Unit is isolated on its own branch + worktree, forked off the station branch. Run that Unit's beat **inside its worktree** so its diff never tangles with another Unit's in-flight work; the manager lands each Unit back onto the station branch when it locks. Do **not** commit a Unit's work to the station branch yourself.

- `ci-gates` → `/Users/jwaldrip/dev/src/github.com/jwaldrip/darkrun/.claude/worktrees/wiggly-gathering-spark/.darkrun/worktrees/darkrun-sim/units/build/ci-gates` (branch `darkrun/darkrun-sim/units/build/ci-gates`)





## The Pass loop — make → challenge → resolve

The Pass loop is adversarial on purpose: a single confident pass is exactly where LLM output is most often confidently wrong, so a second pass red-teams the first before anything locks.

- **make** — the worker produces the Unit's output against its completion criteria. Build the real thing, not a sketch.
- **challenge** — a second pass attacks what make produced: edge cases, missing handling, lazy assumptions. Assume the first pass was optimistic.
- **resolve** — reconcile make and challenge into a Unit that satisfies its completion criteria with the challenges answered.


**Reject routing.** Workers carry a pass-loop role: `self_reviewer` = verify, . A `build` worker produces and repairs; a `verify` worker only judges; a `plan` worker only designs. When a beat **rejects**, bounce back to the **nearest preceding `build` worker** (pass it as `next_worker` to `darkrun_unit_iterate`) — skip `verify`/`plan` beats on the way back, since they can't fix. An `advance` rolls forward to the next worker in order.



**Quality-gate verifier nonce.** This dispatch carries a one-time verifier token: **`affa8c653e282c3d71173554591ff479c8e28a574a322901d86efb62a18c486d`**. When you record a quality gate with `darkrun_quality_gate_record`, pass it as `nonce`. The engine refuses a gate result without the matching token — so a gate is only ever recorded as part of a real verification dispatch, never self-certified. Run the gate's command for real, then record the actual outcome with this nonce.


Run **only the `test_author` beat** this tick. When the beat finishes, **record it** with `darkrun_unit_iterate` — pass the `worker`, the `result` (`advance` or `reject`), and a `note`: on advance, what you did and what the next worker needs to know; on reject, why you bounced it (a reject without a reason is refused). That note becomes the next beat's handoff above. Then call `darkrun_tick`; the manager advances the loop or releases the next wave. A Unit is locked only after Resolve and its completion criteria pass.

A Unit gets a **bounded pass budget** — the manager escalates a Unit that can't converge within it to the operator rather than grinding forever. Don't paper over a stuck Unit to dodge the escalation; a Unit that needs more passes than the budget allows is a signal the spec, the scope, or the approach is wrong, and that's the operator's call to make.



## Done when

The `test_author` beat is complete for every Unit in this wave and its output is recorded. Then call `darkrun_tick`.

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