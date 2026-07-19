//! The representative ticks the followability suite asserts on.
//!
//! Each [`Scenario`] pairs a **real** rendered prompt (captured by walking an
//! engine Run, or rendered directly for an action a linear walk never reaches)
//! with an [`Expect`] describing what a follower should recover from it. The
//! test in `tests/followability.rs` loops these, so adding a scenario needs no
//! new test — see the crate-level docs for the recipe.

use std::collections::BTreeMap;

use crate::harness::{action_tag, Harness};
use darkrun_core::domain::CheckpointKind;
use darkrun_mcp::position::{run_review_stamp, RunAction};

/// What a follower should recover from a scenario's prompt, reading the wording
/// alone.
#[derive(Debug, Clone)]
pub struct Expect {
    /// The one operative tool the tick points the agent at
    /// ([`crate::Plan::primary_deliverable`]). `None` for a terminal / hold tick
    /// that records nothing.
    pub primary: Option<&'static str>,
    /// Tools the prompt must name as required, non-alternative steps
    /// ([`crate::Plan::requires`]).
    pub requires: &'static [&'static str],
    /// Whether the tick hands off to an operator gate decision
    /// ([`crate::Plan::holds_for_operator`]).
    pub holds_for_operator: bool,
    /// Whether the prompt must end by telling the agent to re-tick / advance
    /// ([`crate::Plan::transition`] is `Some`).
    pub expect_transition: bool,
}

/// One representative tick: its action tag, the rendered prompt, and the
/// follower's expected read.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// A human label for test output.
    pub name: &'static str,
    /// The engine action tag that produced the prompt (`spec`, `checkpoint`, …).
    pub action_tag: &'static str,
    /// The real rendered prompt text the sim reads.
    pub prompt: String,
    /// What a follower must recover from it.
    pub expect: Expect,
}

/// The core scenario set: the station-advance phases (spec → checkpoint), the
/// pre-execution operator gate, and a feedback question/answer path — every one
/// driven through the real prompt renderer.
///
/// The station phases come from a single solo-mode `software` Run walked to seal;
/// the feedback-question path is rendered directly, since a clean walk never
/// files one.
pub fn core_scenarios() -> Vec<Scenario> {
    let solo = Harness::start("sim-core", "software", "solo");
    let prompts = capture_to_seal(&solo);
    let get = |tag: &str| -> String {
        prompts.get(tag).cloned().unwrap_or_else(|| {
            panic!("no prompt was captured for action `{tag}` — the walk never surfaced it")
        })
    };

    // The feedback-question tick: an open question preempts run progress and must
    // be surfaced to the operator. A linear walk never files one, so render it
    // directly against a real Run's state.
    let fq = Harness::start("sim-fq", "software", "solo");
    let fq_station = fq
        .store
        .read_state("sim-fq")
        .ok()
        .flatten()
        .map(|s| s.active_station)
        .unwrap_or_else(|| "frame".to_string());
    let fq_prompt = fq
        .render(&RunAction::FeedbackQuestion {
            run: "sim-fq".to_string(),
            station: fq_station,
            feedback_id: "fb-042".to_string(),
        })
        .expect("feedback_question prompt rendered");

    vec![
        Scenario {
            name: "station spec → create units",
            action_tag: "spec",
            prompt: get("spec"),
            expect: Expect {
                primary: Some("darkrun_unit_create"),
                requires: &["darkrun_unit_create"],
                holds_for_operator: false,
                expect_transition: true,
            },
        },
        Scenario {
            name: "spec review → stamp + brief",
            action_tag: "review",
            prompt: get("review"),
            expect: Expect {
                primary: Some("darkrun_brief_record"),
                requires: &["darkrun_review_stamp", "darkrun_brief_record"],
                holds_for_operator: false,
                expect_transition: true,
            },
        },
        Scenario {
            name: "manufacture → iterate the pass loop",
            action_tag: "manufacture",
            prompt: get("manufacture"),
            expect: Expect {
                primary: Some("darkrun_unit_iterate"),
                requires: &["darkrun_unit_iterate"],
                holds_for_operator: false,
                expect_transition: true,
            },
        },
        Scenario {
            name: "audit → stamp approvals",
            action_tag: "audit",
            prompt: get("audit"),
            expect: Expect {
                primary: Some("darkrun_review_stamp"),
                requires: &["darkrun_review_stamp"],
                holds_for_operator: false,
                expect_transition: true,
            },
        },
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
        Scenario {
            name: "pre-execution gate → operator decides",
            action_tag: "user_gate",
            prompt: get("user_gate"),
            expect: Expect {
                primary: Some("darkrun_checkpoint_decide"),
                requires: &["darkrun_checkpoint_decide"],
                holds_for_operator: true,
                expect_transition: true,
            },
        },
        Scenario {
            name: "checkpoint → closing brief, then hold",
            action_tag: "checkpoint",
            prompt: get("checkpoint"),
            expect: Expect {
                primary: Some("darkrun_brief_record"),
                requires: &["darkrun_brief_record"],
                holds_for_operator: false,
                expect_transition: true,
            },
        },
        Scenario {
            name: "feedback question → ask the operator",
            action_tag: "feedback_question",
            prompt: fq_prompt,
            expect: Expect {
                primary: Some("darkrun_question"),
                requires: &["darkrun_question"],
                holds_for_operator: false,
                expect_transition: true,
            },
        },
    ]
}

