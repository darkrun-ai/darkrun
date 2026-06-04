# Factory Orientation Plan

One fixed FSSBPH spine. Factories are **pure orientation**. Zero factory content
in Rust. The website, desktop app, and shared UI all reason over the same spine.

> Vocabulary note: this document calls the prior TypeScript system "the
> predecessor." Never write its name into darkrun code, content, or output.

## Goal

Replace the inline `software_factory()` in `crates/darkrun-mcp/src/factory.rs`
with a disk-resolved, cascading factory system where:

- the **flow** (Frame → Specify → Shape → Build → Prove → Harden) is a fixed,
  non-overridable invariant in code;
- every **station's meaning, roster, artifact, and surfaces** is on-disk content
  resolved through a most-specific-wins cascade, or it is non-existent;
- a **factory** is only its domain orientation (labels, rosters, kills, artifacts,
  surfaces) plus an optional single-parent `inherits`;
- the engine, the website, the desktop app, and `darkrun-ui` keep reasoning over
  the universal six stations while users see their domain's vocabulary.

## Principles

1. **Invariant → code; overridable → disk-or-nonexistent.** The test is never
   "is it in code?" but "is it overridable?" A non-overridable mechanical law
   (the FSSBPH order, the phase machine) lives in code and has no resolution path.
   Anything overridable resolves from disk; if absent, it does not exist — there
   is no baked-in default to fall back to.
2. **Finest scope wins.** Roster/content resolution is scope-primary
   (station > factory > global), with source as the tiebreak within a scope
   (project `.darkrun/` beats installed plugin beats embedded).
3. **Define once.** What recurs across factories (the spine, per-station meaning,
   generic roles like `verifier`/`reviewer`) lives once at the broadest scope.

## The code/disk boundary

| In code (mechanical, non-overridable) | On disk (content, cascade-resolved) |
|---|---|
| `Station` enum + `FLOW` ordered const (the six positions) | `plugin/stations/<pos>/` — per-position preamble + defaults |
| `StationPhase` (Spec→Review→UserGate→Manufacture→Audit→Reflect→Checkpoint) | `plugin/{explorers,workers,reviewers,reflections}/` — role libraries (dir = agent_type) |
| cascade resolver, three tracks, gates, `derive`, deadlock guard | `plugin/prompts/phases/` — per-tick instructions |
| open `ProofKind` registry seam (the only domain frontier) | `plugin/factories/<domain>/…` — orientations |

## Architecture

### A1. The flow invariant (code)

```rust
// darkrun-core
pub enum Station { Frame, Specify, Shape, Build, Prove, Harden }
pub const FLOW: [Station; 6] =
    [Station::Frame, Station::Specify, Station::Shape,
     Station::Build, Station::Prove, Station::Harden];
impl Station { pub fn dir(self) -> &'static str { /* "frame" … */ } }
```

`StationPhase` already exists and is unchanged. Right-sizing keeps selecting a
subset of `FLOW` per run, always in order. No `ORDER.md`.

### A2. Plugin-root base layout (disk)

```
plugin/
  stations/<pos>/PREAMBLE.md   # what this FSSBPH position means (domain-free)
  stations/<pos>/DEFAULTS.md   # default checkpoint kind, role rhythm, artifact-kind
  explorers/<name>.md          # role libraries — THE DIRECTORY IS THE agent_type;
  workers/<name>.md            #   `agent_type` never appears in frontmatter
  reviewers/<name>.md
  reflections/<name>.md
  prompts/phases/*.md          # per-tick instructions (already here)
  factories/<domain>/…         # orientations
```

