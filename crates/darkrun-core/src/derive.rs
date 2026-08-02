//! The **shared, pure phase derivation** — one source of truth consumed by every
//! surface (the engine cursor, the HTTP browse endpoints, the desktop app, and the
//! website). Mirrors the predecessor's `packages/shared/derived-stage-state.ts`.
//!
//! A station's phase/status is a **pure function of its units' on-disk
//! frontmatter** — never a stored snapshot. Because the engine, HTTP, and desktop
//! all call these same functions over the same `Unit` documents, they can never
//! disagree about where a run is. There is no `state.json` to drift.
//!
//! The role lists (`review_roles`, `approval_roles`) are mode-shaped by the caller
//! (autopilot trims the `user` role) so this module stays a pure derivation.

use crate::domain::{IterationResult, StationPhase, Status, Unit};

/// Whether a unit has every required review role stamped (the PRE-execute gate).
fn reviews_signed(unit: &Unit, review_roles: &[String]) -> bool {
    review_roles
        .iter()
        .all(|role| matches!(unit.frontmatter.reviews.get(role), Some(Some(_))))
}

/// Whether a unit has every required approval role stamped (the POST-execute gate).
fn approvals_signed(unit: &Unit, approval_roles: &[String]) -> bool {
    approval_roles
        .iter()
        .all(|role| matches!(unit.frontmatter.approvals.get(role), Some(Some(_))))
}

/// A beat is **settled** when its recorded result releases the loop: the worker
/// either advanced or was consciously waived. A reject (bounce pending) and an
/// in-flight beat (`result: None`) are both unsettled.
fn beat_settled(result: Option<IterationResult>) -> bool {
    matches!(result, Some(IterationResult::Advance) | Some(IterationResult::Skip))
}

/// A worker's MOST RECENT recorded result on this unit, or `None` if it never
/// ran at all. The outer `Option` distinguishes "never ran" from "ran, still in
/// flight" (`Some(None)`).
fn latest_beat(unit: &Unit, worker: &str) -> Option<Option<IterationResult>> {
    unit.frontmatter
        .iterations
        .iter()
        .rev()
        .find(|it| it.worker == worker)
        .map(|it| it.result)
}

/// Whether a unit's Pass loop is complete: **every declared worker has settled**
/// (advanced, or been consciously waived with a recorded reason) and the loop
/// ended on the station's last worker. With no declared workers, any settled
/// last beat qualifies (research-style stations that only produce artifacts).
///
/// The per-worker coverage check is load-bearing. This function used to ask only
/// whether the LAST iteration advanced on the terminal worker, which made the
/// declared worker sequence decorative: a unit could record `designer` then jump
/// straight to `resolver` and be marked `completed` with the middle of its own
/// pipeline never run. That is not a hypothetical — it happened across a real
/// run, silently, on units the engine then reported as done. A beat that should
/// not run is a `Skip` with a reason, never an absence.
fn pass_loop_done(unit: &Unit, workers: &[String]) -> bool {
    let Some(last) = unit.frontmatter.iterations.last() else {
        return false;
    };
    if !beat_settled(last.result) {
        return false;
    }
    match workers.last() {
        // The loop must END on the terminal worker AND have covered every
        // declared beat on the way — neither check implies the other.
        Some(terminal) => {
            &last.worker == terminal
                && workers
                    .iter()
                    .all(|w| latest_beat(unit, w).is_some_and(beat_settled))
        }
        None => true,
    }
}

/// The declared workers a unit has **not** settled: never ran, still in flight,
/// or sitting on an unresolved reject. Empty means the Pass loop is covered.
///
/// Surfaces render this so a jumped beat is visible as a jumped beat rather than
/// as an indistinguishable blank, and the audit path uses it to find units that
/// were marked complete under the old terminal-worker-only rule.
pub fn unsettled_workers(unit: &Unit, workers: &[String]) -> Vec<String> {
    workers
        .iter()
        .filter(|w| !latest_beat(unit, w).is_some_and(beat_settled))
        .cloned()
        .collect()
}

