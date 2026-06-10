---
name: Frame the protocol-fidelity problem, user, value, and single success metric
unit_type: ''
status: in_progress
depends_on: []
worker: challenger
station: frame
branch: darkrun/darkrun-sim/units/frame/frame-protocol-problem
started_at: 2026-06-10T06:35:07.265893+00:00
iterations:
- worker: framer
  started_at: 2026-06-10T06:35:07.265893+00:00
  completed_at: 2026-06-10T06:35:07.265893+00:00
  result: advance
  note: 'Make pass verified frame.md''s Problem/User/Value/Success-metric sections against this unit''s criterion. Problem: the only test of darkrun''s no-agent-mechanics bet fakes the agent (run_to_seal stamps state directly, never reads the prompt). User: the engine developer hardening the protocol. Value: closes the one untested gap; protects real users from stranded runs; bonus replayable demo. Success metric is single + observable: a no-privileged-knowledge agent reaches RunAction::Sealed from emitted prompts alone, deadlock::check never fires, no empty prompt/handoff/nonce, walk persisted + replayed; green = protocol flowed, never "it compiles." For challenger: attack the metric for measurability holes (is "stranded" fully operationalized?), confirm Problem is evidenced not asserted, and check the User is concrete. Do NOT rewrite the locked artifact — it already passed value+feasibility review; only file a real defect if one exists.'
reviews:
  feasibility:
    at: 2026-06-09T23:10:04.256406+00:00
  value:
    at: 2026-06-09T23:09:26.862330+00:00
---

# Frame the protocol-fidelity problem, user, value, and single success metric
