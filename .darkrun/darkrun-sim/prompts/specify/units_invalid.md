
> **Run** `darkrun-sim` · **Station** `specify` · **Phase** `units_invalid`

> Eliminates: _ambiguity_


# Malformed decomposition — `specify`

The manager refuses to manufacture against a broken decomposition. Station `specify` has Units that don't hold together, and the line stops here until they do — a bad DAG caught now is cheap; caught after a wave of parallel workers built on it, it isn't.

**Problem:** `station_inputs_dropped`



**Dropped inputs — carried into `specify` but consumed by no Unit:**


- `frame.md`



## What to do


Each artifact above is part of the run's distillation that station `specify` **carries forward** — but no Unit in this decomposition lists it as an `input`, so the station would rebuild from scratch and lose what upstream already settled. This is exactly what the cross-station contract exists to prevent. For each dropped artifact, **wire it into the `input`s of the Unit(s) that should rest on it** (`darkrun_unit_update`) — a spec, a design, prior code the work must honour rather than reinvent. If a Unit genuinely doesn't need a given input, that's fine *as long as some sibling Unit consumes it*; the requirement is collective, not per-Unit. If the station truly should not carry an artifact at all, that's a station-definition `inputs_waived` change, not a per-run drop.


## Done when

Every Unit in `specify` has a valid slug, every `depends_on` resolves to a real Unit, and the dependency graph is acyclic. Then call `darkrun_tick` — the manager re-validates and, once clean, releases the first wave.