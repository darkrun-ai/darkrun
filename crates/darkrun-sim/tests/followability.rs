//! Protocol-fidelity suite: can a dumb, no-knowledge agent follow the engine's
//! rendered prompts?
//!
//! Each test drives the **real** prompt renderer and hands the resulting text —
//! nothing else — to [`SimAgent`]. If the agent recovers the intended tool, the
//! prompt is followable. If a prompt-wording regression (an unfollowable
//! instruction, a stale tool name) sneaks in, one of these fails.

use std::collections::BTreeSet;

use darkrun_prompts::Cascade;
use darkrun_sim::agent::SimAgent;
use darkrun_sim::known_tool_names;
use darkrun_sim::scenarios::core_scenarios;

/// The heart of the suite: for every representative tick, the sim reads the
/// prompt text alone and recovers the action the engine intended.
#[test]
fn a_dumb_agent_follows_every_core_prompt() {
    let known = known_tool_names();

    for scenario in core_scenarios() {
        let plan = SimAgent::read(&scenario.prompt);
        let ctx = format!("scenario `{}` [{}]", scenario.name, scenario.action_tag);

        // 1. No stale tool names — every tool the prompt names is a real,
        //    registered MCP tool the agent could actually call.
        let unknown = plan.unknown_tools(&known);
        assert!(
            unknown.is_empty(),
            "{ctx}: prompt names tool(s) that are not registered MCP tools: {unknown:?}\n\
             an agent would try to call these and fail.\n--- prompt ---\n{}",
            scenario.prompt
        );

        // 2. The operative action — the one tool this tick points the agent at —
        //    is the one the engine intended for this action.
        if let Some(want) = scenario.expect.primary {
            assert_eq!(
                plan.primary_deliverable(),
                Some(want),
                "{ctx}: the sim read a different operative tool than intended.\n--- prompt ---\n{}",
                scenario.prompt
            );
        }

        // 3. Every required step the follower must take is named, and named on
        //    the happy path (not only in an exception clause).
        for want in scenario.expect.requires {
            assert!(
                plan.requires(want),
                "{ctx}: prompt does not require `{want}` as a happy-path step.\n--- prompt ---\n{}",
                scenario.prompt
            );
        }

        // 4. The gate hand-off reads correctly: a tick that holds for the
        //    operator names the decision, and one that doesn't, doesn't.
        assert_eq!(
            plan.holds_for_operator(),
            scenario.expect.holds_for_operator,
            "{ctx}: operator-hold read differs from expected.\n--- prompt ---\n{}",
            scenario.prompt
        );

        // 5. A non-terminal tick tells the agent how to move on (re-tick /
        //    advance) — the loop can never dead-end.
        if scenario.expect.expect_transition {
            assert!(
                plan.transition().is_some(),
                "{ctx}: prompt never tells the agent to re-tick / advance.\n--- prompt ---\n{}",
                scenario.prompt
            );
        }
    }
}

/// A focused restatement of check (1): every tool named in a prompt an agent
/// actually reaches is a registered tool. Kept as its own test so a stale-name
/// regression names *this* failure, distinct from a followability drift.
#[test]
fn reachable_prompts_name_only_registered_tools() {
    let known = known_tool_names();
    for scenario in core_scenarios() {
        let plan = SimAgent::read(&scenario.prompt);
        assert!(
            plan.unknown_tools(&known).is_empty(),
            "`{}` prompt names an unregistered tool: {:?}",
            scenario.name,
            plan.unknown_tools(&known)
        );
    }
}

/// A corpus-wide static scan: every `darkrun_*` referenced by **any** prompt
/// template (including conditional branches a linear walk never renders) must
/// be a registered MCP tool. No exceptions: a stale or renamed reference (a
/// template naming a tool that no longer exists), or a new but unregistered
/// tool, fails this test. The former `darkrun_checkpoint_choose` compound-gate
/// reference has been dropped from `phases/checkpoint.md`, so the baseline is
/// now empty.
#[test]
fn corpus_references_only_registered_tools_except_documented_gaps() {
    let known = known_tool_names();

    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for key in Cascade::embedded_keys() {
        // Resolve the raw (unrendered) template through the cascade with a repo
        // root that has no overrides, so we read the embedded corpus. Scanning
        // raw source catches tool names in every branch, context-free.
        let raw = darkrun_prompts::resolve(&key, "/nonexistent-darkrun-sim-root")
            .unwrap_or_else(|e| panic!("resolve embedded template `{key}`: {e}"));
        referenced.extend(SimAgent::read(&raw).all_tools());
    }

    let unregistered: BTreeSet<String> = referenced.difference(&known).cloned().collect();

    // No documented-stale ceiling remains: the former `darkrun_checkpoint_choose`
    // reference has been dropped from the corpus, so every referenced tool must
    // now resolve to a registered MCP tool.
    let novel: Vec<String> = unregistered.iter().cloned().collect();
    assert!(
        novel.is_empty(),
        "prompt corpus references tool name(s) that are not registered MCP tools \
         and are not in the documented baseline: {novel:?}\n\
         either the tool was renamed/removed (fix the template) or the tool is new \
         (register it in darkrun-mcp)."
    );
}
