//! darkrun-site web entrypoint — launches the Dioxus app in the browser.
//!
//! Built for `wasm32-unknown-unknown` and served as a single-page app; the
//! router takes over from there. The shared library crate holds all the
//! components so the static-site generator can reuse them.

use darkrun_site::App;

fn main() {
    dioxus::launch(App);
}
