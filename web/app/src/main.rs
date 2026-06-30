//! app.darkrun.ai web entrypoint — launches the Dioxus app in the browser.
//!
//! Built for `wasm32-unknown-unknown` and served by Firebase Hosting as a
//! single-page app. The shared library crate holds the components.

fn main() {
    // The app is dark-only (like darkrun.ai), so pin `data-theme="dark"` on the
    // root BEFORE launch. Without it the app follows the OS appearance — and on a
    // light-mode OS the wordmark's "dark" glyphs render the light treatment
    // (solid `--dr-text`, no cyan stroke) and vanish on the dark background.
    // Setting it pre-launch also avoids a first-frame light flash.
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-theme", "dark");
    }
    dioxus::launch(darkrun_app::App);
}
