//! demo_engine — seed a rich, deterministic demo project and serve it.
//!
//! The website's desktop screenshots are captured against THIS harness so a
//! refresh is reproducible instead of ad hoc:
//!
//! ```sh
//! cargo run -p darkrun-mcp --example demo_engine          # port 7910
//! DARKRUN_PORT=7910 DARKRUN_SESSION_ID=checkout-flow cargo run -p darkrun-desktop
//! ```
//!
//! Seeded surfaces (session ids):
//! - `checkout-flow`  — a build station mid-Manufacture: unit DAG with mixed
//!   beat states, completed upstream stations, an open feedback with a text
//!   annotation anchor.
//! - `billing-portal` — a station parked at its undecided Checkpoint: the
//!   Approve / Request-changes bar.
//! - `d-01`           — an awaiting design-direction pick (three themed
//!   archetype mockups, dark + light SVG pairs).
//! - `d-02`           — the chosen direction annotated: pins + comments.
//! - unpinned launch  — the projects home (the demo project registered).
//!
//! Everything is written to `/tmp/darkrun-demo` (recreated each boot) and the
//! engine announces itself in the discovery registry so the home surface lists
//! it. Ctrl-C to stop; the registry entry can be pruned afterwards.

use std::net::SocketAddr;
use std::path::Path;

use darkrun_core::domain::{
    Checkpoint, CheckpointKind, IterationResult, Mode, Station, StationPhase, Status, Unit,
    UnitFrontmatter, UnitIteration,
};
use darkrun_core::StateStore;
use darkrun_mcp::position::run_start;
use darkrun_mcp::sessions::{self, ArchetypeSpec};

/// Theme palette for the archetype mockups.
fn palette(dark: bool) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
    if dark {
        ("#0b0e13", "#151a22", "#e8edf2", "#53b1fd", "#232a33")
    } else {
        ("#f3f6f9", "#ffffff", "#1d2430", "#1570cd", "#dbe3ec")
    }
}

/// Shared app chrome: top bar with title + actions, left nav rail.
fn chrome(dark: bool, title: &str) -> String {
    let (_bg, panel, text, accent, border) = palette(dark);
    format!(
        r##"<rect x="0" y="0" width="640" height="48" fill="{panel}"/>
<line x1="0" y1="48" x2="640" y2="48" stroke="{border}" stroke-width="1"/>
<text x="20" y="30" font-family="-apple-system,sans-serif" font-size="15" font-weight="700" fill="{text}">{title}</text>
<rect x="520" y="13" width="100" height="22" rx="6" fill="{accent}"/>
<text x="570" y="28" font-family="-apple-system,sans-serif" font-size="11" font-weight="600" fill="#ffffff" text-anchor="middle">Export CSV</text>
<rect x="0" y="48" width="44" height="352" fill="{panel}"/>
<circle cx="22" cy="76" r="6" fill="{accent}"/>
<circle cx="22" cy="104" r="6" fill="{border}"/>
<circle cx="22" cy="132" r="6" fill="{border}"/>"##
    )
}

