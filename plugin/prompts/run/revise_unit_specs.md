{% include "_shared/announcement.md" %}

# Revise unit specs — `{{ station }}`

The operator has rolled Units back for spec revision. Their criteria were wrong, incomplete, or overtaken by something learned downstream — so the manager re-opens their specs before any more is built on top of them. Rework is cheapest at the spec, most expensive after manufacture; this catches it at the spec.

{% if units %}
**Units to revise:**
{% for u in units %}
- `{{ u }}`
{% endfor %}
{% endif %}

## What to do

1. **Read the revision intent.** Why were these Units rolled back? Check their bodies and any feedback attached — the operator flagged them for a reason.
2. **Rewrite the spec, not the code.** For each Unit, correct its completion criteria, inputs/outputs, and dependencies so they're testable and right. This is a Specify-level act: make the spec something Prove could later check.
3. **Re-decompose if needed.** If a Unit is wrong because it's too big or mis-scoped, split or merge it — revision can change the shape of the work, not just its wording.
4. **Clear the `revise` flag** on each Unit once its spec is sound (`darkrun_unit_update`), and reset it to pending so manufacture can pick it up fresh.

## Done when

Every rolled-back Unit has a corrected, testable spec and its `revise` flag is cleared. Then call `darkrun_tick` — the manager re-validates the decomposition and resumes the station.
