# darkrun-sim — protocol-fidelity harness

darkrun's engine hands an agent a **rendered prompt** every tick and trusts it to
read that prompt and call the right tool. Every other test drives the engine
through library calls and reacts to the structured `RunAction`; none of them read
the prompt text the agent actually follows. That leaves a blind spot: a prompt
could name a tool that no longer exists, or drop the sentence that tells the agent
what to do, and CI would stay green.

This crate closes that blind spot with a **dumb, no-knowledge simulated agent**.
Given only the engine's rendered prompt for a tick — never the engine internals,
never the structured action — the agent recovers *which darkrun tool to call next*
purely from the wording. A prompt-wording regression then fails a test instead of
shipping an unfollowable instruction.

This is a first, meaningful version, not the full envisioned simulator. It covers
the core station-advance phases, the checkpoint / operator-gate paths, and a
question/answer path, and is structured so more scenarios are cheap to add.

## The two fidelity checks

1. **Followability.** For each representative tick, the sim reads the prompt and
   its `primary_deliverable()` matches the tool the engine intended for that
   action. If an edit drops the operative instruction or rewords it past
   recognition, this fails.
2. **No stale tool names.** Every `darkrun_*` a prompt names is a real, registered
   MCP tool. A renamed, misspelled, or removed tool in any template is caught —
   both in the prompts an agent actually reaches and in a corpus-wide static scan
   of every template branch.

## How it works

- `agent` — the **pure** prompt reader (`SimAgent::read`). No engine dependency.
  It knows only the *universal protocol vocabulary* (`darkrun_tick` /
  `darkrun_advance` re-drive the cursor; `darkrun_checkpoint_decide` is the
  operator gate; a `*_list` / `*_get` / `*_result` name is a read; everything else
  writes state). From that alone it collects the tools a prompt names, drops the
  ones named only in an *alternative / exception* clause ("file it with `X`
  **instead of** stamping"), and picks the operative one — the gate decision if the
  tick hands off to the operator, else the last write it is told to record.
- `harness` — drives a **real** engine Run (`run_start → run_tick →
  checkpoint_decide`) in a temp dir and captures each tick's rendered prompt.
  Nothing is mocked; the prompt text is exactly what a live agent would read.
- `tool_registry` — the canonical set of MCP tool names, parsed at compile time
  from `darkrun-mcp/src/tools.rs` (rmcp generates the live tool-list accessor
  crate-private, so it is unreachable from here).
- `scenarios` — the representative ticks the suite asserts on.

## Running it

```sh
cargo test  -p darkrun-sim
cargo clippy -p darkrun-sim --all-targets -- -D warnings
```

## Adding a scenario

A scenario is one representative tick plus what a follower should recover from it.

1. **Reach the tick.** Most station phases are surfaced by walking a Run to seal,
   so they are already captured by `Harness::capture_to_seal` under their action
   tag (`spec`, `review`, `manufacture`, `audit`, `reflect`, `user_gate`,
   `checkpoint`, …) — just pull the tag. For an action a clean walk never reaches
   (a feedback question, an escalation), either set that state up on the `Harness`
   before capturing, or render the action directly with `Harness::render(&action)`.
2. **Declare what a follower should recover.** Push a `scenarios::Scenario` with
   the captured prompt and an `Expect`:
   - `primary` — the one operative tool the tick points the agent at, or `None`
     for a terminal/hold tick that records nothing;
   - `requires` — tools the prompt must name as required, non-alternative steps;
   - `holds_for_operator` — whether the tick hands off to an operator gate;
   - `expect_transition` — whether the prompt must end by telling the agent to
     re-tick / advance.

The followability test loops every scenario, so a new one needs no new test.

Example:

```rust
Scenario {
    name: "reflect → record learnings",
    action_tag: "reflect",
    prompt: get("reflect"),
    expect: Expect {
        primary: Some("darkrun_reflection_record"),
        requires: &["darkrun_reflection_record"],
        holds_for_operator: false,
        expect_transition: true,
    },
},
```

## Known gap this harness surfaces

`phases/checkpoint.md`'s compound-gate block references `darkrun_checkpoint_choose`,
which is **not** a registered MCP tool. That block only renders when the manager
populates `checkpoint_options`, which it never does today, so no live tick surfaces
it — a latent stale reference, not an active unfollowable instruction. The
corpus-wide scan pins it as a documented baseline (a *ceiling*): a new stale
reference fails the test, and the baseline can shrink once the engine wires the
option up or drops the reference.
