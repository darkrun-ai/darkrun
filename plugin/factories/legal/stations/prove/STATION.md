---
name: prove
label: Review
description: "Review verifies the instrument against the terms and kills escaped defects."
kills: escaped-defects
explorers: [scenario]
workers: [verifier, breaker, triage]
reviewers: [evidence_reviewer]
checkpoint: external
locked_artifact: review.md
inputs: [matter.md, terms.md, structure.md, draft]
---

# Review

Review kills *escaped defects* — the issues that survive drafting and would only surface in dispute. Independent of the team that drafted, the factory reviews the instrument against the locked terms, runs it through the scenarios that matter (breach, termination, the counterparty acting badly), and records the evidence that each term holds. The checkpoint is **external**: the proof is counsel's recorded attestation, not a number.
