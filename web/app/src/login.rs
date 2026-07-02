//! The `/login` page — two flows behind one route.
//!
//! 1. **CLI-login bridge** (`?provider=&nonce=`): `darkrun login` opens this with
//!    a nonce. The user signs in via a full-page provider redirect
//!    ([`firebase::start_sign_in_redirect`]); on return the minted ID token is
//!    read ([`firebase::consume_redirect`]) and deposited to the relay broker
//!    under the nonce ([`firebase::deposit`]), where the waiting CLI claims it.
//!
//! 2. **Standalone login** (no nonce): visiting app.darkrun.ai's login directly
//!    signs in for the web app itself and lands on the [`Dashboard`], the start
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

    // No nonce → the standalone web flow: sign in (GitHub or GitLab) and land on
    // the dashboard (not the CLI-bridge "close the tab" path).
    let Some(nonce) = nonce else {
        return rsx! { StandaloneLogin {} };
    };

    // Sign-in is a full-page redirect: on return, consume the result and hand the
    // minted ID token to the CLI under its nonce. No pending redirect (a normal
    // load) is a no-op, leaving the sign-in prompt showing.
    {
        let nonce = nonce.clone();
        use_effect(move || {
            let nonce = nonce.clone();
            spawn(async move {
                match firebase::consume_redirect().await {
                    Ok(Some(session)) => {
                        match firebase::deposit(&firebase::web_base(), &nonce, &session.id_token).await {
                            Ok(()) => step.set(Step::Done),
                            Err(e) => step.set(Step::Failed(format!("Couldn't hand the token to the CLI: {e}"))),
                        }
                    }
                    Ok(None) => {}
                    Err(e) => step.set(Step::Failed(e)),
                }
            });
        });
    }

    let start = move |_| {
        let provider = provider.clone();
        step.set(Step::Working);
        spawn(async move {
            // On success the page navigates to the provider; it only returns here
            // (with an error) if it failed before navigating.
            if let Err(e) = firebase::start_sign_in_redirect(&provider).await {
                step.set(Step::Failed(e));
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
                Wordmark { variant: WordmarkVariant::OutlinedSolidRun, size: 22.0, interactive: true }
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

/// The accent sign-in button, with an optional leading Font Awesome icon
/// (`icon` = an `fa-*` class string, e.g. `fa-brands fa-github`).
#[component]
fn SignInButton(
    #[props(default = String::new())] icon: String,
    label: String,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let style = format!(
        "margin-top:20px;display:inline-flex;align-items:center;gap:8px;\
         padding:10px 20px;border:none;border-radius:8px;cursor:pointer;\
         background:{};color:{};font-family:{};font-size:14px;font-weight:600;",
        tokens::ACCENT, tokens::ON_ACCENT, tokens::FONT_SANS,
    );
    rsx! {
        button { style: "{style}", onclick: move |e| onclick.call(e),
            if !icon.is_empty() {
                i { class: "{icon}", style: "font-size:16px;" }
            }
            "{label}"
        }
    }
}

/// The standalone web experience (no CLI nonce) — also the default at the bare
/// app root. Offers BOTH GitHub and GitLab sign-in (no single-provider default);
/// on success it renders the [`Dashboard`] in place. Failure shows why and
/// re-offers both providers. The CLI-bridge path is untouched.
#[component]
pub(crate) fn StandaloneLogin() -> Element {
    let mut account = use_signal(|| None::<Account>);
    // Start in "working": on mount we consume any pending redirect result (we may
    // have just returned from a provider). No pending result flips us to Idle,
    // which shows the sign-in buttons.
    let mut step = use_signal(|| Step::Working);

    use_effect(move || {
        spawn(async move {
            match firebase::consume_redirect().await {
                Ok(Some(session)) => account.set(Some(Account::new(session))),
                Ok(None) => step.set(Step::Idle),
                Err(e) => step.set(Step::Failed(e)),
            }
        });
    });

    // Signed in → hand off to the dashboard. `on_link` starts a full-page redirect
    // to link the second provider to the SAME account; the linked identity comes
    // back through the redirect on the next load.
    if account().is_some() {
        return rsx! {
            Dashboard {
                account: account().unwrap(),
                on_link: move |provider: String| {
                    step.set(Step::Working);
                    spawn(async move {
                        if let Err(e) = firebase::start_link_redirect(&provider).await {
                            step.set(Step::Failed(e));
                        }
                    });
                },
            }
        };
    }

    rsx! {
        Shell {
            match step() {
                Step::Working => rsx! { Centered { title: "Signing in\u{2026}".to_string(), body: String::new() } },
                Step::Failed(msg) => rsx! {
                    div { style: "text-align:center;",
                        Centered { title: "Sign-in didn't finish".to_string(), body: msg }
                        ProviderButtons { step }
                    }
                },
                // `Idle` / the unreachable `Done` (the dashboard takes over on
                // success) both show the sign-in prompt.
                _ => rsx! {
                    div { style: "text-align:center;",
                        Centered {
                            title: "Sign in to darkrun".to_string(),
                            body: "Sign in to see your repositories and the darkrun runs in them.".to_string(),
                        }
                        ProviderButtons { step }
                    }
                },
            }
        }
    }
}

/// GitHub + GitLab sign-in buttons, side by side — the user picks; neither is a
/// default.
#[component]
fn ProviderButtons(step: Signal<Step>) -> Element {
    rsx! {
        div { style: "display:flex;gap:10px;justify-content:center;flex-wrap:wrap;",
            SignInButton { icon: "fa-brands fa-github".to_string(), label: "Sign in with GitHub".to_string(), onclick: move |_| start_sign_in("github", step) }
            SignInButton { icon: "fa-brands fa-gitlab".to_string(), label: "Sign in with GitLab".to_string(), onclick: move |_| start_sign_in("gitlab", step) }
        }
    }
}

/// Kick off a full-page redirect sign-in for `provider`. On success the page
/// navigates to the provider and returns to be picked up by the mount-time
/// `consume_redirect`; only a pre-navigation failure sets [`Step::Failed`].
fn start_sign_in(provider: &'static str, mut step: Signal<Step>) {
    step.set(Step::Working);
    spawn(async move {
        if let Err(e) = firebase::start_sign_in_redirect(provider).await {
            step.set(Step::Failed(e));
        }
    });
}