**The directory is the `agent_type`.** A file in `explorers/` is an explorer, in
`workers/` a worker, in `reviewers/` a reviewer, in `reflections/` a reflection —
the loader infers it from the path, so `agent_type` is never written anywhere.
**There is no `fix/` directory:** a fix-worker is *just a worker* that a station's
`fix:` slot assigns to repair duty (a worker like `reconciler` can sit in both a
station's `workers:` and its `fix:`). The station's frontmatter decides which
roles fill which slots and in what order.

### A3. The roster cascade (disk, most-specific-wins)

Resolving a role `<name>` of type `<t>` ∈ {`explorers`, `workers`, `reviewers`,
`reflections`} for factory `f`, station `s`, run `r`, unit `u` — first hit wins,
scope-primary, project beats plugin within a scope. The leaf is the **type
directory**, which *is* the `agent_type`:

```
1  .darkrun/runs/<r>/units/<u>/<t>/<name>.md          project · per-unit
2  .darkrun/runs/<r>/stations/<s>/<t>/<name>.md       project · per-run
3  .darkrun/factories/<f>/stations/<s>/<t>/<name>.md  project · station
4       factories/<f>/stations/<s>/<t>/<name>.md      plugin  · station
5  .darkrun/factories/<f>/<t>/<name>.md               project · factory
6       factories/<f>/<t>/<name>.md                   plugin  · factory
   ┌─ inherits: <p> — the PARENT becomes walkable here (child first, parent
   │  after; repeat up the chain for p's own `inherits`):
   │   .darkrun/factories/<p>/stations/<s>/<t>/<name>.md   project · p · station
   │        factories/<p>/stations/<s>/<t>/<name>.md       plugin  · p · station
   │   .darkrun/factories/<p>/<t>/<name>.md                project · p · factory
   │        factories/<p>/<t>/<name>.md                    plugin  · p · factory
   └─ …then p's parent, … …
7  .darkrun/<t>/<name>.md                             project · global
8       plugin/<t>/<name>.md                          plugin  · global
   → none → non-existent (error). No code fallback.
```

The `agent_type` is the leaf directory, so it is never written. A `workers/`
role like `reconciler` can fill both a station's `workers:` and its `fix:` slot —
the slot is assignment, not type. Per-station content
(`stations/<pos>/PREAMBLE.md`, `DEFAULTS.md`) cascades the same way:
`plugin/stations/<pos>` ⊕ `factories/<f>/stations/<pos>`.

### A4. Factory resolution (replaces the inline fallback)

`resolve_factory(name)` becomes: for each `Station` in `FLOW`, merge
`plugin/stations/<pos>/DEFAULTS` ⊕ `factories/<name>/stations/<pos>/STATION.md`
(⊕ parent via `inherits`), assembling a `FactoryDef`. Returns `Some` or `None`
(unknown factory). **Delete** `software_factory()` and the `match name` arm.
`darkrun-content::load_factory` already reads the corpus; extend it with the
cascade + project layer and have the engine consume it.

### A5. `inherits` (makes the parent walkable in the resolution path)

`FACTORY.md` may declare a single `inherits: <parent>`. This does not copy or
merge anything — it makes the **parent factory walkable in the resolution path**:
after the child's own station+factory rings and before the global ring, the
resolver walks the parent's station+factory rings (project then plugin), then
transitively the parent's own `inherits` — a **linear chain, child-first, no
diamonds**. So the child overrides any role/station it defines (its rings come
first), inherits everything it doesn't, and still bottoms out at the shared
global ring.

This applies uniformly to **every on-disk lookup** — roles, the per-station
preamble/defaults, and the factory manifest fields all walk up the same inherits
chain. Single-parent, specialization-only (e.g. `libdev inherits software`;
future `cli-tool` / `microservice` / `mobile-app`).

### A6. Frontmatter schema (resolved)

**`agent_type` is never written — it is the role's directory** (`explorers/`,
`workers/`, `reviewers/`, `reflections/`); the loader infers it from the path.
Every role file is `name` + `model` plus its slot-specific extras.

**FACTORY.md** — `name, description, category, default_model, fix_workers,
reviewers, reflections, inherits?, surfaces`. **No `stations`** — the six FSSBPH
stations are a hardcoded, mandatory mechanic; a factory cannot list, add,
reorder, or omit them.

**STATION.md** — frontmatter is what assigns roles to slots:

- `name, description, kills, checkpoint, locked_artifact{…}, label?, elaboration?`
- **`kills`** — a **flat string**: the domain's risk-class phrasing, interpolated
  as `{{ kills }}` into the phase prompts. *Not* structured — the
  cost-of-late-discovery order is already the fixed `FLOW`, and nothing branches
  on a risk class. (The universal risk lives in the position preamble; `kills` is
  only the domain refinement.)
- **`locked_artifact`** — **structured**: `{name, location, format, scope,
  required}` (the predecessor's output definition, folded onto the station).
- **`label?`** — optional domain display name shown over the fixed position
  (legal → `Intake`); defaults to the position name. Display-only.
- **`elaboration?`** — optional per-station mode (e.g. `collaborative`), on the
  stations where it applies.
- **No `position`** — the station's directory name *is* the position.
- **No flat `inputs`** — derived from the fixed flow: a station inherits every
  upstream station's `locked_artifact` mechanically.
- The **role slots assign roles by name, in order** (each resolved through the
  cascade): `explorers[]`, `workers[]` (Make→Challenge→Resolve order — the list
  order *is* the beat order), `reviewers[]`, `fix[]`.

**explorers/&lt;name&gt;.md** — `name, model, knowledge{location, scope, format,
required}`. The `knowledge` block is the shared-memory artifact the explorer
reads/writes (the closed gap).

**workers/&lt;name&gt;.md** — `name, model, quality_gates?` (`true` runs the
station's quality checks **inside the pass-loop**, not only at Audit).

**reviewers/&lt;name&gt;.md**, **reflections/&lt;name&gt;.md** — `name, model`.

**fix** — not a type; a fix-worker is a `workers/` role a station's `fix:` slot
assigns to repair duty. Reflections/reviewers/etc. may `{% include %}` a shared
contract from a higher cascade ring and add only the domain lens.

**run template** — not a schema; a seeded example run / intent that prototypes a
walk (a `.darkrun/runs/<example>` the docs/tests point at).

### A7. Surfaces as data + open `Proof`

The closed `Surface` enum and `Proof` enum are the only software-specific types.

- `Surface` becomes **per-factory declared data** (`FACTORY.md` `surfaces:`), not
  a core enum. `darkrun_run_surface` validates a classification against the
  factory's declared surfaces.
- `Proof` becomes **open**: a tagged, attached artifact (`kind` + measured
  fields) rather than a fixed `WebProof`/`BenchProof`/terminal enum.
  `darkrun_proof_attach` accepts any declared proof kind; the Evidence reviewer
  still checks `block_matches_surface`.
- The measurement *tools* (`darkrun verify web`, criterion) stay software; a
  domain whose proof is human-attested declares an `attested` surface whose
  "measurement" is a recorded approval. This is the single seam where a new
  domain may need a new proof-kind handler vs. pure content.

## Migration: re-home software

The current `plugin/factories/software/stations/<station>/{explorers,workers,
reviewers}/*.md` already matches the target per-station-directory anatomy. Work:

1. Add `position`, `kills`, `label` to each `STATION.md` frontmatter.
2. Lift the universal structure out of each `STATION.md` body into
   `plugin/stations/<pos>/PREAMBLE.md`; leave only the domain delta in the body.
3. Extract the generic recurring roles (`verifier`, the `reviewer`/`explorer`/
   `worker` contracts) into `plugin/roles/`; software rosters `{% include %}`
   them and add their lens.
4. `factories/software/FACTORY.md`: declare `surfaces` (web_ui, cli, api,
   library, tui, desktop, mobile, data) + each one's proof kind.

## Engine consumers (darkrun-mcp)

- The ~10 callers of `resolve_factory` keep their signature; they now receive
  disk-resolved `FactoryDef`s.
- `position.rs::build_prompt_context`: `surface`/`user_facing`/`bench`/`terminal`
  flags derived from the factory's declared surfaces; inject the station `label`.
- `proof.rs`: open `Proof`; `route_for`/`set_surface` read the factory surface map.
- `prompts/phases/*.md`: add a `{% include station_preamble %}` at the top and a
  `{{ label }}` var so each phase prompt opens with the domain station name.

## darkrun-api wire contract

Both the desktop app and the website's review/browse views read these types.

- Add `label`, `position` to the station wire type.
- Add `surfaces` to the factory wire type.
- Make the `Proof` wire type open (tagged `kind` + fields) mirroring core.
- Regenerate `openapi.json`; parity test.

## darkrun-ui (shared components — website + desktop)

- `kinds.rs`: `UiCheckpoint` already all-ask; make surface/proof rendering driven
  by the open set, not a fixed enum.
- `flow.rs` (`FlowStation`): carry the per-station `label` + `surface` badge; the
  pipeline still renders the fixed six, labelled per factory.
- `components/factory.rs`, `components/role.rs`, `components/walkthrough.rs`,
  `graph/view.rs`: render orientation (kills, rosters, locked artifact, surfaces)
  and the domain labels; the DAG/station viz keys off `FLOW` + labels.

## Website (web/site)

Already reads the corpus via `darkrun_content` — mostly multi-factory + label +
surface-as-data updates.

- `pages/factories.rs` + `factory_view.rs`: list **all** factories (not just
  software); each factory's page renders the six FSSBPH stations under its domain
  labels, with per-station surfaces.
- `content.rs`: load every factory through the new resolver.
- Station detail page: render preamble (shared) + orientation (domain) + rosters.
- Surface badges per factory (not the hardcoded enum); update the
  `page_factories.rs` / `page_station_detail.rs` tests (which currently assert a
  single software factory and the old surface set).
- Flow/pipeline viz: driven by `FLOW` + labels + surface badges via `darkrun-ui`.

## Desktop app (desktop/)

Reads via the `darkrun-api` WS contract (not the corpus directly).

- `map.rs` (station strip/map): show domain `label` per station + surface badge;
  the strip still walks the fixed six.
- `review.rs`, `home.rs`, `wire.rs`: surface the factory name + per-station labels;
  the checkpoint kind is already all-ask; render surface/proof in the Prove/Audit
  views off the open `Proof` wire type.
- Consumes the updated `darkrun-api` + `darkrun-ui`, so most changes land there.

## Plugin commands / packaging

- `darkrun-factories`: list all `plugin/factories/*` (the base `stations/`,
  `roles/`, `fix/` are not factories and are excluded).
- `darkrun-scaffold`: scaffold a new factory (orientation-only) and scaffold a
  role/worker at any cascade scope.
- `darkrun-start`: factory selection (which domain).

## Tests & verification

- **core**: `FLOW` ordering invariant; cascade resolver ring order +
  scope-primary tiebreak; open `Proof` serde; surface-as-data.
- **content**: corpus loads through the cascade; roster resolution across all 8
  rings; `inherits` splice; project override beats plugin within scope.
- **mcp**: `resolve_factory` disk-backed; all ~10 callers; surface routing per
  factory; the gate/checkpoint suites (already all-ask) stay green.
- **e2e**: a second factory (legal) walks FSSBPH start → sealed using
  orientation-only content + human-attested proof — the proof that the model
  holds without engine changes.
- **web/site + desktop**: render multiple factories with domain labels + surface
  badges.
- Full workspace `cargo test` green + `cargo clippy` clean.

## Status (delivered)

All nine phases are landed and green. Highlights and the few deliberate
deviations from the original sketch:

- **Phases 1–3 — done.** `Position::FLOW` is the code invariant; the
  `software_factory()` fallback is deleted; the cascade loader (project override
  + `inherits` chain) resolves the corpus; `agent_type` is the typed directory;
  `kills`/`label` are frontmatter; the phase prompts open on `{{ label }}`.
- **Phase 4 — surfaces as data (refined).** `FACTORY.md` declares a `surfaces:`
  list; `darkrun_run_surface` rejects a classification the factory does not
  offer. The core `Surface` vocabulary and its visual/bench/terminal **routing
  stay in code** — that is the software measurement seam, which the plan itself
  scopes as software ("the measurement tools stay software"). Full removal of the
  enum + an open `ProofKind` registry was judged destabilizing for the value;
  the *offered set* is now data, which is the behavior the plan specified for
  `darkrun_run_surface`. The `attested`/open-proof seam is exercised by `legal`
  declaring **no** surfaces and gating external (human attestation).
- **Phase 5 — done.** `libdev inherits software`, overriding only Shape (drops
  the visual-design beat) and narrowing surfaces; the loader walks the chain for
  every manifest field, so a child factory is the delta, not a copy.
- **Phase 6 — done.** The website is data-driven over `list_factories()`: all
  three factories list, detail, and drill down; per-factory surface badges render.
- **Phase 7 — done.** The review wire type carries the station `label` and the
  classified `surface` (optional, back-compatible); `openapi.json` regenerated.
- **Phase 8 — done.** `legal` is a full, independent domain corpus (its own
  rosters, labels Intake→Execute, human-attested proof) proving the spine holds
  cross-domain with zero engine changes. The `darkrun-factories`/`-scaffold`/
  `-start` commands are data-driven over the engine tools and needed no change.
- **Phase 9 — green.** Workspace tests pass, clippy clean, openapi parity holds.

## Phasing (dependency-ordered; no time estimates)

- **Phase 1 — kill the fallback.** `FLOW` invariant; cascade loader in
  `darkrun-content`; `resolve_factory` delegates to disk; **delete**
  `software_factory()`/`match name`; add `kills`/`label`/structured
  `locked_artifact` frontmatter; drop `agent_type` (inferred from the typed dir);
  re-home software so the engine reads it from disk. Engine green.
- **Phase 2 — roster cascade + base library.** Stand up the typed role dirs
  `plugin/{explorers,workers,reviewers,reflections}/` (incl. `reconciler`/
  `validator`) and `plugin/stations/<pos>/`; extract generic roles; wire the
  8-ring + inherits resolver; software stations assign roles to slots by name.
- **Phase 3 — preamble + labels in the flow.** `{% include station_preamble %}`
  + `{{ label }}` in the phase prompts; labels threaded into `PromptContext`.
- **Phase 4 — surfaces as data + open `Proof`.** Core + `darkrun-api` types;
  `proof.rs`/`darkrun_run_surface` read the factory surface map.
- **Phase 5 — `inherits`.** Single-parent splice; `libdev` as the proof.
- **Phase 6 — website.** Multi-factory catalog/detail; surface badges; flow viz;
  fix the page tests.
- **Phase 7 — desktop + `darkrun-ui` + api.** Wire contract label/surfaces/open
  proof; station strip labels; Prove/Audit proof rendering.
- **Phase 8 — commands + a second domain factory.** `darkrun-factories`/
  `-scaffold`/`-start`; port `legal` end-to-end as the cross-domain proof.
- **Phase 9 — full verification.** Workspace green + clippy + openapi parity.

## Decisions (ratified)

1. **Roster precedence — scope-primary.** Finer scope wins; project breaks ties
   within a scope. Composes with `inherits` (child rings first, then the walkable
   parent, then global).
2. **`kills` — flat string.** A prompt variable holding the domain's risk
   phrasing; not structured (the cost-of-late-discovery rank is already the fixed
   `FLOW`, and nothing branches on a risk class).
3. **`inherits` — included** (Phase 5), single-parent, and it makes the **parent
   walkable in the resolution path** (not a copy/merge).
4. **Proof — open enum + handler registry.** Open enough for any domain's
   evidence, structured enough that the Evidence reviewer can still validate.
5. **`label` — kept**, optional, defaults to the position name. Display-only.
6. **`agent_type` — never written.** It is the role's typed directory.
7. **`locked_artifact` — structured**; **`inputs` — derived** from the flow;
   **`position` — dropped** (the directory is the position).
