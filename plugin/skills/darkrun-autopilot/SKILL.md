---
name: darkrun-autopilot
description: Run a Run's gates autonomously — promote ask Checkpoints to auto so the manager advances Station to Station without stopping, pausing only on external/await gates and ambiguity
---

Drive the Run hands-off: call `darkrun_tick` in a loop and do exactly what each returned action says, with `ask` Checkpoints auto-approved so the lifecycle advances on its own. The manager owns Checkpoint promotion — don't flip gates by hand.

Stop and surface to the user on: `external`/`await` gates, elicitation (design-direction / visual picks), scope explosions, the final delivery Checkpoint, and any error or ambiguity that can't be inferred from the Run's goals. Never guess.

No active Run? Create one with `/darkrun:darkrun-start` first.
