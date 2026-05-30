---
name: prove
description: Prove the software satisfies every spec criterion — independently of the people who built it.
explorers: [scenario, regression]
workers: [verifier, breaker, triage]
reviewers: [evidence, coverage]
checkpoint: ask
locked_artifact: proof.md
inputs: [spec.md, code]
---

# Prove

Prove establishes that the software actually does what Specify said it must —
**independently of Build**. Build's own tests prove the code does what Build
*thinks* it should; Prove proves it does what the *spec* requires, with fresh
eyes and adversarial intent. This independence is the point: the people who wrote
the code are the worst judges of whether it is correct.

## Risk class eliminated

*Escaped defects.* The code passed Build's tests, but those tests share the
builder's blind spots. A defect the builder never imagined sails through. Prove
is the independent check that catches what Build could not see.

## What this station produces

- **Criterion-to-evidence mapping** — every acceptance criterion from `spec.md`
  paired with the concrete evidence (a test run, a trace, a measurement) that it holds.
- **Break attempts** — adversarial exploration of the inputs and sequences Build
  never tested, with the failures they surface.
- **Triaged findings** — each discovered defect classified by severity and routed.

## The pass-loop

- **Verifier** walks the spec criterion by criterion and gathers independent evidence that each one holds — not trusting Build's tests.
- **Breaker** attacks the software with the edge cases, adversarial inputs, and sequences from the spec that Build likely under-tested.
- **Triage** classifies every failure by severity and routes blockers back to Build as drift; non-blockers become tracked feedback.

## Locked artifact

`proof.md` — a criterion-by-criterion proof: for each spec criterion, the
evidence that it is satisfied. This is the durable record that the software meets
its contract.

## Checkpoint

**ask.** A human confirms the evidence is sufficient before the software is
declared proven (and may route **external** for sign-off on regulated or
high-stakes work).