/// LEDGER-FIRST: a dense financial table — header row, striped rows,
/// status pills, right-aligned amounts.
fn mock_ledger(dark: bool) -> String {
    let (bg, panel, text, accent, _border) = palette(dark);
    let mut rows = String::new();
    let data = [
        ("INV-1041", "May 2026", "$12,840.00", true),
        ("INV-1040", "Apr 2026", "$11,212.50", true),
        ("INV-1039", "Mar 2026", "$11,090.00", true),
        ("INV-1038", "Feb 2026", "$9,975.25", false),
        ("INV-1037", "Jan 2026", "$9,406.00", true),
    ];
    for (i, (id, period, amount, paid)) in data.iter().enumerate() {
        let y = 116 + i as i32 * 48;
        let stripe = if i % 2 == 0 { panel } else { bg };
        let (pill, pill_text) = if *paid { ("#2da44e", "paid") } else { (accent, "due") };
        rows.push_str(&format!(
            r##"<rect x="60" y="{y}" width="560" height="44" rx="6" fill="{stripe}"/>
<text x="76" y="{ty}" font-family="ui-monospace,monospace" font-size="12" fill="{text}">{id}</text>
<text x="180" y="{ty}" font-family="-apple-system,sans-serif" font-size="12" fill="{text}" opacity="0.65">{period}</text>
<rect x="300" y="{py}" width="44" height="18" rx="9" fill="{pill}" opacity="0.18"/>
<text x="322" y="{ty}" font-family="-apple-system,sans-serif" font-size="10" font-weight="600" fill="{pill}" text-anchor="middle">{pill_text}</text>
<text x="604" y="{ty}" font-family="ui-monospace,monospace" font-size="12" fill="{text}" text-anchor="end">{amount}</text>"##,
            ty = y + 27, py = y + 13,
        ));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400">
<rect width="640" height="400" fill="{bg}"/>
{chrome}
<text x="60" y="100" font-family="-apple-system,sans-serif" font-size="11" font-weight="600" fill="{text}" opacity="0.5">INVOICE</text>
<text x="180" y="100" font-family="-apple-system,sans-serif" font-size="11" font-weight="600" fill="{text}" opacity="0.5">PERIOD</text>
<text x="300" y="100" font-family="-apple-system,sans-serif" font-size="11" font-weight="600" fill="{text}" opacity="0.5">STATUS</text>
<text x="604" y="100" font-family="-apple-system,sans-serif" font-size="11" font-weight="600" fill="{text}" opacity="0.5" text-anchor="end">AMOUNT</text>
{rows}
</svg>"##,
        chrome = chrome(dark, "Invoices"),
    )
}

/// TIMELINE: a vertical activity stream — line, dots, event cards.
fn mock_timeline(dark: bool) -> String {
    let (bg, panel, text, accent, border) = palette(dark);
    let mut events = String::new();
    let data = [
        ("Invoice INV-1041 issued", "May 1 \u{00b7} $12,840.00", accent),
        ("Payment received", "Apr 28 \u{00b7} ACH \u{00b7} $11,212.50", "#2da44e"),
        ("Invoice INV-1040 issued", "Apr 1 \u{00b7} $11,212.50", accent),
        ("Plan upgraded \u{00b7} 48 \u{2192} 64 seats", "Mar 18", "#b07acc"),
    ];
    for (i, (title, meta, dot)) in data.iter().enumerate() {
        let y = 84 + i as i32 * 76;
        events.push_str(&format!(
            r##"<circle cx="96" cy="{cy}" r="7" fill="{dot}"/>
<rect x="124" y="{y}" width="468" height="60" rx="10" fill="{panel}" stroke="{border}"/>
<text x="142" y="{t1}" font-family="-apple-system,sans-serif" font-size="13" font-weight="600" fill="{text}">{title}</text>
<text x="142" y="{t2}" font-family="-apple-system,sans-serif" font-size="11" fill="{text}" opacity="0.55">{meta}</text>"##,
            cy = y + 30, t1 = y + 26, t2 = y + 45,
        ));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400">
<rect width="640" height="400" fill="{bg}"/>
{chrome}
<line x1="96" y1="68" x2="96" y2="388" stroke="{border}" stroke-width="2"/>
{events}
</svg>"##,
        chrome = chrome(dark, "Billing activity"),
    )
}

