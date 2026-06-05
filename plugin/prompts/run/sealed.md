{% include "_shared/announcement.md" %}

# Sealed — `{{ run }}`

The run is **sealed**. Every station is locked, the run-level review passed, and reflections are recorded. There is no further work to dispatch.

## What this means

- All locked artifacts are final. Touching one now is **drift** and will reopen the run.
- The evidence trail — specs, audits, test results, reflections — stands as the record of what was built and why.

{% if branch_status == "ahead" or branch_status == "diverged" %}
## Origin may be behind — the work landed locally

The run's verified work merged onto the run's base branch **locally** (a real merge commit), but the default branch on the remote has **not** been pushed (`branch status: {{ branch_status }}`). darkrun is local-first — it does not force-push your default branch. **Tell the operator the work is complete locally and that pushing the base branch (and opening/merging a PR if that's the team's flow) is the remaining step**, so CI runs against the shipped code rather than stale origin. Don't assume "sealed" means "on origin."
{% endif %}

## What to do

Nothing further on this run. If new work is needed, start a new run rather than reopening this one. Report the seal to the operator with a short summary of what shipped{% if branch_status %} and the branch status (`{{ branch_status }}`){% endif %}.
