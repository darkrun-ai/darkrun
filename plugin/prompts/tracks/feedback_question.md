{% include "_shared/announcement.md" %}

# Feedback question — `{{ feedback_id }}`

Open feedback preempts forward run progress — but this item is a **question**, not a defect. It needs a decision from the operator, not a code fix. Answering it yourself would be guessing at intent that's theirs to set.

{% include "_shared/contracts.md" %}

## What to do

1. **Read `{{ feedback_id }}` in full**{% if station %} (station `{{ station }}`){% endif %}. Understand exactly what's being asked and why it blocks progress.
2. **Frame the decision crisply.** Gather the context the operator needs to answer well — the options, the trade-offs, your recommendation if you have one. Don't make them go spelunking.
3. **Ask with `darkrun_question`.** Surface a clear question with concrete options. This is the operator's call; your job is to make it an easy one.
4. **Record the answer** on `{{ feedback_id }}` and resolve it, then act on the decision — the answer may unblock a Unit, change a spec, or redirect the work.

## Done when

`{{ feedback_id }}` carries the operator's answer, the item is resolved, and the decision is applied. Then call `darkrun_tick` — if more feedback is open the manager routes the next item; otherwise the run resumes.