/// CARD GRID: one card per billing period — amount, period, status chip.
fn mock_cards(dark: bool) -> String {
    let (bg, panel, text, accent, border) = palette(dark);
    let mut cards = String::new();
    let data = [
        ("May 2026", "$12,840.00", "open", "#53b1fd"),
        ("Apr 2026", "$11,212.50", "paid", "#2da44e"),
        ("Mar 2026", "$11,090.00", "paid", "#2da44e"),
        ("Feb 2026", "$9,975.25", "paid", "#2da44e"),
        ("Jan 2026", "$9,406.00", "paid", "#2da44e"),
        ("Dec 2025", "$9,118.75", "paid", "#2da44e"),
    ];
    for (i, (period, amount, chip, chip_color)) in data.iter().enumerate() {
        let x = 64 + (i % 3) as i32 * 192;
        let y = 72 + (i / 3) as i32 * 156;
        cards.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="176" height="140" rx="12" fill="{panel}" stroke="{border}"/>
<text x="{tx}" y="{t1}" font-family="-apple-system,sans-serif" font-size="12" fill="{text}" opacity="0.6">{period}</text>
<text x="{tx}" y="{t2}" font-family="ui-monospace,monospace" font-size="17" font-weight="700" fill="{text}">{amount}</text>
<rect x="{tx}" y="{cy}" width="46" height="20" rx="10" fill="{chip_color}" opacity="0.18"/>
<text x="{ctx}" y="{cty}" font-family="-apple-system,sans-serif" font-size="10" font-weight="600" fill="{chip_color}" text-anchor="middle">{chip}</text>
<line x1="{tx}" y1="{ly}" x2="{lx2}" y2="{ly}" stroke="{border}" stroke-width="1"/>
<text x="{tx}" y="{dl}" font-family="-apple-system,sans-serif" font-size="10" fill="{accent}">Download PDF</text>"##,
            tx = x + 18, t1 = y + 30, t2 = y + 58, cy = y + 72,
            ctx = x + 41, cty = y + 86, ly = y + 106, lx2 = x + 158, dl = y + 126,
        ));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400">
<rect width="640" height="400" fill="{bg}"/>
{chrome}
{cards}
</svg>"##,
        chrome = chrome(dark, "Billing periods"),
    )
}

/// An archetype mockup as a data URI.
fn mockup(kind: &str, dark: bool) -> String {
    let svg = match kind {
        "ledger" => mock_ledger(dark),
        "timeline" => mock_timeline(dark),
        _ => mock_cards(dark),
    };
    format!("data:image/svg+xml;base64,{}", base64_encode(svg.as_bytes()))
}

/// Tiny dependency-free base64 (standard alphabet, padded).
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

fn unit(slug: &str, title: &str, station: &str, status: Status, deps: &[&str],
        beats: &[(&str, Option<IterationResult>)]) -> Unit {
    Unit {
        slug: slug.into(),
        frontmatter: UnitFrontmatter {
            name: Some(title.into()),
            status,
            station: Some(station.into()),
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            started_at: (!matches!(status, Status::Pending))
                .then(|| "2026-06-10T16:00:00Z".into()),
            completed_at: matches!(status, Status::Completed)
                .then(|| "2026-06-10T17:00:00Z".into()),
            iterations: beats
                .iter()
                .map(|(w, r)| UnitIteration {
                    worker: w.to_string(),
                    started_at: None,
                    completed_at: None,
                    result: *r,
                    note: None,
                })
                .collect(),
            ..Default::default()
        },
        title: title.into(),
        body: format!(
            "# {title}\n\n## Goal\nDeliver the {title} slice of the checkout flow.\n\n\
             ## Completion criteria\n- behavior verified \u{2192} `cargo test -p checkout {slug}` exits 0\n\
             - types clean \u{2192} `cargo clippy -p checkout` exits 0\n\n\
             ## Out of scope\n- payment-provider failover (parked to billing-portal)\n"
        ),
    }
}

fn station(name: &str, status: Status, phase: StationPhase, gate: Option<Checkpoint>) -> Station {
    Station {
        station: name.into(),
        status,
        phase,
        elaborated: true,
        checkpoint: gate,
        branch: None,
        pr_ref: None,
        pr_status: None,
        pr_ready_at: None,
        pr_merged_at: None,
        verifier_nonce: None,
        started_at: Some("2026-06-10T15:00:00Z".into()),
        completed_at: matches!(status, Status::Completed)
            .then(|| "2026-06-10T16:00:00Z".into()),
    }
}

