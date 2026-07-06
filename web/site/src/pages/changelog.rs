//! `/changelog` — a simple reverse-chronological release log, rendered from a
//! small in-crate data table.

use darkrun_ui::prelude::*;

use crate::ui::theme;

use crate::ui::SectionHead;

/// A single changelog entry.
struct Release {
    version: &'static str,
    date: &'static str,
    notes: &'static [&'static str],
}

const RELEASES: &[Release] = &[
    Release {
        version: "0.7.0",
        date: "2026-06-17",
        notes: &[
            "Web push registration and the Firebase web config: login is now functional.",
            "Web Push VAPID key wired, so remote push is fully configured.",
            "Firestore-backed device registry so push survives restarts.",
        ],
    },
    Release {
        version: "0.6.0",
        date: "2026-06-16",
        notes: &[
            "Remote-push spine: FCM fan-out plus device registration.",
            "The host connector fires a remote notification when a run reaches a gate.",
            "Google service-account token source, so FCM push goes live.",
        ],
    },
    Release {
        version: "0.5.0",
        date: "2026-06-16",
        notes: &[
            "darkrun login and the shared local/remote tunnel protocol: remote access to your runs.",
            "Firebase-native cloud foundation: Firestore data model, rules, project config.",
            "SHA optimistic-concurrency write guards on artifacts; engine-driven run setup; questions surface on the run and persist across restarts.",
        ],
    },
    Release {
        version: "0.4.0",
        date: "2026-06-11",
        notes: &[
            "The statusline demo on the website: boxed session banner, left/right stepper, in-situ render under the prompt box.",
            "Terminal panels follow the site theme; the statusline reads on light terminals.",
        ],
    },
    Release {
        version: "0.3.0",
        date: "2026-06-11",
        notes: &[
            "Composite runs: multi-factory topology with sync points.",
            "Reject-escalation up the model ladder, the keep-or-drop station offer, and the runtime-verifier reviewer.",
            "Run-level draft PRs, docs search with JSON-LD, and a live per-tick desktop session mirror.",
        ],
    },
    Release {
        version: "0.2.0",
        date: "2026-06-08",
        notes: &[
            "The factory-orchestration engine, design system, website, and Claude Code plugin.",
            "Pure-Rust gitoxide backend — worktrees, three-way merge, native fetch, C-free.",
            "Surface-routed objective verification, visual-question / design-direction sessions.",
        ],
    },
    Release {
        version: "0.1.0",
        date: "2026-05-30",
        notes: &[
            "First cut of the software factory: Frame -> Specify -> Shape -> Build -> Prove -> Harden.",
            "The six-phase station machine: spec -> review -> manufacture -> audit -> tests -> checkpoint.",
            "Embedded factory corpus, the local engine, and the desktop review app.",
        ],
    },
];

/// `/changelog` — the release log.
#[component]
pub fn Changelog() -> Element {
    rsx! {
        SectionHead {
            kicker: "releases".to_string(),
            title: "Changelog".to_string(),
            lead: Some("What shipped, newest first.".to_string()),
        }
        div { style: "display:flex;flex-direction:column;gap:16px;",
            for release in RELEASES {
                Card {
                    div {
                        style: format!(
                            "display:flex;align-items:baseline;gap:10px;margin-bottom:8px;font-family:{};",
                            tokens::FONT_SANS,
                        ),
                        span {
                            style: format!("font-size:18px;font-weight:700;color:{};", theme::TEXT),
                            "v{release.version}"
                        }
                        span {
                            style: format!("font-family:{};font-size:12px;color:{};", tokens::FONT_MONO, theme::TEXT_FAINT),
                            "{release.date}"
                        }
                    }
                    ul {
                        style: format!("margin:0;padding-left:18px;color:{};font-family:{};font-size:14px;line-height:1.7;", theme::TEXT_MUTED, tokens::FONT_SANS),
                        for note in release.notes {
                            li { "{note}" }
                        }
                    }
                }
            }
        }
    }
}
