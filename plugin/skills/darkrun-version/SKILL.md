---
name: darkrun-version
description: Show the running darkrun engine version, plugin version, build kind (compiled bundle vs dev source), runtime, and entry point — for triaging behavior that doesn't match the docs
---

Call `darkrun_version_info` and report what it returns — engine version, plugin version, build kind (compiled vs dev), runtime, and entry point. Mention any pending update. When the build kind is `dev`, note that engine-source edits are live only after a rebuild (`cargo build`) — there's no hot reload.
