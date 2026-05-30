---
name: darkrun-checkpoint
description: Review and decide a Station's Checkpoint — approve to advance the Run, or request changes to route rework as drift
---

# Decide a Checkpoint

A **Checkpoint** is the gate at the end of a Station. It stops defects from propagating downstream
and locks the Station's durable artifact once passed.

## Process

1. **Understand the gate.** The Run is holding at a Checkpoint of kind `ask` (or `external`). Use
   `/darkrun:darkrun-show` to see the Station, its Units, and the Reviewers' findings.

2. **Surface the decision to the user.** Summarize what the Station produced and what the Reviewers
   flagged. For `ask`, ask the user to approve or request changes (use `AskUserQuestion` when a
   clear choice helps).

3. **Record it with `darkrun_checkpoint_decide`:**
   - `decision: "approve"` — the Checkpoint passes, the artifact locks, and the manager advances to
     the next Station.
   - `decision: "request_changes"` with `notes` — the manager routes the work back as **drift** to
     the right Station (Build for defects, Shape for structural issues) and re-runs the relevant
     Workers.

4. **Continue.** After deciding, call `/darkrun:darkrun-pickup` to advance.
