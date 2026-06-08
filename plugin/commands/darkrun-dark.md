---
description: Run a Run lights-out in dark mode — pre-elaborate up front, then advance without stopping for review, pausing only on external/await gates and ambiguity
argument-hint: [run slug]
---

Run a darkrun Run in dark mode.

Set dark mode at the Run level (`darkrun_run_new { ..., mode: "dark" }` or on an existing Run): pre-elaborate the work up front, then drive `darkrun_advance { run: "$ARGUMENTS" }` in a loop without stopping for review. Pause and surface to the user on external/await gates, scope explosion, or ambiguity. See the `darkrun-dark` skill.
