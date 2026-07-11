
> **Run** `darkrun-sim` · **Station** `specify` · **Phase** `review`

> Eliminates: _ambiguity_


# Review — `specify` spec

Before a single Unit is manufactured, the spec gets reviewed. A bad spec that reaches manufacture is the most expensive failure in the line — kill it here, cheaply.


**Contract**

- Do exactly the work this action describes — no more, no less. Don't skip ahead to a later phase.
- Treat the locked artifact (`spec.md`) as the source of truth. Read it before you act; never silently rewrite a locked decision.
- Every claim you make must be backed by something you actually ran, read, or wrote. No assumed results.
- Be specific and committed. **No placeholders** — a `TBD`, `similar to …`, `add error handling`, `etc.`, or `…` is a hole, not a decision; name the actual, checkable condition. **No hedging** — when you report work done, use a verb of completed action (`added`, `implemented`, `fixed`), never `should`, `seems`, `probably`, `might`, or `looks like`. Hedging is the tell of unfinished work.
- When the action is finished, record your output where the station expects it, then call `darkrun_tick` again for the next instruction. The manager — not you — decides what comes next.



**Explorers** (2): `contract`, `edge_case`


**Workers** (3): `spec_writer` → `adversary` → `tightener`


**Reviewers** (2): `testability`, `completeness`


Review walks three beats, in order: **spec → adversarial → brief**. The operator's decision is a *separate* step the manager surfaces as a gate once this work lands — you do not ask for it inline here.

## 1. spec — verify against the spec

Read the spec produced in the previous phase against its own intent. Before any adversary touches it, confirm it is internally coherent:

- Does it actually name and bound **ambiguity**, or does it leave a hole?
- Does every Unit carry testable completion criteria and explicit dependencies?
- Is the out-of-scope boundary stated, so later phases can't drift into it?

## 2. adversarial — adversarial reviewer pass

Dispatch each reviewer (`testability`, `completeness`) against the spec. Each owns one lens — let them be ruthless inside it:



- Does the spec actually eliminate **ambiguity**, or does it only look like it does?
- Are the completion criteria testable, or are they wishful?
- Are the Units genuinely independent, or will they collide during manufacture?
- Is anything load-bearing left unstated?

**Dispatch the reviewers in parallel** — one subagent each, fanned out concurrently, not one-after-another. They share no state and each owns a different lens, so they run independently. When a reviewer is satisfied it records its own sign-off with **`darkrun_review_stamp`** (`kind: review`, its `role`) — that stamps only its role and returns without advancing the run, so parallel reviewers never contend on the tick. A reviewer that finds a real problem files it with `darkrun_feedback_create` (origin `adversarial_review`) **instead of** stamping. You call `darkrun_tick` once, after every reviewer has returned.

**A reviewer reviews — it does not redesign.** Each reviewer MUST NOT propose new requirements outside the spec's stated intent, MUST NOT substitute its own approach or relitigate a settled tradeoff, and MUST NOT block on stylistic preference. It finds where the spec fails *its own* goal and files exactly that — nothing more.


### Units under review

- `author-spec`



If a reviewer blocks, fix the spec and re-review — do not advance a spec a reviewer rejected.

## 3. brief — the review summary

Produce a tight brief of the review: which lenses signed off, which filed concerns, and how each concern was resolved (or why it was deferred). This is the record manufacture inherits — it should make the spec's verdict obvious without re-reading every reviewer's notes.

Persist it: call `darkrun_brief_record` with `slug: darkrun-sim`, `station: specify`, `phase: pre`, and the brief as `body`. This is the pre-execution brief the operator's review gate surfaces — a durable artifact, not inline prose.

## Done when

Every reviewer has signed off or filed addressable concerns and the brief is recorded with `darkrun_brief_record`. Then call `darkrun_tick` — the manager opens the operator's pre-execution gate on the next tick. Do not surface the decision inline; the gate does that.