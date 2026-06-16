//! app.darkrun.ai web entrypoint — launches the Dioxus app in the browser.
//!
//! Built for `wasm32-unknown-unknown` and served by Firebase Hosting as a
//! single-page app. The shared library crate holds the components.

fn main() {
    dioxus::launch(darkrun_app::App);
}
