//! The smart install banner — nudges toward the native apps.
//!
//! The web app is the fallback; the native apps are the real surface (and the
//! universal-link target — tapping a run link opens the installed app instead).
//! This dismissible bar offers the App Store / Play / desktop downloads.

use darkrun_ui::prelude::*;
use darkrun_ui::tokens;

/// A dismissible bar pointing at the native apps.
#[component]
pub fn InstallBanner() -> Element {
    let mut dismissed = use_signal(|| false);
    if dismissed() {
        return rsx! {};
    }

    let bar = format!(
        "display:flex;align-items:center;gap:12px;padding:10px 20px;\
         background:{};border-bottom:1px solid {};font-family:{};font-size:13px;color:{};",
        tokens::SURFACE_RAISED,
        tokens::BORDER,
        tokens::FONT_SANS,
        tokens::TEXT_MUTED,
    );
    let link = format!(
        "color:{};text-decoration:none;font-weight:600;",
        tokens::ACCENT,
    );

    let dl = format!("{link}display:inline-flex;align-items:center;gap:6px;");

    rsx! {
        div { style: "{bar}",
            i { class: "fa-solid fa-arrow-down-to-bracket", style: format!("color:{};", tokens::TEXT) }
            span { style: format!("color:{};", tokens::TEXT), "Get the darkrun app" }
            span { "\u{2014} faster, with notifications and Handoff." }
            span { style: "flex:1;" }
            a { style: "{dl}", href: "https://apps.apple.com/app/darkrun",
                i { class: "fa-brands fa-apple" } "App Store"
            }
            a { style: "{dl}", href: "https://play.google.com/store/apps/details?id=ai.darkrun.app",
                i { class: "fa-brands fa-google-play" } "Google Play"
            }
            a { style: "{dl}", href: "https://darkrun.ai/download",
                i { class: "fa-solid fa-desktop" } "Desktop"
            }
            button {
                style: format!(
                    "background:none;border:none;cursor:pointer;color:{};font-size:16px;line-height:1;padding:0 4px;",
                    tokens::TEXT_FAINT,
                ),
                "aria-label": "Dismiss",
                onclick: move |_| dismissed.set(true),
                i { class: "fa-solid fa-xmark" }
            }
        }
    }
}
