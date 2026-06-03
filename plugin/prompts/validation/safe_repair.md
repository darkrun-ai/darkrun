{% include "_shared/announcement.md" %}

# Safe repair — `{{ run }}`

The run's persisted state is internally inconsistent. The manager won't make forward progress on top of a corrupt cursor — it would only compound the damage — so it routes a **guarded repair** first.

**Inconsistency:** {{ reason }}

## What to do

Repair conservatively. The goal is to restore a coherent state, not to redesign anything.

1. **Confirm the inconsistency.** Read the cited Unit / station against the run's factory definition. Is the Unit's `station` a typo, a renamed station, or a genuine orphan?
2. **Make the minimal correction:**
   - Wrong/renamed station on a Unit → `darkrun_unit_update` it to a station the factory actually defines.
   - The Unit belongs to no station in this factory → move it to the right one, or remove it if it's a stray.
   - `state.json` and the Units disagree → reconcile to what the Units on disk actually say (the documents are the source of truth).
3. **Don't fabricate.** If you can't tell what the correct state is, surface it to the operator with `darkrun_question` rather than guessing.

## Done when

Every Unit references a station its factory defines and the persisted state is coherent again. Then call `darkrun_tick` — the manager re-derives the cursor and resumes the normal phase walk.
