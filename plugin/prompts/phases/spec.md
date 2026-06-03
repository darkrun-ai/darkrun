{% include "_shared/announcement.md" %}

# Spec — `{{ station }}`

You are opening station **{{ station }}**. Its job is to eliminate a whole class of risk: **{{ kills }}**. Nothing downstream is allowed to proceed until that risk is named and bounded here.

{% include "_shared/contracts.md" %}

{% include "_shared/roster.md" %}

Spec runs **elaboration and discovery in tandem** — they are NOT two sequential
steps. The moment the station opens, kick off both at once: dispatch the explorers
in parallel *while* you frame the problem. They sharpen each other. Only once both
have landed do you decompose.

## elaborate — frame the problem (concurrently with discovery)

State plainly what this station must achieve to kill **{{ kills }}**: the intent, the inputs it inherits from upstream, and the boundary of what is explicitly *out of scope* so later phases don't drift into it. This is the frame the explorers work against — but do NOT wait on a finished frame to start them; the frame and the exploration are written in parallel and inform each other.

## discover — run the explorers in parallel (concurrently with elaboration)

Dispatch **all** explorers{% if explorers %} ({% for e in explorers %}`{{ e }}`{% if not loop.last %}, {% endif %}{% endfor %}){% endif %} **at once, in parallel** — one subagent each, fanned out concurrently, never one-after-another. Explorers don't build — they surface unknowns, constraints, prior art, and traps. They run alongside your framing; neither blocks the other.

## decompose — once elaboration + discovery have both landed

Turn the framed, explored problem into the smallest set of independently completable **Units** that, together, kill the risk above. For each Unit write:
   - a one-line intent,
   - explicit **completion criteria** (how you'll know it's done — testable, not vibes),
   - its dependencies on other Units (so the manager can wave them).

{% if units %}
### Units already on record
{% for u in units %}
- `{{ u }}`
{% endfor %}
Reconcile these against what the explorers found — extend, split, or tighten them; don't blindly accept them.
{% else %}
There are no Units yet. You are creating them.
{% endif %}

{% if user_facing %}
### User-facing surfaces

This work touches a **user-facing surface**. For every Unit that renders a screen, flow, component, or page, mark it as visual so Shape's design step knows to act: its UI must not be built until the operator has chosen a design direction (via `darkrun_question` / `darkrun_direction`). Make the surface and its acceptance criteria explicit here; non-visual Units carry no such requirement.
{% endif %}

## Done when

The spec names the risk, lists Units with testable completion criteria and dependencies, and marks what's out of scope. Write it to the station's spec artifact, then call `darkrun_tick`.
