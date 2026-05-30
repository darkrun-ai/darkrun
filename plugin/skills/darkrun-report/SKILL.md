---
name: darkrun-report
description: Submit feedback or a bug report about darkrun itself — synthesize the user's experience into a structured, actionable report
---

Synthesize the user's feedback into a clear, structured summary (what they were doing, what went wrong, the expected behavior) — not their words verbatim — confirm it with them, then call `darkrun_report` with the synthesized `message` (and `contact_email` / `name` only if offered).

This is for feedback about darkrun the tool. To file rework inside a Run, use `darkrun_feedback_create` instead.
