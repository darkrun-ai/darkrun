//! `/replay` — a static, no-live-feed player for the committed darkrun-sim
//! transcript fixture.
//!
//! Renders the ONE committed, Phase-1-produced fixture
//! (`crates/darkrun-sim/fixtures/dark-core.json`) using the shared
//! `darkrun-ui` prelude components — [`StationStrip`], [`StationPipeline`],
//! and [`UnitGraph`] — the same station strip, phase pipeline, and unit
//! dependency graph the desktop app draws. Zero network fetches, no engine
//! process, no `StateStore` reachable from the browser.
//!
//! Modeled on `/preview` (`web/site/src/pages/preview.rs`): a hardcoded /
//! embedded payload, zero fetch, an explicit no-live-feed banner. NEVER
//! modeled on `/browse` (`web/site/src/pages/browse.rs`), whose live,
//! CORS-fetch-driven repo-browsing pattern is the banned shape here.

use darkrun_core::sim_fixture::{FixtureOutcome, FixtureTick, SimFixture};
use darkrun_ui::prelude::*;

use crate::pages::review::ScaffoldNote;
use crate::ui::{theme, SectionHead};

/// The committed, regenerated fixture (Phase 1's scripted `dark-core` run),
/// embedded at compile time — no filesystem read at wasm runtime. Mirrors
/// `web/site/src/content.rs`'s `include_str!` pattern for the markdown
/// corpus. Path is four `..` up from this file (`web/site/src/pages/`) to
/// the workspace root, then down into `crates/darkrun-sim/fixtures/`.
const EMBEDDED_FIXTURE: &str =
    include_str!("../../../../crates/darkrun-sim/fixtures/dark-core.json");

/// The no-live-feed banner sentence, matching `/preview`'s wording precedent
/// (`web/site/src/pages/preview.rs` lines 88-93, the `lead` prop of its
/// `SectionHead`) adapted for the replay surface.
const NO_LIVE_FEED_NOTICE: &str = "Replay only — no live feed is attached.";

/// Parse a fixture JSON string. A parse failure is a legitimate `Err` the
/// caller matches — never an `.unwrap()`/`.expect()` in the render path — so
/// a malformed embedded fixture renders an error state instead of panicking
/// the wasm module.
pub fn parse_fixture(raw: &str) -> Result<SimFixture, serde_json::Error> {
    serde_json::from_str(raw)
}

/// `/replay` — parses the embedded fixture once and renders it, or an error
/// state if the embedded JSON fails to parse.
#[component]
pub fn Replay() -> Element {
    match parse_fixture(EMBEDDED_FIXTURE) {
        Ok(fixture) => render_fixture(&fixture),
        Err(err) => rsx! {
            SectionHead {
                kicker: "fixture".to_string(),
                title: "Replay".to_string(),
                lead: Some(NO_LIVE_FEED_NOTICE.to_string()),
            }
            div {
                "data-fixture-error": "true",
                style: format!(
                    "border:1px dashed {border};border-radius:8px;padding:12px 14px;\
                     font-family:{mono};font-size:13px;color:{danger};",
                    border = theme::BORDER_STRONG,
                    mono = tokens::FONT_MONO,
                    danger = tokens::var::STATUS_DANGER,
                ),
                "The embedded transcript fixture failed to parse: {err}"
            }
        },
    }
}