/// Walk the Run to a sealed state, capturing the **first** rendered prompt seen
/// for each distinct action tag (`spec`, `review`, `manufacture`, `audit`,
/// `reflect`, `user_gate`, `checkpoint`, …). The map is what the followability
/// suite reads.
///
/// This is the walk-until-`Sealed` loop, relocated verbatim from the pre-rebuild
/// `harness.rs`: same match arms, same `guard < 2000` convergence check. It
/// lives here (not in the narrowed `harness.rs`) so `harness.rs` makes no
/// decision keyed on the structured action; the `.action` reads below are the
/// loop's own post-hoc prompt-capture bookkeeping, off the sim world's
/// grade-confined tick loop entirely.
pub(crate) fn capture_to_seal(harness: &Harness) -> BTreeMap<String, String> {
    let mut prompts: BTreeMap<String, String> = BTreeMap::new();
    let mut guard = 0;
    loop {
        guard += 1;
        assert!(guard < 2000, "capture_to_seal failed to converge");
        let tick = harness.tick();
        let action = tick.action.clone();
        if let Some(p) = &tick.prompt {
            prompts
                .entry(action_tag(&action))
                .or_insert_with(|| p.clone());
        }

        // The pre-execution operator gate is an internal hold — approve and
        // keep walking.
        if matches!(action, RunAction::UserGate { .. }) {
            harness.decide(true, None);
            continue;
        }
        // The whole-run review holds until every reviewer signs — stamp them.
        if let RunAction::RunReview { reviewers, .. } = &action {
            for r in reviewers {
                run_review_stamp(&harness.store, &harness.slug, r).expect("run review stamp");
            }
            continue;
        }

        match &action {
            RunAction::Sealed { .. } => break,
            RunAction::Spec { station, .. } => harness.seed_spec(station),
            RunAction::Manufacture { units, .. } => {
                let owned: Vec<&str> = units.iter().map(|s| s.as_str()).collect();
                harness.complete_units(&owned);
            }
            // A held gate (non-auto checkpoint, or an external review gate)
            // needs an operator decision; the decide re-tick advances the
            // next station, so re-seed its spec to stay in sync.
            RunAction::Checkpoint { kind, .. } if !matches!(kind, CheckpointKind::Auto) => {
                let decided = harness.decide(true, None);
                if let RunAction::Spec { station, .. } = &decided.action {
                    harness.seed_spec(station);
                }
            }
            RunAction::ExternalReviewRequested { .. } => {
                let decided = harness.decide(true, None);
                if let RunAction::Spec { station, .. } = &decided.action {
                    harness.seed_spec(station);
                }
            }
            _ => {}
        }
    }
    prompts
}
