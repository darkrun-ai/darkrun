---
name: darkrun-start
description: Start a new darkrun Run — describe what you want to accomplish and the factory manager scaffolds a right-sized lifecycle for it
---

# Start a Run

A **Run** moves through a **Factory**'s **Stations**. Your job here is to capture intent cleanly and
let the manager scaffold and right-size it.

## Process

1. **Prelaborate.** If the description is short (under ~2 sentences) or vague, ask 2–3 targeted
   questions via `AskUserQuestion` (scope, desired outcome, constraints). Fold answers into a
   richer 3–5 sentence description. If the user referenced a file (spec, screenshot, path), read it
   and synthesize its *substance* into the description — never store the path.

2. **Quick context scan.** 2–3 tool calls to understand the stack and project purpose. This informs
   factory selection.

3. **Pick the factory.** Call `darkrun_factory_list` to see available factories with descriptions,
   then pass the 2–4 best-fit `factory_candidates` so the picker is pre-narrowed (software is the
   one shipped today).

4. **Create the Run.** Call `darkrun_run_start` with:
   - `title` — crisp 3–8 word human-readable name (≤80 chars, no trailing period). Not a truncated
     description.
   - `description` — the prelaborated 2–5 sentence narrative (scope, motivation, constraints).
   - `slug` — kebab-case id (≤40 chars), usually from the title.
   - `context` — key decisions/constraints from the conversation.
   - `factory_candidates` — the shortlist from step 3.

5. **Right-sizing is automatic.** The manager assesses size at run start and collapses Stations for
   small work (a one-liner runs Build → Prove with auto-Checkpoints). Don't pre-pick a "mode" —
   just describe the work accurately and let the manager size it.

6. **Hand off.** Once created, drive the loop with `/darkrun:darkrun-pickup` — the manager tells you
   the next action.
