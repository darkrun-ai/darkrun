---
name: darkrun-gate-review
description: Pre-Checkpoint code review with a multi-agent fix loop — compute the diff, dispatch Reviewers, and process findings before the Checkpoint locks
---

Call `darkrun_gate_review` for the current Station and follow the returned instructions verbatim — they spell out which Reviewers to spawn and how to process findings. Drive the fix loop until the Reviewers come back clean or the user accepts the remainder, then decide the Checkpoint with `/darkrun:darkrun-checkpoint`.
