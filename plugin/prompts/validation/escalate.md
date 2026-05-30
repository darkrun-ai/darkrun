{% include "_shared/announcement.md" %}

# Escalation — `{{ station }}`

A Unit's Pass loop has burned through its iteration budget without locking. The manager **stops auto-looping** here on purpose: another identical pass is unlikely to converge, and grinding the same Unit forever just spends tokens to stay stuck.

**Why:** {{ reason }}

## What to do

This is a judgement call, and it's the operator's to make — not another silent retry.

1. **Diagnose the stall.** Read the Unit's passes. Why isn't Resolve converging? Common causes: the completion criteria are contradictory or untestable, the Unit is too big to land in one piece, a dependency is wrong, or the approach is a dead end.
2. **Pick a real way out**, then put it to the operator with `darkrun_question`:
   - **Re-spec** — the criteria are wrong; roll the Unit back for spec revision (`revise`).
   - **Split** — the Unit is too large; decompose it into smaller Units that *can* land.
   - **Change approach** — the strategy is wrong; redirect and reset the pass count.
   - **Accept** — the output is good enough and the loop is chasing diminishing returns; lock it.
3. **Apply the decision** and reset the Unit so the loop can move again.

## Done when

The stalled Unit has a path forward the operator chose — re-spec'd, split, redirected, or accepted — and its pass count no longer trips the budget. Then call `run_next`.
