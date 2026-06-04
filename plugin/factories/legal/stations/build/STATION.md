---
name: build
label: Draft
description: "Draft writes the instrument clause by clause and kills drafting defects."
kills: drafting-defects
explorers: [clause_library]
workers: [clause_drafter, redline_challenger, draft_reconciler]
reviewers: [drafting_reviewer]
checkpoint: ask
locked_artifact: draft
inputs: [matter.md, terms.md, structure.md]
---

# Draft

Draft kills *drafting defects* — the clause that says the wrong thing, the cross-reference that points nowhere, the defined term used before it is defined. With the structure locked, the factory drafts the actual instrument clause by clause, each clause challenged by an internal redline before it is reconciled. This is the legal equivalent of writing code with tests: the redline is the test.