/// Derive a station's [`StationPhase`] from its units — the pure cursor-walk
/// signal, shared by every surface.
///
/// Order is load-bearing (review BEFORE execute): a not-yet-spec-signed unit has
/// empty iterations and would otherwise mislabel as `Manufacture`.
///
/// - `elaboration_verified`: `Some(true)` verified, `Some(false)` present-unverified,
///   `None` artifact missing. Skipped entirely under `autopilot`.
pub fn derive_station_phase(
    units: &[Unit],
    workers: &[String],
    review_roles: &[String],
    approval_roles: &[String],
    elaboration_verified: Option<bool>,
    autopilot: bool,
) -> StationPhase {
    // 1. Elaborate gate (Spec phase). Skipped under autopilot.
    if !autopilot {
        if elaboration_verified == Some(false) {
            return StationPhase::Spec;
        }
        if elaboration_verified.is_none() && units.is_empty() {
            return StationPhase::Spec;
        }
    }
    // 2. Decompose pending → still Spec.
    if units.is_empty() {
        return StationPhase::Spec;
    }
    // 3. Review: any unit missing a required review role.
    if units.iter().any(|u| !reviews_signed(u, review_roles)) {
        return StationPhase::Review;
    }
    // 4. Manufacture: any unit whose Pass loop isn't done.
    if !workers.is_empty() && units.iter().any(|u| !pass_loop_done(u, workers)) {
        return StationPhase::Manufacture;
    }
    // 5. Audit/gate: any unit missing a required approval role (post-execute
    //    reviewers + quality gates sign here; Reflect/observations is a sub-step
    //    the cursor handles after the gate is signed).
    if units.iter().any(|u| !approvals_signed(u, approval_roles)) {
        return StationPhase::Audit;
    }
    // 6. All signed — the Checkpoint gate fires (awaiting the station→run-main merge).
    StationPhase::Checkpoint
}

/// Whether every unit in a station is fully signed (all reviews + approvals +
/// Pass loops done) — the predecessor's `isStageComplete`. A station with no units
/// is NOT complete (it still owes decomposition).
pub fn station_units_complete(
    units: &[Unit],
    workers: &[String],
    review_roles: &[String],
    approval_roles: &[String],
) -> bool {
    !units.is_empty()
        && units.iter().all(|u| {
            reviews_signed(u, review_roles)
                && pass_loop_done(u, workers)
                && approvals_signed(u, approval_roles)
        })
}

