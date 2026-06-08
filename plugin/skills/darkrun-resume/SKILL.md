---
name: darkrun-resume
description: Advance a darkrun Run — the factory manager returns the next concrete action across Stations, Workers, and Checkpoints
---

Call `darkrun_advance` (omit the arg to resume the active Run, or pass a Run slug/id). Do exactly what the returned action's rendered prompt says — it carries the Station, Worker, and Checkpoint instructions verbatim. Then call `darkrun_advance` again, and loop until the manager reports the Run complete.

For an `ask` Checkpoint, record the decision with `/darkrun:darkrun-checkpoint`. Inspect state any time with `/darkrun:darkrun-inspect`.
