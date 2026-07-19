//! The transcript projector: turn a driven run into a normalized, self-contained
//! [`SimFixture`] — the exact JSON the `/replay` page renders.
//!
//! The fixture is a projection over the three streams the engine already
//! persists, merged under one ordering rule:
//!
//! 1. **`ticks`** come from `action-log.jsonl` (`{at, track, action, station}`,
//!    one line per resolved action, in append order). Each tick's `prompt` is
//!    filled from the SAME tick's IN-MEMORY capture (aligned by order), never
//!    re-read from `.darkrun/<slug>/prompts/<scope>/<tag>.md` — that path is
//!    overwrite-on-reuse, so a recurring tag would read the clobbered text.
//! 2. **`events`** are a separate, parallel projection of `events.jsonl`
//!    (`{at, event, run, ...}`), not interleaved 1:1 with ticks.
//! 3. **`units`** are captured once, after the terminal tick, via `read_units`
//!    (slug / station / depends_on / terminal status only).
//!
//! Three normalization rules run before serialization (Contract 3): every
//! RFC3339 timestamp becomes `"<normalized>"`, every minted `verifier_nonce`
//! value becomes `<nonce>`, and `deadlock.json` is never embedded at all (the
//! deadlock outcome is carried only as `FixtureOutcome::Escalated`).

use darkrun_core::domain::Status;
use darkrun_core::sim_fixture::{
    FixtureEvent, FixtureOutcome, FixtureTick, FixtureUnit, SimFixture, SIM_FIXTURE_SCHEMA_VERSION,
};
use darkrun_core::StateStore;
use serde_json::Value;
use std::collections::BTreeSet;

use crate::world::{DriveResult, WorldOutcome};

/// Project a driven run into its normalized [`SimFixture`].
pub fn project(store: &StateStore, slug: &str, drive: &DriveResult) -> SimFixture {
    let run = store.read_run(slug).expect("read_run for projection");
    let factory = run.frontmatter.factory.clone();
    let repo_root = store
        .root()
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nonces = &drive.nonces;

    // 1. ticks: one FixtureTick per action-log line, prompt from the in-memory
    //    capture aligned by ORDER (never the clobberable on-disk file).
    let action_log = store.read_journal(slug, "action-log.jsonl");
    let ticks: Vec<FixtureTick> = action_log
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let v: Value = serde_json::from_str(line).expect("action-log entry is json");
            let prompt = drive
                .prompts
                .get(i)
                .cloned()
                .flatten()
                .map(|p| normalize_text(&p, nonces, &repo_root));
            FixtureTick {
                seq: i as u32,
                track: v["track"].as_str().unwrap_or("run").to_string(),
                action_tag: v["action"].as_str().unwrap_or_default().to_string(),
                station: v["station"].as_str().map(str::to_string),
                prompt,
            }
        })
        .collect();

    // 2. events: a parallel projection of events.jsonl, normalized recursively.
    let events_log = store.read_journal(slug, "events.jsonl");
    let events: Vec<FixtureEvent> = events_log
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let mut v: Value = serde_json::from_str(line).expect("events entry is json");
            let event = v["event"].as_str().unwrap_or_default().to_string();
            let mut fields = v.take();
            if let Some(obj) = fields.as_object_mut() {
                obj.remove("event");
            }
            normalize_value(&mut fields, nonces, &repo_root);
            FixtureEvent {
                seq: i as u32,
                event,
                fields,
            }
        })
        .collect();

    // 3. units: captured once after the terminal tick — identifiers + terminal
    //    status only (nothing normalization must strip).
    let units: Vec<FixtureUnit> = store
        .read_units(slug)
        .unwrap_or_default()
        .iter()
        .map(|u| FixtureUnit {
            slug: u.slug.clone(),
            station: u.station().to_string(),
            depends_on: u.frontmatter.depends_on.clone(),
            status: status_label(u.status()),
        })
        .collect();

    let outcome = match &drive.outcome {
        WorldOutcome::Sealed => FixtureOutcome::Sealed,
        WorldOutcome::Escalated { reason } => FixtureOutcome::Escalated {
            reason: normalize_text(reason, nonces, &repo_root),
        },
    };

    SimFixture {
        schema_version: SIM_FIXTURE_SCHEMA_VERSION,
        run_slug: slug.to_string(),
        factory,
        mode: "dark".to_string(),
        outcome,
        ticks,
        events,
        units,
    }
}