fn seed(repo: &Path) -> std::io::Result<StateStore> {
    let _ = std::fs::remove_dir_all(repo);
    std::fs::create_dir_all(repo)?;
    let store = StateStore::new(repo);

    // ── Run 1: checkout-flow — build mid-Manufacture, rich DAG ──────────────
    run_start(&store, "checkout-flow", "software", Some("Checkout flow".into()),
              Mode::Solo, "full").expect("run 1");
    let mut state = store.read_state("checkout-flow").unwrap().unwrap();
    for (name, st) in [
        ("frame", Status::Completed),
        ("specify", Status::Completed),
        ("shape", Status::Completed),
    ] {
        state.stations.insert(name.into(), station(name, st, StationPhase::Checkpoint, None));
    }
    state.stations.insert(
        "build".into(),
        station("build", Status::InProgress, StationPhase::Manufacture, None),
    );
    state.active_station = "build".into();
    store.write_state("checkout-flow", &state).unwrap();
    let mut run_doc = store.read_run("checkout-flow").unwrap();
    run_doc.frontmatter.active_station = "build".into();
    store.write_run(&run_doc).unwrap();

    let adv = Some(IterationResult::Advance);
    let rej = Some(IterationResult::Reject);
    for u in [
        unit("cart-state", "Cart state machine", "build", Status::Completed, &[],
             &[("test_author", adv), ("builder", adv), ("self_reviewer", adv), ("reconciler", adv)]),
        unit("price-engine", "Pricing + discounts", "build", Status::Completed, &[],
             &[("test_author", adv), ("builder", adv), ("self_reviewer", adv), ("reconciler", adv)]),
        unit("payment-intent", "Payment intent API", "build", Status::InProgress, &["cart-state"],
             &[("test_author", adv), ("builder", adv)]),
        unit("checkout-ui", "Checkout surface", "build", Status::InProgress, &["cart-state", "price-engine"],
             &[("test_author", adv), ("builder", rej)]),
        unit("receipts", "Receipts + email", "build", Status::Pending, &["payment-intent"], &[]),
        unit("e2e-journey", "End-to-end journey", "build", Status::Pending,
             &["payment-intent", "checkout-ui"], &[]),
    ] {
        store.write_unit("checkout-flow", &u).unwrap();
    }
    let _ = darkrun_mcp::feedback::create(
        &store, "checkout-flow", "build",
        "Discount stacking is ambiguous for bundled SKUs \u{2014} spec says \u{201c}best single discount\u{201d} but the pricing unit applies both.",
        Some(darkrun_core::domain::FeedbackSeverity::High),
    );

    // ── Run 2: billing-portal — parked at an undecided Checkpoint ───────────
    run_start(&store, "billing-portal", "software", Some("Billing portal".into()),
              Mode::Solo, "full").expect("run 2");
    let mut state2 = store.read_state("billing-portal").unwrap().unwrap();
    state2.stations.insert(
        "frame".into(),
        station("frame", Status::InProgress, StationPhase::Checkpoint,
                Some(Checkpoint { kind: CheckpointKind::Ask, entered_at: Some("2026-06-10T17:20:00Z".into()), outcome: None })),
    );
    state2.active_station = "frame".into();
    store.write_state("billing-portal", &state2).unwrap();
    let frame_doc = "# Billing portal \u{2014} frame\n\n**Problem** Self-serve invoices live in email threads.\n\n**User** Finance admins at seat-billed teams.\n\n**Success metric** 80% of invoice questions self-served in 30 days.\n";
    let frame_dir = store.run_dir("billing-portal").join("frame");
    std::fs::create_dir_all(&frame_dir).ok();
    std::fs::write(frame_dir.join("frame.md"), frame_doc).ok();
    Ok(store)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::io::Result<()> {
    let repo = std::path::PathBuf::from(
        std::env::args().nth(1).unwrap_or_else(|| "/tmp/darkrun-demo".into()),
    );
    let port: u16 = std::env::var("DEMO_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7910);
    let store = seed(&repo)?;

    let state = darkrun_http::AppState::new(store, darkrun_http::Limits::default());

    // Review surfaces (registered under each run's slug + "current"); the
    // checkout-flow show lands LAST so "current" points at the richest surface.
    sessions::create_show(&state.sessions, &state.store, "billing-portal").expect("show 1");
    sessions::create_show(&state.sessions, &state.store, "checkout-flow").expect("show 2");

    // d-01: an awaiting design-direction pick with themed archetype pairs.
    let archetypes = vec![
        ArchetypeSpec {
            id: "ledger".into(),
            label: "Ledger-first".into(),
            image_url: mockup("ledger", true),
            image_url_light: Some(mockup("ledger", false)),
            description: "A dense financial table leads; actions live on each row.".into(),
        },
        ArchetypeSpec {
            id: "timeline".into(),
            label: "Timeline".into(),
            image_url: mockup("timeline", true),
            image_url_light: Some(mockup("timeline", false)),
            description: "Invoices as a vertical activity stream with inline previews.".into(),
        },
        ArchetypeSpec {
            id: "cards".into(),
            label: "Card grid".into(),
            image_url: mockup("cards", true),
            image_url_light: Some(mockup("cards", false)),
            description: "One card per billing period; drill in for line items.".into(),
        },
    ];
    sessions::create_direction(
        &state.sessions, "billing-portal",
        Some("Invoice surface".into()),
        "Pick the archetype for the invoice surface \u{2014} the portal's main screen.",
        Some("Frame locked: finance admins, self-serve invoices.".into()),
        archetypes.clone(),
    )
    .expect("direction d-01");

    // d-02: the same direction, CHOSEN + annotated (pins + comments).
    let picked = sessions::create_direction(
        &state.sessions, "billing-portal",
        Some("Invoice surface".into()),
        "Pick the archetype for the invoice surface \u{2014} the portal's main screen.",
        Some("Frame locked: finance admins, self-serve invoices.".into()),
        archetypes,
    )
    .expect("direction d-02");
    if let Some(darkrun_api::SessionPayload::Direction(mut d)) =
        state.sessions.get(&picked.session_id)
    {
        d.status = darkrun_api::SessionStatus::Answered;
        d.chosen_archetype = Some("ledger".into());
        d.annotations = Some(darkrun_api::DirectionAnnotations {
            pins: vec![
                darkrun_api::DirectionPin { x: 0.18, y: 0.22, note: "Keep the period switcher here".into() },
                darkrun_api::DirectionPin { x: 0.62, y: 0.55, note: "Row actions: download, dispute".into() },
            ],
            screenshot: None,
            comments: vec![
                "Ledger-first, but soften the grid \u{2014} finance admins live here all day.".into(),
                "Export must be one click from every state.".into(),
            ],
        });
        state.sessions.upsert(darkrun_api::SessionPayload::Direction(d));
    }

    // Serve + announce.
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let listener = darkrun_http::bind_listener(addr).await?;
    let bound = listener.local_addr()?;
    let _ = darkrun_mcp::registry::register_project(&repo, Some("acme-checkout".into()));
    let announce = darkrun_mcp::registry::EngineRegistry::new(repo.to_str().unwrap())
        .ok()
        .and_then(|r| r.announce(bound, "claude-code").ok());
    let _ = &announce;
    println!("demo engine on http://{bound}  (repo {})", repo.display());
    println!("sessions: checkout-flow · billing-portal · d-01 · d-02 (annotated)");
    let limits = state.limits;
    let router = darkrun_http::build_router(state);
    darkrun_http::serve_router_on(listener, router, limits).await?;
    Ok(())
}
