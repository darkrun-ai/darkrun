---
topic: deadlock-escalate-is-stranded-verdict
created_at: 2026-07-02T23:52:13.300691+00:00
updated_at: 2026-07-02T23:52:13.300691+00:00
---
`crates/darkrun-mcp/src/deadlock.rs` is the engine's cross-tick wheel-spin guard (the predecessor's HALT_THRESHOLD ported forward — this bug class is a scar, not a hypothetical). After 4 same-signature no-progress ticks, or a two-signature A↔B churn over ≥8 ticks, `run_tick` swaps the wedged action for `RunAction::Escalate { reason }`. External-await actions are exempt; per-run history lives in `.darkrun/<slug>/deadlock.json` and resets after STALE_AGE_SECS=3600, so a bounded sim run must complete inside the hour window. For any stranded-agent/protocol-fidelity test, `Escalate` is the machine-readable red verdict — key pass/fail off it instead of inventing new stall detection. Note its limit: it catches the ENGINE refusing to advance; only a zero-knowledge agent in the seat extends it to catch prompts that never taught the agent what to stamp.
