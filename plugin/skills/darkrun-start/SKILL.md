---
name: darkrun-start
description: Start a new darkrun Run — describe what you want to accomplish and the factory manager scaffolds a right-sized lifecycle for it
---

Capture the intent cleanly, then let the manager scaffold and right-size the lifecycle. If the request is vague, prelaborate it into a crisp description first (ask via `AskUserQuestion`). Call `darkrun_factory_list` if the factory isn't obvious, then `darkrun_run_start { slug, title, factory }`. Drive the loop with `/darkrun:darkrun-pickup`.
