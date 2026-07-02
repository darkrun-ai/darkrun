
> **Run** `darkrun-sim` · **Station** `frame` · **Phase** `spec`

> Eliminates: _wrong-thing_


# Spec — `frame`

You are opening station **frame**. Its job is to eliminate a whole class of risk: **wrong-thing**. Nothing downstream is allowed to proceed until that risk is named and bounded here.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`frame.md`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `context`, `value`


**Workers** (3): `framer` → `challenger` → `distiller`


**Reviewers** (2): `value`, `feasibility`


Spec runs **elaboration and discovery in tandem** — they are NOT two sequential
steps. The moment the station opens, kick off both at once: dispatch the explorers
in parallel *while* you frame the problem. They sharpen each other. Only once both
have landed do you decompose.



## elaborate — frame the problem (concurrently with discovery)

State plainly what this station must achieve to kill **wrong-thing**: the intent, the inputs it inherits from upstream, and the boundary of what is explicitly *out of scope* so later phases don't drift into it. This is the frame the explorers work against — but do NOT wait on a finished frame to start them; the frame and the exploration are written in parallel and inform each other.

## discover — run the explorers in parallel (concurrently with elaboration)

Dispatch **all** explorers (`context`, `value`) **at once, in parallel** — one subagent each, fanned out concurrently, never one-after-another. Explorers don't build — they surface unknowns, constraints, prior art, and traps. They run alongside your framing; neither blocks the other.



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