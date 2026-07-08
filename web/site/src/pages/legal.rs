//! `/privacy` and `/terms` — the legal pages, rendered as prose.

use darkrun_ui::prelude::*;

const PRIVACY: &str = r#"# Privacy

darkrun runs locally. Your code, your runs, and your review feedback live on your
machine and in your repository. The binary ships its factory corpus inline, and
nothing about the local engine requires an account.

## Crash reports (released binaries)

The released CLI and desktop builds send crash reports to Sentry. If one of the
shipped binaries panics, the stack and a small amount of build context (version,
platform, environment tag) go to Sentry so the bug is visible and fixable. No
personal identifiers are attached: `send_default_pii` is off, so your source, your
prompts, and your run contents are not sent. Local dev builds with no DSN never
report at all.

To turn crash reporting off, set either `DARKRUN_NO_TELEMETRY` or the standard
`DO_NOT_TRACK` in your environment. When either is set, telemetry is a no-op.

## Model providers

When you point darkrun at a model provider, your prompts and the context the
factory assembles are sent to that provider under their terms. That traffic goes
straight to the provider you chose. darkrun does not proxy it.

## The web app (app.darkrun.ai)

If you sign in to enable remote access, the web app collects the data it needs to
connect you to your own runs:

- **Firebase account.** Your identity is a Firebase account keyed to the provider
  you sign in with.
- **Linked identities.** The GitHub and GitLab identities you link to that account.
- **Provider OAuth tokens.** The GitHub/GitLab OAuth tokens are parked on the
  server so it can act on your behalf against those providers.
- **Device push tokens.** FCM device tokens are persisted in Firestore so push
  notifications survive restarts.
- **Run state in transit.** When you drive a run remotely, run state transits the
  relay between your machine and the web app.

If you never sign in, none of this applies. The local engine stays local.

This website serves static content and sets no tracking cookies.
"#;

const TERMS: &str = r#"# Terms

darkrun is licensed under **FSL-1.1-ALv2**, the Functional Source License,
version 1.1, with an Apache 2.0 future grant. It is source-available and free to
use, modify, and self-host for any purpose that is not building a competing
product, and two years after each release that release converts to Apache 2.0.
See the LICENSE file at the root of the repository for the full terms.

The software is provided as-is, without warranty of any kind. You are responsible
for the work the factory produces and for reviewing it before you ship it. That is
the entire point of the checkpoints.

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
