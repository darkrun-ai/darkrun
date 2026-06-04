---
name: legal
description: The legal factory — takes a matter from intake to an executed instrument through six risk-eliminating stations, ordered by the cost of catching a defect late.
category: legal
default_model: sonnet
fix_workers: [redrafter, reconciler, verifier]
reviewers: [matter-auditor, compliance-auditor]
reflections: [process, quality]
surfaces: []
---

# Legal Factory

The legal factory delivers an **executed, enforceable instrument** from a raw
matter. It is the proof that darkrun's spine is domain-agnostic: the same six
stations, the same phase machine, the same checkpoints — but every station wears
legal vocabulary and the "proof" is a human attestation, not a benchmark.

It changes nothing in the engine. There is no `legal` code path. The factory is
pure orientation: role rosters, risk classes, locked artifacts, and the domain
labels the operator sees over the fixed positions.

## The six stations, in legal dress

| Position | Label | Risk it kills | Locked artifact | Checkpoint |
|---|---|---|---|---|
| **Frame** | Intake | taking the *wrong matter* | `matter.md` | ask |
| **Specify** | Position | *ambiguous terms* | `terms.md` | ask |
| **Shape** | Structure | the *wrong instrument* | `structure.md` | ask |
| **Build** | Draft | *drafting defects* | `draft` | ask |
| **Prove** | Review | *escaped defects* | `review.md` | external |
| **Harden** | Execute | *unenforceable in the wild* | `execution.md` | external |

The order is the same cost-of-late-discovery logic software uses: a misjudged
matter at Intake is cheap to fix and catastrophic to discover at Execute; a
drafting defect caught at Review is a redline, the same defect caught after
signature is a dispute.

## No measured surface — proof is attestation

Software classifies a delivery surface at Shape so Prove can measure it (a
headless browser, a benchmark). Legal has no such surface: it declares **none**.
The evidence a legal matter produces is **human attestation** — counsel's signed
review and the client's execution. That is why Review and Execute gate
**external**: the proof is a recorded approval from outside the run, not a number
the engine computes. The model holds without a new engine seam.

## fix-workers

When a checkpoint routes rework back, the fix-workers (Redrafter, Reconciler,
Verifier) take the repair — a clause redline, a reconciliation against the locked
terms, a re-verification — without re-running the whole station.

## Run-level review and reflection

Two whole-matter auditors judge the executed result end-to-end after the last
station closes: the **Matter Auditor** (does the instrument actually serve the
matter as framed?) and the **Compliance Auditor** (does it hold against the
governing law and the client's obligations?). Both gate. Two reflections —
**Process** and **Quality** — look back over the finished matter and write down
what would make the next one sharper. They never block.
