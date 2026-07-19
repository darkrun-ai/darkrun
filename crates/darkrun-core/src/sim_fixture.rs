//! The wasm-safe serde schema `darkrun-sim` serializes and `web/site`
//! deserializes: the only shared type surface between the sim (writer) and
//! the site (reader). It lives here, in `darkrun-core`, because this is the
//! crate's established wasm-clean home (its only native dependency is
//! `nix`, gated `[target.'cfg(unix)'.dependencies]`), rather than reusing
//! `darkrun-mcp`'s `TickResult`/`RunAction`/`Position` types, which derive
//! `Serialize` only and whose crate carries unconditional `ureq`, `tokio`,
//! `nix`, and `rmcp` dependencies.
//!
//! Schema is v1 only: no migrator chain, no back-compat shims. A future
//! shape change bumps [`SIM_FIXTURE_SCHEMA_VERSION`] and is the projector's
//! problem to handle on write, not this module's problem to migrate on read.

use serde::{Deserialize, Serialize};

/// The fixture schema's version. Written by the sim's transcript projector
/// on every regenerated fixture; read, not gated, by the site — the site
/// decides what (if anything) to do with a mismatched value. There is no
/// migrator chain: this crate ships v1 only.
pub const SIM_FIXTURE_SCHEMA_VERSION: u32 = 1;

/// A single regenerated, normalized transcript of a scripted darkrun-sim
/// run: the complete data the `/replay` page renders, self-contained and
/// embedded at site build time via `include_str!` (no live engine, no
/// network fetch reachable from the browser).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimFixture {
    pub schema_version: u32,
    pub run_slug: String,
    pub factory: String,
    /// "dark" — the sim's mode is always `Mode::Dark`, carried here as a
    /// plain string rather than `darkrun_core::domain::Mode` so this crate's
    /// wasm-facing schema never has to track that enum's shape.
    pub mode: String,
    pub outcome: FixtureOutcome,
    pub ticks: Vec<FixtureTick>,
    pub events: Vec<FixtureEvent>,
    pub units: Vec<FixtureUnit>,
}

/// The sim's entire red/green verdict vocabulary for a regenerated run.
///
/// Wire form is snake_case, per this crate's established tagged-enum
/// convention (`Status`, `StationPhase`, `Surface` in
/// `crates/darkrun-core/src/domain.rs`): `Sealed` serializes as the bare
/// string `"sealed"`, and `Escalated { reason }` serializes as the
/// externally-tagged object `{"escalated":{"reason":"..."}}`. The sibling
/// sim-spine unit's fixture gate asserts the `"sealed"` literal directly, so
/// this wire form is load-bearing, not incidental.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureOutcome {
    Sealed,
    Escalated { reason: String },
}

/// One projected entry from `action-log.jsonl`: a resolved action the
/// driving loop took, in chronological append order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureTick {
    pub seq: u32,
    /// "run" | "feedback"
    pub track: String,
    pub action_tag: String,
    pub station: Option<String>,
    /// Normalized: RFC3339 timestamps and the minted `verifier_nonce` value
    /// are replaced with fixed placeholder tokens before serialization.
    pub prompt: Option<String>,
}

/// One projected entry from `events.jsonl`, a separate parallel stream from
/// `ticks` (not interleaved 1:1 with it — some events, e.g.
/// `darkrun.run.created`, have no `action-log.jsonl` counterpart).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureEvent {
    pub seq: u32,
    pub event: String,
    /// Normalized: any RFC3339 timestamp value nested inside is replaced
    /// with the fixed placeholder token before serialization.
    pub fields: serde_json::Value,
}

