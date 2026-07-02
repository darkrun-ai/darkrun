---
topic: subagent-dispatch-is-prose
created_at: 2026-07-02T23:52:17.043321+00:00
updated_at: 2026-07-02T23:52:17.043321+00:00
---
The prompt corpus (`plugin/prompts/`, embedded by darkrun-prompts with a project `.darkrun/prompts/` → plugin-root → embedded override cascade) contains NO machine-parseable subagent dispatch markup — no `<subagent>`/`<dispatch>`/relay blocks anywhere. Manufacture prompts instruct in prose: dispatch the worker beat in parallel across wave-ready Units and pass each Unit's spec verbatim into the dispatch; the agent spawns subagents itself. Pool/scheduling behavior is prompt prose, not structured blocks — so any sim or tooling must NOT build a dispatch-block parser; followability of the prose IS the surface under test. Action→template mapping is `darkrun_prompts::template_key_for_action` (23 keys); a bare tempdir with no overrides resolves deterministically to the embedded corpus.
