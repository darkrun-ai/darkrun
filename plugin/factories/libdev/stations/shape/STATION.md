---
name: shape
description: Shape the structure — decide the public API and prove the risky assumptions cheaply before they get expensive. A library has no UI, so there is no visual-design beat.
kills: expensive-structural-reversal
explorers: [surface, architecture, risk]
workers: [designer, spiker, pressure_tester, resolver]
reviewers: [fit, reversibility, simplicity]
locked_artifact: design.md
inputs: [frame.md, spec.md]
---

# Shape

Shape decides *how* the spec gets satisfied structurally. For a library the
structure that matters most is the **public API** — the names, signatures, and
guarantees other code compiles against. It kills the most expensive class of late
discovery: **structural reversal** — shipping an API shape that consumers depend
on and then having to break it. Structure is the most expensive thing to change
late, so Shape pays a small cost now (a throwaway spike) to avoid an enormous one
later (a breaking-change migration across every consumer).

## Risk class eliminated

*Expensive structural reversal.* The spec is clear, but the chosen public surface
collides with reality only after consumers depend on it — wrong abstraction,
leaky boundary, an assumption that does not hold once real callers arrive.

## What this station produces

- **The classified surface** — a library always shapes a **public API**, recorded
  onto the run via `darkrun_run_surface`. This routes both *how Shape designs*
  (public-API design: the contract other code links against) *and how Prove/Audit
  verify* (API stability, doctests, semver discipline).
- **The design** — modules, boundaries, the exported types and traits, data flow,
  and the key decisions with their rationale, shaped to the public surface.
- **Spike results** — the output of a throwaway proof that the riskiest
  assumptions actually hold. Spikes are deleted after; only their findings survive.

## The pass-loop

A library has no UI, so Shape runs **without the visual-design beat**:

- **Designer** classifies the surface as a public API, records it with
  `darkrun_run_surface`, then proposes the export structure that satisfies the
  spec with the least machinery — designing *for consumers*, not for screens.
- **Spiker** builds a *throwaway* proof of the riskiest assumptions — the thing
  most likely to be wrong — and reports what it learned. The spike code is
  discarded; the knowledge is kept.
- **PressureTester** attacks the API under change: what reverses this, what locks
  a consumer into a corner, what is hard to evolve without a breaking change?
- **Resolver** reconciles the spike findings and pressure tests into the final
  design.

## Locked artifact

`design.md` + spike results — the public API plus evidence the risky parts work.
Build inherits this and may not re-litigate the surface; a structural change is
drift that routes back here.

## Checkpoint

**ask.** A human signs off on the public API (and may route it **external** for a
formal API review on widely-depended-on libraries) before Build commits to it.