/// One unit's terminal snapshot, captured exactly once after the run's
/// terminal tick (`Sealed` or `Escalated`). Carries only identifiers and the
/// terminal status label — no timestamps, iterations, or body content — so
/// the projector's normalization pass has nothing to strip from it. The
/// `/replay` page's `UnitGraph` renders its nodes from `slug` and its edges
/// from `depends_on` (fb-08 amendment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureUnit {
    pub slug: String,
    pub station: String,
    pub depends_on: Vec<String>,
    /// Terminal `Status` label, e.g. "completed".
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_fixture() -> SimFixture {
        SimFixture {
            schema_version: SIM_FIXTURE_SCHEMA_VERSION,
            run_slug: "darkrun-sim".into(),
            factory: "software".into(),
            mode: "dark".into(),
            outcome: FixtureOutcome::Sealed,
            ticks: vec![
                FixtureTick {
                    seq: 0,
                    track: "run".into(),
                    action_tag: "spec".into(),
                    station: Some("frame".into()),
                    prompt: Some("<normalized>".into()),
                },
                FixtureTick {
                    seq: 1,
                    track: "run".into(),
                    action_tag: "sealed".into(),
                    station: None,
                    prompt: None,
                },
            ],
            events: vec![FixtureEvent {
                seq: 0,
                event: "darkrun.run.created".into(),
                fields: json!({ "run": "darkrun-sim", "nested": { "at": "<normalized>" } }),
            }],
            units: vec![FixtureUnit {
                slug: "fixture-schema".into(),
                station: "build".into(),
                depends_on: vec!["frame".into()],
                status: "completed".into(),
            }],
        }
    }

    /// (a) A fully-populated fixture round-trips through
    /// `serde_json::to_string` then `from_str` and compares equal.
    #[test]
    fn full_fixture_round_trips() {
        let fixture = sample_fixture();
        let serialized = serde_json::to_string(&fixture).expect("serialize");
        let deserialized: SimFixture =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(fixture, deserialized);
    }

    /// (b) `Escalated { reason }` and `Sealed` both round-trip distinctly,
    /// and `Sealed`'s serialized wire form is exactly the JSON string
    /// `"sealed"` — the literal the sibling sim-spine unit's fixture gate
    /// depends on.
    #[test]
    fn outcome_variants_round_trip_and_sealed_wire_form_is_snake_case() {
        let sealed = FixtureOutcome::Sealed;
        let escalated = FixtureOutcome::Escalated {
            reason: "no-progress loop".into(),
        };

        let sealed_json = serde_json::to_string(&sealed).expect("serialize sealed");
        let escalated_json = serde_json::to_string(&escalated).expect("serialize escalated");
        assert_ne!(sealed_json, escalated_json);

        let sealed_back: FixtureOutcome =
            serde_json::from_str(&sealed_json).expect("deserialize sealed");
        let escalated_back: FixtureOutcome =
            serde_json::from_str(&escalated_json).expect("deserialize escalated");
        assert_eq!(sealed_back, sealed);
        assert_eq!(escalated_back, escalated);

        // The wire-form assertion the sibling gate depends on.
        assert_eq!(serde_json::to_value(FixtureOutcome::Sealed).unwrap(), json!("sealed"));
    }

    /// (c) A fixture serialized with a missing optional prompt (`None`)
    /// round-trips.
    #[test]
    fn missing_optional_prompt_round_trips() {
        let tick = FixtureTick {
            seq: 2,
            track: "feedback".into(),
            action_tag: "feedback_question".into(),
            station: Some("build".into()),
            prompt: None,
        };
        let serialized = serde_json::to_string(&tick).expect("serialize");
        let deserialized: FixtureTick = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(tick, deserialized);
        assert!(deserialized.prompt.is_none());
    }

    /// (d) Deserializing JSON whose `schema_version` differs from the crate
    /// constant still parses — the version field is data for the reader to
    /// interpret, not a gate this type enforces.
    #[test]
    fn mismatched_schema_version_still_parses() {
        let mut fixture = sample_fixture();
        fixture.schema_version = SIM_FIXTURE_SCHEMA_VERSION + 41;
        let serialized = serde_json::to_string(&fixture).expect("serialize");
        let deserialized: SimFixture =
            serde_json::from_str(&serialized).expect("deserialize despite version drift");
        assert_eq!(deserialized.schema_version, SIM_FIXTURE_SCHEMA_VERSION + 41);
    }

    /// (e) An empty-units, empty-events fixture round-trips (the degenerate
    /// case).
    #[test]
    fn empty_units_and_events_round_trip() {
        let fixture = SimFixture {
            schema_version: SIM_FIXTURE_SCHEMA_VERSION,
            run_slug: "darkrun-sim".into(),
            factory: "software".into(),
            mode: "dark".into(),
            outcome: FixtureOutcome::Escalated {
                reason: "deadlock guard tripped".into(),
            },
            ticks: vec![],
            events: vec![],
            units: vec![],
        };
        let serialized = serde_json::to_string(&fixture).expect("serialize");
        let deserialized: SimFixture =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(fixture, deserialized);
        assert!(deserialized.ticks.is_empty());
        assert!(deserialized.events.is_empty());
        assert!(deserialized.units.is_empty());
    }

    /// Failure path: `from_str` on malformed (truncated) JSON returns an
    /// `Err`, never a panic.
    #[test]
    fn truncated_json_returns_err_not_panic() {
        let truncated = r#"{"schema_version":1,"run_slug":"darkrun-sim","factory":"#;
        let result: Result<SimFixture, _> = serde_json::from_str(truncated);
        assert!(result.is_err());
    }

    /// Edge: `serde_json::Value` fields with nested objects survive the
    /// round trip byte-for-byte after re-serialization with the same
    /// serializer settings.
    #[test]
    fn nested_value_fields_survive_round_trip_byte_for_byte() {
        let event = FixtureEvent {
            seq: 7,
            event: "darkrun.station.dropped".into(),
            fields: json!({
                "run": "darkrun-sim",
                "station": "build",
                "nested": {
                    "at": "<normalized>",
                    "detail": { "deep": [1, 2, 3], "flag": true, "note": null }
                }
            }),
        };
        let serialized = serde_json::to_string(&event).expect("serialize");
        let deserialized: FixtureEvent = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(event.fields, deserialized.fields);

        // Re-serializing the deserialized value with the same serializer
        // settings must produce the exact same bytes.
        let reserialized = serde_json::to_string(&deserialized).expect("reserialize");
        assert_eq!(serialized, reserialized);
    }
}