/// Render the parsed fixture: the assembly-line station strip, the current
/// station's phase pipeline, the unit dependency graph, and the raw tick
/// transcript, all derived from fixture data alone.
fn render_fixture(fixture: &SimFixture) -> Element {
    let stations = derive_stations(fixture);
    let active_phase = derive_active_phase(fixture);
    let dots = strip_for(active_phase);
    let (units, edges) = derive_graph(fixture);
    let outcome_label = match &fixture.outcome {
        FixtureOutcome::Sealed => "sealed".to_string(),
        FixtureOutcome::Escalated { reason } => format!("escalated — {reason}"),
    };
    let lead = format!(
        "A static replay of the committed darkrun-sim transcript for run `{}` (factory `{}`, \
         mode `{}`, outcome `{outcome_label}`), rendered entirely from the embedded, \
         regenerated fixture. {NO_LIVE_FEED_NOTICE}",
        fixture.run_slug, fixture.factory, fixture.mode,
    );
    let h2_style =
        format!("font-family:{};font-size:18px;color:{};margin:0 0 12px;", tokens::FONT_SANS, theme::TEXT);

    rsx! {
        SectionHead {
            kicker: "fixture".to_string(),
            title: "Replay".to_string(),
            lead: Some(lead),
        }

        ScaffoldNote {
            text: "Fixture: the committed, normalized `dark-core` transcript \
                   (`crates/darkrun-sim/fixtures/dark-core.json`), embedded at compile time and \
                   rendered read-only. No engine process, no MCP server, no fetch of any kind."
                .to_string(),
        }

        div { style: "display:flex;flex-direction:column;gap:32px;margin-top:8px;",
            section {
                "data-fixture": "stations",
                h2 { style: "{h2_style}", "Stations" }
                StationStrip { stations }
            }

            section {
                "data-fixture": "pipeline",
                h2 { style: "{h2_style}", "Current station phase" }
                StationPipeline { dots, labels: true }
            }

            section {
                "data-fixture": "graph",
                h2 { style: "{h2_style}", "Unit dependency graph" }
                UnitGraph { units, edges }
            }

            section {
                "data-fixture": "ticks",
                h2 { style: "{h2_style}", "Transcript" }
                div { style: "display:flex;flex-direction:column;gap:8px;",
                    for tick in fixture.ticks.iter() {
                        {render_tick(tick)}
                    }
                }
            }
        }
    }
}

/// One collapsed transcript row: `seq`, `track`, `action_tag`, `station`,
/// and the normalized prompt in a collapsed `<details>`/`<pre>` block so the
/// full transcript is inspectable without flooding the page.
fn render_tick(tick: &FixtureTick) -> Element {
    let station_label = tick.station.clone().unwrap_or_else(|| "—".to_string());
    let row_style = format!("border:1px solid {border};border-radius:6px;padding:8px 10px;", border = theme::BORDER);
    let meta_style = format!(
        "font-family:{mono};font-size:12px;color:{muted};display:flex;gap:12px;flex-wrap:wrap;",
        mono = tokens::FONT_MONO,
        muted = theme::TEXT_MUTED,
    );
    let summary_style = format!(
        "font-family:{mono};font-size:12px;color:{accent};cursor:pointer;margin-top:6px;",
        mono = tokens::FONT_MONO,
        accent = theme::ACCENT,
    );
    let pre_style = format!(
        "font-family:{mono};font-size:12px;color:{text};white-space:pre-wrap;\
         margin:8px 0 0;max-height:280px;overflow:auto;",
        mono = tokens::FONT_MONO,
        text = theme::TEXT,
    );

    rsx! {
        div {
            class: "dr-replay-tick",
            "data-seq": "{tick.seq}",
            "data-action-tag": "{tick.action_tag}",
            style: "{row_style}",
            div { style: "{meta_style}",
                span { "seq {tick.seq}" }
                span { "track {tick.track}" }
                span { "action {tick.action_tag}" }
                span { "station {station_label}" }
            }
            if let Some(prompt) = &tick.prompt {
                details {
                    summary { style: "{summary_style}", "prompt" }
                    pre { style: "{pre_style}", "{prompt}" }
                }
            }
        }
    }
}

/// Every distinct station named across the fixture's ticks, in first-seen
/// order, marked `Done` for every station before the last and `Current` for
/// the last — the run's assembly-line position at the moment the transcript
/// was captured. A station name is rendered EXACTLY as the fixture spells
/// it, never resolved against the live `darkrun_content` factory corpus —
/// so a later rename or removal in that corpus can never break this page
/// (per the spec's "fixture referencing content the site no longer embeds"
/// edge case). An empty tick list derives an empty strip, not a panic.
pub fn derive_stations(fixture: &SimFixture) -> Vec<StationItem> {
    let mut names: Vec<String> = Vec::new();
    for tick in &fixture.ticks {
        if let Some(station) = &tick.station {
            if !names.iter().any(|n| n == station) {
                names.push(station.clone());
            }
        }
    }
    let last_idx = names.len().checked_sub(1);
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let status = if Some(i) == last_idx { StationStatus::Current } else { StationStatus::Done };
            StationItem::new(name, status)
        })
        .collect()
}

