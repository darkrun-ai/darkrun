---
name: darkrun-checkpoint
description: Review and decide a Station's Checkpoint — approve to advance the Run, or request changes to route rework as drift
---

The Run is holding at a Station Checkpoint. Surface the Station's result and the Reviewers' findings to the user, then record the decision with `darkrun_checkpoint_decide`:

- `approved: true` — the Checkpoint passes, the artifact locks, the manager advances.
- `approved: false` with `feedback` — the manager holds the Station and routes the rework back through the feedback track.

Then `/darkrun:darkrun-pickup` to continue.
