{% include "_shared/announcement.md" %}

# Malformed decomposition — `{{ station }}`

The manager refuses to manufacture against a broken decomposition. Station `{{ station }}` has Units that don't hold together, and the line stops here until they do — a bad DAG caught now is cheap; caught after a wave of parallel workers built on it, it isn't.

**Problem:** `{{ problem }}`

{% if units %}
{% if problem == "station_inputs_dropped" %}
**Dropped inputs — carried into `{{ station }}` but consumed by no Unit:**
{% else %}
**Offending Units:**
{% endif %}
{% for u in units %}
- `{{ u }}`
{% endfor %}
{% endif %}

## What to do

{% if problem == "invalid_naming" %}
A Unit slug is empty or not kebab-case (lowercase, hyphen-separated, no spaces). Rename the offending Units to valid slugs — `darkrun_unit_update` the name, or recreate them cleanly. Slugs are identity; downstream deps reference them, so get them right before anything depends on them.
{% elif problem == "unresolved_deps" %}
A Unit declares a `depends_on` that names no Unit in this run — a dangling edge. Either **create the missing Unit** (the dependency is real and was never decomposed) or **drop the stale edge** from the offending Unit's `depends_on`. Don't manufacture against a dependency that doesn't exist.
{% elif problem == "dependency_cycle" %}
The Units above form a **dependency cycle** — they depend on each other in a loop, so no wave can ever be ready. Break the cycle: find the edge that doesn't truly need to exist and remove it, or merge the mutually-dependent Units into one. A DAG has no cycles by definition; restore that.
{% elif problem == "missing_output" %}
Each Unit above locked but **never produced a declared output** — the artifact it promised in `outputs:` is not on disk. A station cannot advance to Audit on a promise. For each Unit, either **produce the missing artifact** at its declared path, or **correct the `outputs:` declaration** if the path was wrong. Don't audit work that didn't ship its output.
{% elif problem == "gates_unmet" %}
Each Unit above locked but has a **declared quality gate that isn't satisfied** — a gate with no recorded result, a `fail`, or an unresolved `env_blocked`. A station cannot reach Audit on unverified work. For each Unit: **run its gate commands** (you have a shell), then record the outcome with `darkrun_quality_gate_record` — `pass` when it's green, `fail` (fix it, then re-run), or `env_blocked` when a dependency genuinely can't run locally (a repeatedly-blocked gate auto-defers to CI so it can't wedge the run). Don't mark a gate passed you didn't actually run.
{% elif problem == "input_not_a_path" %}
Each Unit above declares an **`input` that names another Unit instead of a file path**. Inputs are *premises* — artifact paths the Unit was built against, which the engine witnesses for drift. A bare Unit slug can't be witnessed. If the Unit must run *after* another, that's a `depends_on` edge, not an `input`. Move the slug to `depends_on`, and declare the real upstream artifact path (e.g. `frame/frame.md`) as the `input`.
{% elif problem == "station_inputs_dropped" %}
Each artifact above is part of the run's distillation that station `{{ station }}` **carries forward** — but no Unit in this decomposition lists it as an `input`, so the station would rebuild from scratch and lose what upstream already settled. This is exactly what the cross-station contract exists to prevent. For each dropped artifact, **wire it into the `input`s of the Unit(s) that should rest on it** (`darkrun_unit_update`) — a spec, a design, prior code the work must honour rather than reinvent. If a Unit genuinely doesn't need a given input, that's fine *as long as some sibling Unit consumes it*; the requirement is collective, not per-Unit. If the station truly should not carry an artifact at all, that's a station-definition `inputs_waived` change, not a per-run drop.
{% else %}
The decomposition is structurally invalid. Inspect the offending Units, correct their naming / dependencies, and re-validate.
{% endif %}

## Done when

Every Unit in `{{ station }}` has a valid slug, every `depends_on` resolves to a real Unit, and the dependency graph is acyclic. Then call `darkrun_tick` — the manager re-validates and, once clean, releases the first wave.