/// The lifecycle [`Status`] of a station relative to the active one: `Completed`
/// (before the active), `Active` (the active station), `Pending` (after).
pub fn station_status(index: usize, active_index: Option<usize>) -> Status {
    match active_index {
        Some(active) if index < active => Status::Completed,
        Some(active) if index == active => Status::Active,
        _ => Status::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Stamp, UnitFrontmatter, UnitIteration};

    fn unit(slug: &str) -> Unit {
        Unit {
            slug: slug.into(),
            frontmatter: UnitFrontmatter::default(),
            title: slug.into(),
            body: String::new(),
        }
    }
    fn signed() -> Option<Stamp> {
        Some(Stamp { at: "2026-06-02T00:00:00Z".into() })
    }
    fn roles(rs: &[&str]) -> Vec<String> {
        rs.iter().map(|s| s.to_string()).collect()
    }

    /// Append a settled beat to a unit.
    fn beat(u: &mut Unit, worker: &str, result: IterationResult) {
        u.frontmatter.iterations.push(UnitIteration {
            worker: worker.into(),
            result: Some(result),
            ..Default::default()
        });
    }

    /// A unit that has cleared its review gate, so phase derivation falls
    /// through to the Pass loop rather than stopping at Review.
    fn reviewed(slug: &str, review_roles: &[String]) -> Unit {
        let mut u = unit(slug);
        for r in review_roles {
            u.frontmatter.reviews.insert(r.clone(), signed());
        }
        u
    }

    #[test]
    fn a_jumped_middle_beat_does_not_complete_the_pass_loop() {
        // THE REGRESSION. The old rule asked only "did the last iteration
        // advance on the terminal worker", so a unit could record the first
        // worker, jump the middle of its own declared pipeline, and land on the
        // terminal worker to be marked complete. A real run did exactly this on
        // five units across two stations; three were reported `completed` with
        // two of five beats never run.
        let workers = roles(&["designer", "visual_designer", "spiker", "pressure_tester", "resolver"]);
        let review_roles = roles(&["fit"]);

        let mut u = reviewed("design-container-and-crypto", &review_roles);
        beat(&mut u, "designer", IterationResult::Advance);
        beat(&mut u, "pressure_tester", IterationResult::Advance);
        beat(&mut u, "resolver", IterationResult::Advance);

        // Last beat advanced ON the terminal worker — the old rule's entire test.
        assert_eq!(u.frontmatter.iterations.last().unwrap().worker, "resolver");
        assert!(!pass_loop_done(&u, &workers), "jumped beats must not complete the loop");

        // The two jumped beats are named, not silently absent.
        assert_eq!(
            unsettled_workers(&u, &workers),
            vec!["visual_designer".to_string(), "spiker".to_string()]
        );

        // And the station stays in Manufacture rather than sailing to Audit.
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&u), &workers, &review_roles, &[], Some(true), false
            ),
            StationPhase::Manufacture
        );
        assert!(!station_units_complete(&u_slice(&u), &workers, &review_roles, &[]));
    }

    fn u_slice(u: &Unit) -> Vec<Unit> {
        vec![u.clone()]
    }

    #[test]
    fn a_recorded_skip_settles_a_beat_and_completes_the_loop() {
        // A waived beat is legitimate — when it is written down. The same unit
        // that fails above passes once the two jumped beats are recorded as
        // conscious skips, which is the whole point of the variant.
        let workers = roles(&["designer", "visual_designer", "spiker", "pressure_tester", "resolver"]);
        let review_roles = roles(&["fit"]);

        let mut u = reviewed("design-container-and-crypto", &review_roles);
        beat(&mut u, "designer", IterationResult::Advance);
        beat(&mut u, "visual_designer", IterationResult::Skip);
        beat(&mut u, "spiker", IterationResult::Skip);
        beat(&mut u, "pressure_tester", IterationResult::Advance);
        beat(&mut u, "resolver", IterationResult::Advance);

        assert!(pass_loop_done(&u, &workers), "recorded skips settle their beats");
        assert!(unsettled_workers(&u, &workers).is_empty());
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&u), &workers, &review_roles, &[], Some(true), false
            ),
            StationPhase::Checkpoint
        );
    }

    #[test]
    fn an_unresolved_reject_leaves_its_beat_unsettled() {
        // A reject bounces back; until that worker advances again it is not
        // settled, even if a later worker went on to advance.
        let workers = roles(&["make", "challenge", "resolve"]);
        let mut u = unit("a");
        beat(&mut u, "make", IterationResult::Advance);
        beat(&mut u, "challenge", IterationResult::Reject);
        assert_eq!(unsettled_workers(&u, &workers), roles(&["challenge", "resolve"]));
        assert!(!pass_loop_done(&u, &workers));

        // The bounce is worked and the loop runs forward to the end.
        beat(&mut u, "make", IterationResult::Advance);
        beat(&mut u, "challenge", IterationResult::Advance);
        beat(&mut u, "resolve", IterationResult::Advance);
        assert!(unsettled_workers(&u, &workers).is_empty());
        assert!(pass_loop_done(&u, &workers));
    }

    #[test]
    fn an_in_flight_beat_is_not_settled() {
        // `result: None` means still running — never a completed beat.
        let workers = roles(&["make", "resolve"]);
        let mut u = unit("a");
        beat(&mut u, "make", IterationResult::Advance);
        u.frontmatter.iterations.push(UnitIteration {
            worker: "resolve".into(),
            result: None,
            ..Default::default()
        });
        assert_eq!(unsettled_workers(&u, &workers), roles(&["resolve"]));
        assert!(!pass_loop_done(&u, &workers));
    }

    #[test]
    fn the_loop_must_still_end_on_the_terminal_worker() {
        // Covering every beat is necessary but not sufficient: the run must come
        // to rest on the last worker, so a late bounce cannot read as done.
        let workers = roles(&["make", "resolve"]);
        let mut u = unit("a");
        beat(&mut u, "resolve", IterationResult::Advance);
        beat(&mut u, "make", IterationResult::Advance);
        assert!(unsettled_workers(&u, &workers).is_empty(), "both beats settled");
        assert!(!pass_loop_done(&u, &workers), "but the loop ended on `make`");
    }

    #[test]
    fn a_station_with_no_declared_workers_still_completes_on_any_advance() {
        // Research-style stations that only produce artifacts keep the old
        // behaviour — there is no sequence to enforce.
        let mut u = unit("a");
        beat(&mut u, "whoever", IterationResult::Advance);
        assert!(pass_loop_done(&u, &[]));
        assert!(unsettled_workers(&u, &[]).is_empty());
    }

    #[test]
    fn empty_units_is_spec() {
        assert_eq!(
            derive_station_phase(&[], &[], &[], &[], Some(true), false),
            StationPhase::Spec
        );
    }

    #[test]
    fn unverified_elaboration_is_spec_unless_autopilot() {
        let us = [unit("a")];
        assert_eq!(
            derive_station_phase(&us, &roles(&["w"]), &[], &[], Some(false), false),
            StationPhase::Spec
        );
        // autopilot skips the elaborate gate → falls to review (no review roles → execute…)
        assert_ne!(
            derive_station_phase(&us, &roles(&["w"]), &[], &[], Some(false), true),
            StationPhase::Spec
        );
    }

    #[test]
    fn missing_review_is_review_then_manufacture_then_audit_then_checkpoint() {
        let review_roles = roles(&["spec"]);
        let approval_roles = roles(&["user"]);
        let workers = roles(&["make", "resolve"]);

        // 3. No review stamp → Review.
        let mut a = unit("a");
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &approval_roles, Some(true), false
            ),
            StationPhase::Review
        );

        // 4. Review signed, Pass loop not done → Manufacture.
        a.frontmatter.reviews.insert("spec".into(), signed());
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &approval_roles, Some(true), false
            ),
            StationPhase::Manufacture
        );

        // 4b. Last iteration advanced but NOT on the terminal worker → still Manufacture.
        a.frontmatter.iterations.push(UnitIteration {
            worker: "make".into(), result: Some(IterationResult::Advance), ..Default::default()
        });
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &approval_roles, Some(true), false
            ),
            StationPhase::Manufacture
        );

        // 5. Terminal worker advanced, approval missing → Audit.
        a.frontmatter.iterations.push(UnitIteration {
            worker: "resolve".into(), result: Some(IterationResult::Advance), ..Default::default()
        });
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &approval_roles, Some(true), false
            ),
            StationPhase::Audit
        );

        // 6. Approval signed → Checkpoint, and the station is complete.
        a.frontmatter.approvals.insert("user".into(), signed());
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &approval_roles, Some(true), false
            ),
            StationPhase::Checkpoint
        );
        assert!(station_units_complete(
            std::slice::from_ref(&a), &workers, &review_roles, &approval_roles
        ));
    }

    #[test]
    fn station_status_orders_relative_to_active() {
        assert_eq!(station_status(0, Some(2)), Status::Completed);
        assert_eq!(station_status(2, Some(2)), Status::Active);
        assert_eq!(station_status(3, Some(2)), Status::Pending);
        assert_eq!(station_status(0, None), Status::Pending);
    }

    #[test]
    fn pass_loop_done_edge_arms_and_missing_elaboration_is_spec() {
        use crate::domain::IterationResult;
        // A rejected last iteration is not "done".
        let mut a = unit("a");
        a.frontmatter.iterations.push(UnitIteration {
            worker: "make".into(), result: Some(IterationResult::Reject), ..Default::default()
        });
        assert!(!pass_loop_done(&a, &roles(&["make"])));
        // An advance with NO declared workers qualifies (artifact-only stations).
        let mut b = unit("b");
        b.frontmatter.iterations.push(UnitIteration {
            worker: "make".into(), result: Some(IterationResult::Advance), ..Default::default()
        });
        assert!(pass_loop_done(&b, &[]));
        // Elaboration unknown (None) + no units yet → the Spec decompose gate.
        assert_eq!(
            derive_station_phase(&[], &roles(&["make"]), &[], &[], None, false),
            StationPhase::Spec
        );
    }

    /// The derivation reaches Checkpoint on exactly the approval roles it is
    /// given: it has no privileged "user" role. The non-dark Audit wedge (DF-4/
    /// RM-6) came from a caller injecting an approval role that no flow ever
    /// stamps; this pins the pure contract so a signed unit advances on the real
    /// reviewer roles alone. An UNSTAMPED extra role, however, correctly holds at
    /// Audit, proving the wedge is a function of the role LIST, not the module.
    #[test]
    fn checkpoint_needs_only_the_given_approval_roles() {
        use crate::domain::IterationResult;
        let review_roles = roles(&["value", "feasibility"]);
        let workers = roles(&["make"]);
        let mut a = unit("a");
        for r in &review_roles {
            a.frontmatter.reviews.insert(r.clone(), signed());
        }
        a.frontmatter.iterations.push(UnitIteration {
            worker: "make".into(), result: Some(IterationResult::Advance), ..Default::default()
        });
        // Approvals for the REAL reviewer roles → Checkpoint, no phantom needed.
        for r in &review_roles {
            a.frontmatter.approvals.insert(r.clone(), signed());
        }
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &review_roles, Some(true), false
            ),
            StationPhase::Checkpoint
        );
        // Add an approval role nothing stamps and it pins at Audit forever: the
        // exact shape of the shipped wedge, reproduced at the derivation.
        let mut approval_roles = review_roles.clone();
        approval_roles.push("user".into());
        assert_eq!(
            derive_station_phase(
                std::slice::from_ref(&a), &workers, &review_roles, &approval_roles, Some(true), false
            ),
            StationPhase::Audit
        );
    }
}
