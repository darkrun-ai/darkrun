{% include "_shared/announcement.md" %}

# Resolve Merge Conflict — `{{ branch }}`

A land (or a downstream sync) left **genuine content conflicts** in-tree on `{{ branch }}`. The merge is **not** aborted — `MERGE_HEAD` is still set and the conflict markers are present, waiting on you. Merge resolution preempts everything: the run can't advance while a merge is half-applied.

{% include "_shared/contracts.md" %}

## What happened

Merging into `{{ branch }}` could not auto-resolve every path. Engine-owned `.darkrun/{{ run }}/…` state was already force-held to the target side; what remains is **real agent content** the two sides both touched.

While this merge is in progress the engine **suspends** its ownership / lifecycle / branch-enforcement write guards so you *can* edit the conflicted files directly — schema validation stays on, so a malformed resolution still fails loudly.

## Conflicted paths

{% for p in conflict_paths %}- `{{ p }}`
{% endfor %}

## What to do

1. **Open each conflicted path** and resolve the `<<<<<<<` / `=======` / `>>>>>>>` markers — keep the correct content from both sides.
2. **Stage** each resolved file (`git add`).
3. **Commit** the merge (`git commit --no-edit`) to finish it. Do **not** `git merge --abort` — that throws away the land.
4. Re-run `run_next`. The next tick re-derives this action until the merge is no longer in progress, then resumes the run.

## Done when

Every conflicted path is resolved and staged, the merge is committed (no `MERGE_HEAD`), and `run_next` advances past this action.