/// The snake_case terminal [`Status`] label carried in the fixture.
fn status_label(status: Status) -> String {
    match status {
        Status::Pending => "pending",
        Status::Active => "active",
        Status::InProgress => "in_progress",
        Status::Completed => "completed",
        Status::Blocked => "blocked",
    }
    .to_string()
}

/// Apply normalization rules 1 + 2 to a string: minted nonce values become
/// `<nonce>`, RFC3339 timestamps become `<normalized>`. The tempdir absolute
/// path is also collapsed to `<root>` so two runs in different tempdirs project
/// byte-identically.
fn normalize_text(text: &str, nonces: &BTreeSet<String>, repo_root: &str) -> String {
    let mut out = text.to_string();
    for n in nonces {
        if !n.is_empty() {
            out = out.replace(n.as_str(), "<nonce>");
        }
    }
    if !repo_root.is_empty() {
        out = out.replace(repo_root, "<root>");
    }
    replace_rfc3339(&out)
}

/// Recursively normalize every string leaf inside a JSON value.
fn normalize_value(value: &mut Value, nonces: &BTreeSet<String>, repo_root: &str) {
    match value {
        Value::String(s) => {
            *s = normalize_text(s, nonces, repo_root);
        }
        Value::Array(items) => {
            for item in items {
                normalize_value(item, nonces, repo_root);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                normalize_value(v, nonces, repo_root);
            }
        }
        _ => {}
    }
}

