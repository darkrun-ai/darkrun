//! The `/login` page — two flows behind one route.
//!
//! 1. **CLI-login bridge** (`?provider=&nonce=`): `darkrun login` opens this with
//!    a nonce. The user signs in with Firebase Auth ([`firebase::sign_in`]) and
//!    the minted ID token is deposited to the relay broker under the nonce
//!    ([`firebase::deposit`]), where the waiting CLI claims it. The user then
//!    closes the tab and the CLI is logged in. This path is unchanged.
//!
//! 2. **Standalone login** (no nonce): visiting app.darkrun.ai's login directly
//!    signs in for the web app itself and lands on the [`Dashboard`] — the start
//!    of the standalone web experience, not a "close the tab" dead end.

use darkrun_ui::prelude::*;
use darkrun_ui::tokens;

use crate::dashboard::Dashboard;
use crate::firebase::{self, Account};

/// The step the page is on — drives what's shown.
#[derive(Clone, PartialEq)]
enum Step {
    /// Waiting for the user to start sign-in.
    Idle,
    /// Sign-in / deposit in flight.
    Working,
    /// Done — the CLI has the token.
    Done,
    /// Something failed; show why and let them retry.
    Failed(String),
}

/// The login page.
#[component]
pub fn LoginPage() -> Element {
    let provider = firebase::query_param("provider").unwrap_or_else(|| "github".to_string());
    let nonce = firebase::query_param("nonce");
    let mut step = use_signal(|| Step::Idle);

    let provider_label = if provider == "gitlab" { "GitLab" } else { "GitHub" };

    // No nonce → the standalone web flow: sign in for the web app and land on the
    // dashboard (not the CLI-bridge "close the tab" path).
    let Some(nonce) = nonce else {
        return rsx! { StandaloneLogin { provider, provider_label: provider_label.to_string() } };
    };

    let start = move |_| {
        let provider = provider.clone();
        let nonce = nonce.clone();
        step.set(Step::Working);
        spawn(async move {
            match firebase::sign_in(&provider).await {
                Ok(token) => match firebase::deposit(&firebase::web_base(), &nonce, &token).await {
                    Ok(()) => step.set(Step::Done),
                    Err(e) => step.set(Step::Failed(format!("Couldn't hand the token to the CLI: {e}"))),
                },
                Err(e) => step.set(Step::Failed(e)),
            }
        });
    };

    rsx! {
        Shell {
            match step() {
                Step::Done => rsx! {
                    Centered {
                        title: "You're signed in".to_string(),
                        body: "Remote access is enabled. Return to your terminal — it's ready. You can close this tab.".to_string(),
                    }
                },
                Step::Working => rsx! { Centered { title: "Signing in…".to_string(), body: String::new() } },
                Step::Failed(msg) => rsx! {
                    div { style: "text-align:center;",
                        Centered { title: "Sign-in didn't finish".to_string(), body: msg }
                        SignInButton { label: format!("Try again with {provider_label}"), onclick: start }
                    }
                },
                Step::Idle => rsx! {
                    div { style: "text-align:center;",
                        Centered {
                            title: "Enable remote access".to_string(),
                            body: format!("Sign in with {provider_label} to control this run from the web and your phone."),
                        }
                        SignInButton { label: format!("Sign in with {provider_label}"), onclick: start }
                    }
                },
            }
        }
    }
}

/// The dark page shell (header + centered content).
#[component]
fn Shell(children: Element) -> Element {
    let shell = format!(
        "min-height:100vh;background:{};color:{};font-family:{};display:flex;flex-direction:column;",
        tokens::SURFACE_BASE, tokens::TEXT, tokens::FONT_SANS,
    );
    rsx! {
        div { style: "{shell}",
            header { style: format!("padding:16px 20px;border-bottom:1px solid {};", tokens::BORDER),
                Wordmark { variant: WordmarkVariant::OutlinedSolidRun }
            }
            main { style: "flex:1;display:flex;align-items:center;justify-content:center;padding:24px;",
                {children}
            }
        }
    }
}

/// A centered title + body block.
#[component]
fn Centered(title: String, body: String) -> Element {
    rsx! {
        div { style: "max-width:34ch;text-align:center;",
            h1 { style: format!("font-family:{};font-size:20px;color:{};margin:0 0 10px;", tokens::FONT_SANS, tokens::TEXT), "{title}" }
            if !body.is_empty() {
                p { style: format!("font-family:{};font-size:14px;color:{};margin:0;line-height:1.5;", tokens::FONT_SANS, tokens::TEXT_MUTED), "{body}" }
            }
        }
    }
}

/// The accent sign-in button.
#[component]
fn SignInButton(label: String, onclick: EventHandler<MouseEvent>) -> Element {
    let style = format!(
        "margin-top:20px;padding:10px 20px;border:none;border-radius:8px;cursor:pointer;\
         background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
        tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
    );
    rsx! {
        button { style: "{style}", onclick: move |e| onclick.call(e), "{label}" }
    }
}

/// The standalone web login (no CLI nonce): sign in for the web app itself and,
/// on success, render the [`Dashboard`] in place. Failure shows why and offers a
/// retry; the CLI-bridge path is untouched.
#[component]
fn StandaloneLogin(provider: String, provider_label: String) -> Element {
    let mut account = use_signal(|| None::<Account>);
    let mut step = use_signal(|| Step::Idle);

    // Signed in → hand off to the dashboard. `on_link` lets the dashboard add the
    // second provider to the SAME account (one Firebase uid spanning both).
    if account().is_some() {
        return rsx! {
            Dashboard {
                account: account().unwrap(),
                on_link: move |identity| account.with_mut(|a| {
                    if let Some(a) = a { a.link(identity); }
                }),
            }
        };
    }

    let start = move |_| {
        let provider = provider.clone();
        step.set(Step::Working);
        spawn(async move {
            match firebase::sign_in_for_dashboard(&provider).await {
                Ok(s) => account.set(Some(Account::new(s))),
                Err(e) => step.set(Step::Failed(e)),
            }
        });
    };

    rsx! {
        Shell {
            match step() {
                Step::Working => rsx! { Centered { title: "Signing in\u{2026}".to_string(), body: String::new() } },
                Step::Failed(msg) => rsx! {
                    div { style: "text-align:center;",
                        Centered { title: "Sign-in didn't finish".to_string(), body: msg }
                        SignInButton { label: format!("Try again with {provider_label}"), onclick: start }
                    }
                },
                // `Idle` and the unreachable `Done` (the dashboard takes over on
                // success) both show the sign-in prompt.
                _ => rsx! {
                    div { style: "text-align:center;",
                        Centered {
                            title: "Sign in to darkrun".to_string(),
                            body: format!("Sign in with {provider_label} to see your repositories and runs."),
                        }
                        SignInButton { label: format!("Sign in with {provider_label}"), onclick: start }
                    }
                },
            }
        }
    }
}
