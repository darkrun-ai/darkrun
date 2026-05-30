//! `/privacy` and `/terms` — the legal pages, rendered as prose.

use darkrun_ui::prelude::*;

const PRIVACY: &str = r#"# Privacy

darkrun runs locally. Your code, your runs, and your review feedback stay on your
machine and in your repository. The binary ships its factory corpus inline and
does not phone home.

When you point darkrun at a model provider, your prompts and the context the
factory assembles are sent to that provider under their terms. darkrun adds no
telemetry of its own.

This website serves static content and sets no tracking cookies.
"#;

const TERMS: &str = r#"# Terms

darkrun is provided under the MIT license, as-is, without warranty of any kind.
You are responsible for the work the factory produces and for reviewing it before
you ship it — that is the entire point of the checkpoints.

By using darkrun you agree that you are responsible for complying with the terms
of any model provider you connect it to.
"#;

/// `/privacy`.
#[component]
pub fn Privacy() -> Element {
    let html = crate::content::render_markdown(PRIVACY);
    rsx! { article { class: "dr-prose", dangerous_inner_html: "{html}" } }
}

/// `/terms`.
#[component]
pub fn Terms() -> Element {
    let html = crate::content::render_markdown(TERMS);
    rsx! { article { class: "dr-prose", dangerous_inner_html: "{html}" } }
}