/// Replace every RFC3339 timestamp token in `text` with `<normalized>`.
fn replace_rfc3339(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some(len) = rfc3339_match_len(&text[i..]) {
            out.push_str("<normalized>");
            i += len;
        } else {
            let ch = text[i..].chars().next().expect("char at boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// The byte length of an RFC3339 timestamp at the START of `s`, or `None`. Matches
/// `YYYY-MM-DDThh:mm:ss` plus an optional `.fraction` and an optional `Z` / `±hh:mm`
/// offset — the shape `Utc::now().to_rfc3339()` and the frontmatter timestamps emit.
fn rfc3339_match_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let digit = |k: usize| b[k].is_ascii_digit();
    let ok = digit(0)
        && digit(1)
        && digit(2)
        && digit(3)
        && b[4] == b'-'
        && digit(5)
        && digit(6)
        && b[7] == b'-'
        && digit(8)
        && digit(9)
        && (b[10] == b'T' || b[10] == b't')
        && digit(11)
        && digit(12)
        && b[13] == b':'
        && digit(14)
        && digit(15)
        && b[16] == b':'
        && digit(17)
        && digit(18);
    if !ok {
        return None;
    }
    let mut len = 19;
    // Optional fractional seconds.
    if len < b.len() && b[len] == b'.' {
        let mut k = len + 1;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > len + 1 {
            len = k;
        }
    }
    // Optional timezone: Z / z, or ±hh:mm.
    if len < b.len() {
        match b[len] {
            b'Z' | b'z' => len += 1,
            b'+' | b'-'
                if len + 6 <= b.len()
                    && b[len + 1].is_ascii_digit()
                    && b[len + 2].is_ascii_digit()
                    && b[len + 3] == b':'
                    && b[len + 4].is_ascii_digit()
                    && b[len + 5].is_ascii_digit() =>
            {
                len += 6;
            }
            _ => {}
        }
    }
    Some(len)
}

/// Whether `text` contains an RFC3339 timestamp anywhere (the AC-7 observable).
#[cfg(test)]
fn contains_rfc3339(text: &str) -> bool {
    let mut i = 0;
    while i < text.len() {
        if rfc3339_match_len(&text[i..]).is_some() {
            return true;
        }
        i += text[i..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    false
}

#[cfg(test)]
mod fixture {
    use super::*;
    use crate::provider::{Provider, ProviderMove, ScriptedProvider};
    use crate::world::{dark_core_script, DriveResult, NoopHosting, World, WorldOutcome};
    use darkrun_core::domain::{StationPhase, Unit, UnitFrontmatter};
    use darkrun_mcp::position::run_tick_with_hosting;

    /// Regenerate the default dark scenario and its fixture in a fresh tempdir.
    fn regenerate(slug: &str) -> (World, SimFixture) {
        let world = World::new(slug, "software");
        let drive = world.drive(&mut ScriptedProvider::new(dark_core_script()));
        let fixture = project(&world.store, &world.slug, &drive);
        (world, fixture)
    }

    /// AC-8 (named): regenerating the scenario twice in independent tempdirs
    /// yields byte-identical serialized fixtures after normalization.
    #[test]
    fn regenerate_twice_is_byte_equal() {
        let (_w1, a) = regenerate("dark-core");
        let (_w2, b) = regenerate("dark-core");
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "two independent regenerations must serialize byte-identically"
        );
    }

    /// AC-15 (named): the committed fixture equals a fresh regeneration.
    #[test]
    fn committed_fixture_matches_regeneration() {
        let (_w, regenerated) = regenerate("dark-core");
        let committed: SimFixture =
            serde_json::from_str(include_str!("../fixtures/dark-core.json"))
                .expect("committed dark-core.json parses");
        assert_eq!(
            regenerated, committed,
            "the committed fixture is stale — regenerate it from the scripted scenario"
        );
    }

    /// AC-6: `ticks.len()` equals the run's `action-log.jsonl` line count, and
    /// `events.len()` equals its `events.jsonl` line count.
    #[test]
    fn stream_lengths_match_the_journals() {
        let (world, fixture) = regenerate("dark-lengths");
        let action_log = world.store.read_journal(&world.slug, "action-log.jsonl");
        let events_log = world.store.read_journal(&world.slug, "events.jsonl");
        assert_eq!(fixture.ticks.len(), action_log.len(), "ticks vs action-log");
        assert_eq!(fixture.events.len(), events_log.len(), "events vs events.jsonl");
        assert!(!fixture.ticks.is_empty() && !fixture.units.is_empty());
    }

    /// Edge (`render_prompt` None): no captured prompt is ever `None` for a
    /// non-terminal action tag — a missing prompt would be a corpus regression.
    #[test]
    fn no_captured_prompt_is_none_for_a_nonterminal_tag() {
        let (_w, fixture) = regenerate("dark-prompts");
        for t in &fixture.ticks {
            let terminal = matches!(t.action_tag.as_str(), "sealed" | "pending_seal" | "noop");
            if !terminal {
                assert!(
                    t.prompt.is_some(),
                    "non-terminal tick `{}` captured a None prompt",
                    t.action_tag
                );
            }
        }
    }

    /// AC-7: the minted `verifier_nonce` is replaced with the `<nonce>`
    /// placeholder, and no raw nonce value survives into the serialized fixture.
    #[test]
    fn nonce_is_replaced_with_placeholder() {
        let world = World::new("dark-nonce", "software");
        let drive = world.drive(&mut ScriptedProvider::new(dark_core_script()));
        let fixture = project(&world.store, &world.slug, &drive);
        assert!(
            !drive.nonces.is_empty(),
            "the run minted at least one verifier nonce"
        );
        let serialized = serde_json::to_string(&fixture).unwrap();
        for n in &drive.nonces {
            assert!(
                !serialized.contains(n.as_str()),
                "a raw verifier nonce survived normalization"
            );
        }
        assert!(
            fixture
                .ticks
                .iter()
                .any(|t| t.prompt.as_deref().map(|p| p.contains("<nonce>")).unwrap_or(false)),
            "no captured prompt carries the <nonce> placeholder — the nonce was not embedded"
        );
    }

    /// AC-7: no RFC3339 timestamp survives into the serialized fixture.
    #[test]
    fn no_rfc3339_timestamp_survives() {
        let (_w, fixture) = regenerate("dark-timestamps");
        let serialized = serde_json::to_string(&fixture).unwrap();
        assert!(
            !contains_rfc3339(&serialized),
            "an RFC3339 timestamp survived normalization"
        );
    }

    /// AC-6 (in-memory capture, not the clobberable on-disk file): the projector
    /// never reads `prompts/<scope>/<tag>.md`. Clobbering every on-disk prompt
    /// file with a sentinel and re-projecting leaves the fixture prompts
    /// untouched — proving the prompt text came from the in-memory capture.
    #[test]
    fn projector_reads_in_memory_capture_not_the_clobbered_files() {
        let world = World::new("dark-inmem", "software");
        let drive = world.drive(&mut ScriptedProvider::new(dark_core_script()));
        clobber_prompt_files(&world.store, &world.slug, "CLOBBERED-SENTINEL");
        let fixture = project(&world.store, &world.slug, &drive);
        for t in &fixture.ticks {
            if let Some(p) = &t.prompt {
                assert!(
                    !p.contains("CLOBBERED-SENTINEL"),
                    "a prompt was read from the clobbered on-disk file, not the in-memory capture"
                );
            }
        }
    }

    /// Overwrite every persisted prompt file under `prompts/` with `sentinel`.
    fn clobber_prompt_files(store: &StateStore, slug: &str, sentinel: &str) {
        let prompts_dir = store.run_dir(slug).join("prompts");
        clobber_recursive(&prompts_dir, sentinel);
    }

    fn clobber_recursive(dir: &std::path::Path, sentinel: &str) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                clobber_recursive(&path, sentinel);
            } else {
                let _ = std::fs::write(&path, sentinel);
            }
        }
    }

    /// The RFC3339 replacer collapses timestamps but leaves surrounding text —
    /// including the fraction + offset forms — intact.
    #[test]
    fn rfc3339_replacer_is_precise() {
        assert_eq!(
            replace_rfc3339("at 2026-07-19T10:11:12Z ok"),
            "at <normalized> ok"
        );
        assert_eq!(
            replace_rfc3339("2026-07-19T10:11:12.345678+00:00"),
            "<normalized>"
        );
        assert_eq!(replace_rfc3339("no timestamp here"), "no timestamp here");
        // A partial / malformed date is left alone.
        assert_eq!(replace_rfc3339("2026-07-19 plain"), "2026-07-19 plain");
    }

    /// Edge (an empty `prompts/` directory for an unreached station): an
    /// escalated run reaches only its first station, so later stations never get
    /// a `prompts/<station>/` directory at all. The projector must not panic and
    /// must bound `ticks` EXACTLY by the run's `action-log.jsonl` line count —
    /// never padding a placeholder tick for a station the run did not reach — and
    /// the outcome is `Escalated`.
    #[test]
    fn escalate_run_projects_without_padding_unreached_stations() {
        struct AlwaysAdvance;
        impl Provider for AlwaysAdvance {
            fn next_move(&mut self, _prompt: Option<&str>) -> ProviderMove {
                ProviderMove::AdvanceStation
            }
        }
        let world = World::new("dark-escalate-proj", "software");
        let drive = world.drive(&mut AlwaysAdvance);
        // (a) no panic:
        let fixture = project(&world.store, &world.slug, &drive);
        // (c) the outcome is Escalated.
        assert!(
            matches!(fixture.outcome, FixtureOutcome::Escalated { .. }),
            "an induced no-progress loop projects as Escalated"
        );
        // (b) ticks are bounded exactly by the action-log — no padding.
        let action_log = world.store.read_journal(&world.slug, "action-log.jsonl");
        assert_eq!(
            fixture.ticks.len(),
            action_log.len(),
            "ticks must equal the action-log line count, never padded for unreached stations"
        );
        // The run only reached `frame`; a later station has no prompts dir at all,
        // and the fixture carries no tick for it.
        let prompts_root = world.store.run_dir(&world.slug).join("prompts");
        assert!(
            !prompts_root.join("specify").exists(),
            "an unreached station never gets a prompts/ directory"
        );
        assert!(
            !fixture.ticks.iter().any(|t| t.station.as_deref() == Some("specify")),
            "no tick is projected for the unreached `specify` station"
        );
    }

    /// Edge (a stale-content fixture): the committed fixture is fully
    /// self-contained — every display string the replay page needs is embedded
    /// as literal text, so a later edit to the live factory corpus can never
    /// break `/replay`. A real yes/no check: parse it standalone, and assert
    /// every prompt is either absent or FULLY RENDERED literal text carrying no
    /// unresolved template delimiter that would require the live corpus to
    /// resolve at render time.
    #[test]
    fn committed_fixture_is_self_contained() {
        let fixture: SimFixture =
            serde_json::from_str(include_str!("../fixtures/dark-core.json"))
                .expect("committed fixture parses standalone");
        assert!(!fixture.factory.is_empty(), "factory title is embedded literally");
        assert!(!fixture.run_slug.is_empty(), "run slug is embedded literally");
        assert!(!fixture.units.is_empty(), "unit labels are embedded");
        for u in &fixture.units {
            assert!(
                !u.slug.is_empty() && !u.station.is_empty(),
                "every unit carries literal slug + station labels"
            );
        }
        for t in &fixture.ticks {
            assert!(!t.action_tag.is_empty(), "every tick carries a literal action tag");
            if let Some(prompt) = &t.prompt {
                assert!(!prompt.is_empty(), "a present prompt is non-empty literal text");
                for marker in ["{{", "}}", "{%", "%}"] {
                    assert!(
                        !prompt.contains(marker),
                        "tick `{}` carries an unrendered template marker `{marker}` — not self-contained",
                        t.action_tag
                    );
                }
            }
        }
    }

    /// AC-6 (second test): the same `(action_tag, station)` pair on two different
    /// ticks with DIFFERENT prompt text — both survive at their own tick index.
    /// The engine writes each tick's prompt to `prompts/<station>/<tag>.md`, so a
    /// recurring tag clobbers the on-disk file; both distinct prompts survive
    /// ONLY because the projector reads the in-memory capture, never that file.
    #[test]
    fn distinct_prompts_for_a_recurring_tag_station_survive() {
        let world = World::new("dark-recurring-spec", "software");
        let mut prompts: Vec<Option<String>> = Vec::new();

        // Tick 1: spec@frame with ZERO units.
        let t1 = run_tick_with_hosting(&world.store, &world.slug, &NoopHosting).expect("tick 1");
        prompts.push(t1.prompt.clone());

        // Honest state manipulation: re-open frame's Spec phase and give it a
        // unit, so the SAME spec@frame renders again with DIFFERENT content.
        {
            let mut state = world.store.read_state(&world.slug).unwrap().unwrap();
            if let Some(st) = state.stations.get_mut("frame") {
                st.phase = StationPhase::Spec;
            }
            world.store.write_state(&world.slug, &state).unwrap();
            let unit = Unit {
                slug: "frame-extra".into(),
                frontmatter: UnitFrontmatter {
                    status: darkrun_core::domain::Status::Pending,
                    station: Some("frame".into()),
                    ..Default::default()
                },
                title: "frame-extra".into(),
                body: String::new(),
            };
            world.store.write_unit(&world.slug, &unit).unwrap();
        }

        // Tick 2: spec@frame again, now with a unit on record.
        let t2 = run_tick_with_hosting(&world.store, &world.slug, &NoopHosting).expect("tick 2");
        prompts.push(t2.prompt.clone());

        let drive = DriveResult {
            prompts,
            moves: Vec::new(),
            outcome: WorldOutcome::Sealed,
            nonces: BTreeSet::new(),
        };
        let fixture = project(&world.store, &world.slug, &drive);

        assert!(fixture.ticks.len() >= 2, "at least the two spec@frame ticks");
        for i in 0..2 {
            assert_eq!(fixture.ticks[i].action_tag, "spec", "tick {i} is spec");
            assert_eq!(
                fixture.ticks[i].station.as_deref(),
                Some("frame"),
                "tick {i} targets frame"
            );
        }
        let p0 = fixture.ticks[0].prompt.as_deref().expect("prompt 0");
        let p1 = fixture.ticks[1].prompt.as_deref().expect("prompt 1");
        assert_ne!(
            p0, p1,
            "the two spec@frame ticks must carry distinct prompt text"
        );

        // The on-disk prompts/frame/spec.md holds only the LAST-written (tick 2)
        // prompt — tick 1's distinct prompt survived only from the in-memory
        // capture, never from this clobbered file.
        let on_disk = std::fs::read_to_string(
            world
                .store
                .run_dir(&world.slug)
                .join("prompts")
                .join("frame")
                .join("spec.md"),
        )
        .expect("on-disk spec prompt exists");
        let repo_root = world
            .store
            .root()
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let on_disk_normalized = normalize_text(&on_disk, &BTreeSet::new(), &repo_root);
        assert_eq!(
            p1, on_disk_normalized,
            "tick 1 matches the last-written on-disk file"
        );
        assert_ne!(
            p0, on_disk_normalized,
            "tick 0's distinct prompt could not have come from the clobbered on-disk file"
        );
    }

    /// Regenerate and write the committed `fixtures/dark-core.json`. Ignored by
    /// default; run explicitly to refresh the committed copy:
    /// `cargo test -p darkrun-sim fixture::write_committed_fixture -- --ignored`.
    #[test]
    #[ignore]
    fn write_committed_fixture() {
        let (_w, fixture) = regenerate("dark-core");
        let pretty = serde_json::to_string_pretty(&fixture).expect("serialize fixture");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/dark-core.json");
        std::fs::write(path, format!("{pretty}\n")).expect("write committed fixture");
    }
}
