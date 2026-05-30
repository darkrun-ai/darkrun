---
name: darkrun-scaffold
description: Scaffold custom darkrun artifacts — Factories, Stations, Workers, and Reviewers — as editable templates under .darkrun/
---

Ask the user which artifact (Factory / Station / Worker / Reviewer) and its name, then call `darkrun_scaffold` and follow what it returns — it writes the editable template under `.darkrun/` and wires it into its parent. Point the user at the file(s) to fill in, then `/darkrun:darkrun-factories` to confirm it shows up in the registry.
