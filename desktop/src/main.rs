//! darkrun-desktop — the Dioxus cross-platform review app.
//!
//! The chrome is built entirely from the shared [`darkrun_ui`] design system so
//! the desktop app and the website stay visually identical (dark-only, the
//! darkrun brand). This binary connects to the local engine over a WebSocket
//! (`ws://127.0.0.1:PORT/ws/session/:id`), renders the live Review session — the
//! station pipeline, the unit list with completion criteria, declared outputs,
//! and a Checkpoint bar — and POSTs approve / request-changes decisions back to
//! `POST /review/:id/decide`.
//!
//! The session id and engine port are read from the environment so the engine
//! can launch the app pointed at a live run:
//!   - `DARKRUN_PORT`       (default `7878`)
//!   - `DARKRUN_SESSION_ID` (default `current`)

use darkrun_ui::prelude::*;

mod map;
mod review;
mod wire;

use wire::ConnConfig;

fn main() {
    dioxus::launch(app);
}

/// Top-level app: reads the connection config from the environment, opens the
/// live session feed, and renders the review screen.
fn app() -> Element {
    let cfg = ConnConfig::from_env();
    rsx! {
        style { "{darkrun_ui::tokens::THEME_CSS}" }
        review::ReviewApp { cfg }
    }
}
