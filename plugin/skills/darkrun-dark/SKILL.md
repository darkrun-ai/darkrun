---
name: darkrun-dark
description: Run a Run lights-out in dark mode — pre-elaborate the work up front, then advance Station to Station without stopping for review, pausing only on external/await gates and ambiguity
---

Drive the Run hands-off in **dark mode**: pre-elaborate the work up front, then call `darkrun_advance` in a loop and do exactly what each returned action says — the lifecycle advances on its own without stopping for review. The manager owns gate promotion — don't flip gates by hand.

Stop and surface to the user on: `external`/`await` gates, elicitation (design-direction / visual picks), scope explosions, the final delivery Checkpoint, and any error or ambiguity that can't be inferred from the Run's goals. Never guess.

No active Run? Create one with `/darkrun:darkrun-new` first.
