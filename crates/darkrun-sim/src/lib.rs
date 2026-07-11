//! # darkrun-sim — the protocol-fidelity harness
//!
//! darkrun's engine hands an agent a **rendered prompt** each tick and trusts it
//! to read that prompt and call the right tool. Every other test drives the
//! engine through library calls and reacts to the structured [`RunAction`]; none
//! of them read the prompt text the agent actually follows. That leaves a blind
//! spot: a prompt could name a tool that no longer exists, or drop the sentence
//! that tells the agent what to do, and CI would stay green.
//!
//! This crate closes that blind spot with a **dumb, no-knowledge simulated
//! agent**. Given only the engine's rendered prompt for a tick — never the
//! engine internals, never the structured action — the agent
//! ([`agent::SimAgent`]) recovers *which darkrun tool to call next* purely from
//! the wording. The followability suite drives representative engine ticks
//! through the real renderer, feeds each rendered prompt to the agent, and
//! asserts the agent recovers the intended action. A prompt-wording regression
//! then fails a test instead of shipping an unfollowable instruction.
//!
//! ## The two fidelity checks
//!
//! 1. **Followability** — for each representative tick, the sim reads the prompt
//!    and its [`Plan::primary_deliverable`] matches the tool the engine intended
//!    for that action. See [`scenarios::core_scenarios`].
//! 2. **No stale tool names** — every `darkrun_*` a prompt names is a real,
//!    registered MCP tool ([`tool_registry::known_tool_names`]). A renamed or
//!    misspelled tool in any template is caught here.
//!
//! ## Layout
//!
//! - [`agent`] — the pure prompt reader. No engine dependency; unit-testable in
//!   isolation.
//! - [`harness`] — drives a **real** engine Run and captures each tick's rendered
//!   prompt. Nothing is mocked.
//! - [`tool_registry`] — the canonical set of MCP tool names, for the stale-name
//!   check.
//! - [`scenarios`] — the representative ticks the followability suite asserts on.
//!
//! ## Adding a scenario
//!
//! A scenario is one representative tick plus what a follower should recover from
//! it. To add one:
//!
//! 1. Make the harness reach the tick. If a linear walk surfaces the action
//!    (most station phases do), it is already captured by
//!    [`harness::Harness::capture_to_seal`] under its action tag — just pull it.
//!    If the action needs specific state (a feedback item, an escalation), either
//!    set that state up on the [`harness::Harness`] before capturing, or render
//!    the action directly with [`harness::Harness::render`].
//! 2. Push a [`scenarios::Scenario`] with the captured prompt and an
//!    [`scenarios::Expect`] naming the tool a follower must recover, the tools it
//!    must require, whether the tick holds for an operator, and whether it ends
//!    by telling the agent to re-tick.
//!
//! The followability test loops every scenario, so a new one needs no new test.

pub mod agent;
pub mod harness;
pub mod scenarios;
pub mod tool_registry;

pub use agent::{Plan, SimAgent, ToolMention, ToolRole};
pub use tool_registry::known_tool_names;