/// The [`Phase`] the pipeline strip highlights: the LAST tick whose
/// `action_tag` names a phase (`spec`/`review`/`manufacture`/`audit`/
/// `reflect`/`checkpoint`), skipping non-phase terminal tags such as the
/// fixture's own final `"sealed"` tick. `None` when no tick names a phase
/// (an empty transcript, or one that never reached a station), which
/// renders every pipeline dot pending rather than panicking. This is the
/// `RunAction`-to-`Phase` translation the spec requires to live in
/// `web/site`, since `darkrun-ui` stays `darkrun-core`-free by design.
pub fn derive_active_phase(fixture: &SimFixture) -> Option<Phase> {
    fixture.ticks.iter().rev().find_map(|tick| Phase::from_name(&tick.action_tag))
}

/// The unit dependency graph derived from the fixture's `units` field (the
/// fb-08 amendment): one [`UnitGraphNode`] per `FixtureUnit.slug`, one
/// [`GraphEdge`] per `depends_on` entry — `from` is the dependency, `to` is
/// the dependent unit, matching [`UnitGraph`]'s own edge convention
/// (`crates/darkrun-ui/src/graph/view.rs`). An empty `units` list yields
/// empty nodes and edges, never a panic.
pub fn derive_graph(fixture: &SimFixture) -> (Vec<UnitGraphNode>, Vec<GraphEdge>) {
    let nodes = fixture.units.iter().map(|u| UnitGraphNode::new(u.slug.clone(), u.slug.clone())).collect();
    let mut edges = Vec::new();
    for unit in &fixture.units {
        for dep in &unit.depends_on {
            edges.push(GraphEdge::new(dep.clone(), unit.slug.clone()));
        }
    }
    (nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkrun_core::sim_fixture::{FixtureUnit, SIM_FIXTURE_SCHEMA_VERSION};

    fn unit(slug: &str, station: &str, deps: &[&str]) -> FixtureUnit {
        FixtureUnit {
            slug: slug.to_string(),
            station: station.to_string(),
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            status: "completed".to_string(),
        }
    }

    fn tick(seq: u32, action_tag: &str, station: Option<&str>) -> FixtureTick {
        FixtureTick {
            seq,
            track: "run".to_string(),
            action_tag: action_tag.to_string(),
            station: station.map(|s| s.to_string()),
            prompt: Some("<normalized>".to_string()),
        }
    }

    fn base_fixture() -> SimFixture {
        SimFixture {
            schema_version: SIM_FIXTURE_SCHEMA_VERSION,
            run_slug: "dark-core".to_string(),
            factory: "software".to_string(),
            mode: "dark".to_string(),
            outcome: FixtureOutcome::Sealed,
            ticks: vec![],
            events: vec![],
            units: vec![],
        }
    }

    /// The real embedded fixture (the committed `dark-core.json`) parses and
    /// carries the shapes the unit spec names: schema v1, 37 ticks, 6
    /// units, a sealed outcome.
    #[test]
    fn embedded_fixture_parses_with_expected_shape() {
        let fixture = parse_fixture(EMBEDDED_FIXTURE).expect("embedded fixture parses");
        assert_eq!(fixture.schema_version, SIM_FIXTURE_SCHEMA_VERSION);
        assert_eq!(fixture.ticks.len(), 37);
        assert_eq!(fixture.units.len(), 6);
        assert_eq!(fixture.outcome, FixtureOutcome::Sealed);
    }

    /// `derive_stations` walks the embedded fixture's ticks in order,
    /// dedupes to the six distinct stations in tick order, and marks every
    /// station but the last (`harden`) `Done`, the last `Current`.
    #[test]
    fn derive_stations_marks_all_but_last_done() {
        let fixture = parse_fixture(EMBEDDED_FIXTURE).expect("embedded fixture parses");
        let stations = derive_stations(&fixture);
        let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["frame", "specify", "shape", "build", "prove", "harden"]);
        for st in &stations[..stations.len() - 1] {
            assert_eq!(st.status, StationStatus::Done, "{} should be done", st.name);
        }
        assert_eq!(stations.last().unwrap().status, StationStatus::Current);
    }

    /// A station name the site's embedded factory content no longer knows
    /// is rendered as plain text, verbatim — never resolved against the
    /// content corpus.
    #[test]
    fn derive_stations_passes_unknown_station_names_through_verbatim() {
        let mut fixture = base_fixture();
        fixture.ticks = vec![tick(0, "spec", Some("not-a-real-station"))];
        let stations = derive_stations(&fixture);
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].name, "not-a-real-station");
        assert_eq!(stations[0].status, StationStatus::Current);
    }

    /// An empty tick list derives no stations, without panicking.
    #[test]
    fn derive_stations_empty_ticks_is_empty() {
        let fixture = base_fixture();
        assert!(derive_stations(&fixture).is_empty());
    }

    /// The active phase is the LAST tick whose `action_tag` names a phase:
    /// the embedded fixture's terminal `"sealed"` tag is skipped, so the
    /// pipeline highlights `checkpoint` (the harden station's last phase),
    /// not `None`.
    #[test]
    fn derive_active_phase_skips_the_terminal_sealed_tag() {
        let fixture = parse_fixture(EMBEDDED_FIXTURE).expect("embedded fixture parses");
        assert_eq!(derive_active_phase(&fixture), Some(Phase::Checkpoint));
    }

    /// No phase-bearing tick yields `None` (every pipeline dot pending),
    /// never a panic.
    #[test]
    fn derive_active_phase_none_when_no_tick_names_a_phase() {
        let mut fixture = base_fixture();
        fixture.ticks = vec![tick(0, "sealed", None)];
        assert_eq!(derive_active_phase(&fixture), None);
    }

    /// `derive_graph` builds one node per unit and one edge per
    /// `depends_on` entry, `from` the dependency and `to` the dependent
    /// (matching `UnitGraph`'s own edge convention).
    #[test]
    fn derive_graph_builds_nodes_and_edges_from_depends_on() {
        let mut fixture = base_fixture();
        fixture.units = vec![unit("frame-unit", "frame", &[]), unit("build-unit", "build", &["frame-unit"])];
        let (nodes, edges) = derive_graph(&fixture);
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().any(|n| n.node.id == "frame-unit"));
        assert!(nodes.iter().any(|n| n.node.id == "build-unit"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, "frame-unit");
        assert_eq!(edges[0].to, "build-unit");
    }

    /// Edge: an empty `units` list renders an empty graph state — no nodes,
    /// no edges, no panic.
    #[test]
    fn derive_graph_empty_units_is_empty_graph() {
        let fixture = base_fixture();
        let (nodes, edges) = derive_graph(&fixture);
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }

    /// The embedded fixture's own `units` field (every unit's `depends_on`
    /// is empty) still derives six real nodes with zero edges — the
    /// degenerate case as it actually occurs in the committed fixture, not
    /// a fabricated one.
    #[test]
    fn derive_graph_on_embedded_fixture_has_nodes_and_no_edges() {
        let fixture = parse_fixture(EMBEDDED_FIXTURE).expect("embedded fixture parses");
        let (nodes, edges) = derive_graph(&fixture);
        assert_eq!(nodes.len(), 6);
        assert!(edges.is_empty());
    }

    /// Failure path: a truncated copy of the embedded fixture JSON fails to
    /// parse — `parse_fixture` returns `Err`, never panics — proving the
    /// component's `match` reaches its error arm on a malformed fixture
    /// instead of unwrapping.
    #[test]
    fn truncated_embedded_json_returns_err_not_panic() {
        // Cut at the nearest UTF-8 char boundary at or before the midpoint —
        // the embedded prompt text carries multi-byte glyphs (e.g. `—`), and
        // a raw byte-index slice could otherwise land mid-character.
        let mut cut = EMBEDDED_FIXTURE.len() / 2;
        while !EMBEDDED_FIXTURE.is_char_boundary(cut) {
            cut -= 1;
        }
        let truncated = &EMBEDDED_FIXTURE[..cut];
        let result = parse_fixture(truncated);
        assert!(result.is_err(), "truncated JSON must fail to parse, not panic");
    }

    /// A minimal hand-truncated fixture (mirrors
    /// `crates/darkrun-core/src/sim_fixture.rs`'s own truncated-JSON test)
    /// also fails to parse rather than panicking.
    #[test]
    fn hand_truncated_json_returns_err() {
        let truncated = r#"{"schema_version":1,"run_slug":"dark-core","factory":"#;
        let result = parse_fixture(truncated);
        assert!(result.is_err());
    }
}
