---
name: harden
label: Execute
description: "Execute finalizes the instrument for the real world and kills unenforceability."
kills: unenforceable-in-the-wild
explorers: [jurisdiction]
workers: [finalizer, redteam_counsel, executor]
reviewers: [readiness_reviewer]
checkpoint: external
locked_artifact: execution.md
inputs: [matter.md, terms.md, structure.md, draft, review.md]
---

# Execute

Execute kills *unenforceable in the wild* — the instrument that is sound on paper but fails at execution: a missing signature block, an unmet formality, a jurisdiction whose execution rules were ignored, no plan for what happens after signing. The factory finalizes the instrument for the real world — execution formalities, the signing and delivery plan, the post-execution obligations — and gates **external** on the client's actual execution.
