//! darkrun observability — one Sentry init shared by every binary surface.
//!
//! All four surfaces report to Sentry through [`init`]: the `darkrun` CLI/MCP
//! binary, the `darkrun-desktop` app, the `darkrun-web` server, and (separately,
//! via the browser JS SDK) the website. Each calls `init(<service>)` at the top
//! of `main` and holds the returned guard for the process lifetime.
//!
//! **DSN resolution** is the key design point. For the DISTRIBUTED binaries (the
//! CLI + desktop) the DSN is compiled in: the release build sets
//! `DARKRUN_SENTRY_DSN` so [`option_env!`] bakes it into the artifact and a
//! shipped binary reports without any runtime config. The SERVER reads the same
//! var from its environment (Cloud Run wires it from Secret Manager). A build /
//! run with no DSN is a clean no-op — local dev never phones home.
//!
//! **Opt-out is authoritative.** Regardless of a compiled-in DSN, telemetry is
//! disabled when the user sets `DARKRUN_NO_TELEMETRY` or the cross-vendor
//! `DO_NOT_TRACK` to a truthy value, or when they explicitly set an empty
//! `DARKRUN_SENTRY_DSN`. The opt-out always wins over the baked-in DSN.
//!
//! C-free: the transport is reqwest + rustls (no native-tls / openssl).

#![deny(missing_docs)]

/// What [`init`] should do once opt-out and DSN resolution are applied.
///
/// Pure decision type so the "do we phone home?" logic is unit-testable without
/// ever contacting Sentry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Decision {
    /// Do not initialize Sentry — opt-out is active or no DSN is configured.
    Skip,
    /// Initialize Sentry, reporting to this resolved DSN.
    Init(String),
}

/// Whether an env value opts telemetry OUT: present and set to something other
/// than an empty / explicitly-false value (`0`, `false`, `no`, `off`).
fn is_optout(value: Option<&str>) -> bool {
    match value {
        Some(v) => {
            let v = v.trim();
            !v.is_empty()
                && !matches!(
                    v.to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
        }
        None => false,
    }
}

/// Resolve whether to initialize Sentry, and to which DSN.
///
/// Precedence, opt-out first so it always wins over a compiled-in DSN:
/// 1. `DARKRUN_NO_TELEMETRY` or `DO_NOT_TRACK` truthy → [`Decision::Skip`].
/// 2. An explicitly-empty `DARKRUN_SENTRY_DSN` (set but blank) → [`Decision::Skip`].
/// 3. Otherwise the compiled-in DSN (distributed binaries) takes precedence over
///    the run-time env DSN (the server). No DSN → [`Decision::Skip`].
fn decide(
    compile_dsn: Option<&str>,
    env_dsn: Option<&str>,
    no_telemetry: Option<&str>,
    do_not_track: Option<&str>,
) -> Decision {
    if is_optout(no_telemetry) || is_optout(do_not_track) {
        return Decision::Skip;
    }
    // An explicitly-set-but-empty DARKRUN_SENTRY_DSN disables telemetry even
    // when a DSN was baked in at build time.
    if let Some(env) = env_dsn {
        if env.trim().is_empty() {
            return Decision::Skip;
        }
    }
    match compile_dsn
        .map(str::to_string)
        .or_else(|| env_dsn.map(str::to_string))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(dsn) => Decision::Init(dsn),
        None => Decision::Skip,
    }
}

/// The release identifier reported to Sentry — `darkrun@<version>`, or the exact
/// build tag when the release pipeline injects `DARKRUN_RELEASE`.
fn release() -> std::borrow::Cow<'static, str> {
    match option_env!("DARKRUN_RELEASE") {
        Some(r) if !r.trim().is_empty() => r.into(),
        _ => format!("darkrun@{}", env!("CARGO_PKG_VERSION")).into(),
    }
}

/// The deployment environment tag — `DARKRUN_ENV` if set, else `development` for
/// debug builds and `production` for release builds.
fn environment() -> std::borrow::Cow<'static, str> {
    if let Ok(env) = std::env::var("DARKRUN_ENV") {
        let env = env.trim().to_string();
        if !env.is_empty() {
            return env.into();
        }
    }
    if cfg!(debug_assertions) {
        "development".into()
    } else {
        "production".into()
    }
}

/// Initialize Sentry for `service` (e.g. `"cli"`, `"desktop"`, `"web"`), tagging
/// every event with that surface. Returns the client guard — **hold it for the
/// program's lifetime** (dropping it flushes and shuts Sentry down). Returns
/// `None` when no DSN is configured, in which case telemetry is a no-op.
///
/// Panics are captured automatically (the `panic` integration). The PII default
/// is off — `send_default_pii` stays `false`.
#[must_use = "hold the guard for the process lifetime; dropping it disables Sentry"]
pub fn init(service: &'static str) -> Option<sentry::ClientInitGuard> {
    // Read every input, then let `decide` apply opt-out + DSN precedence. Note
    // `option_env!` is compile-time; `env::var` distinguishes unset (`Err`) from
    // set-but-empty (`Ok("")`), which is the explicit-disable signal.
    let env_dsn = std::env::var("DARKRUN_SENTRY_DSN").ok();
    let no_telemetry = std::env::var("DARKRUN_NO_TELEMETRY").ok();
    let do_not_track = std::env::var("DO_NOT_TRACK").ok();
    let dsn = match decide(
        option_env!("DARKRUN_SENTRY_DSN"),
        env_dsn.as_deref(),
        no_telemetry.as_deref(),
        do_not_track.as_deref(),
    ) {
        Decision::Init(dsn) => dsn,
        Decision::Skip => return None,
    };
    let guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            release: Some(release()),
            environment: Some(environment()),
            // Conservative defaults: no PII, errors-only (no perf sampling by
            // default — surfaces can opt in via traces_sample_rate later).
            send_default_pii: false,
            ..Default::default()
        },
    ));
    sentry::configure_scope(|scope| {
        scope.set_tag("service", service);
    });
    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::{decide, Decision};

    const DSN: &str = "https://public@example.ingest.sentry.io/42";

    #[test]
    fn no_dsn_is_a_noop() {
        assert_eq!(decide(None, None, None, None), Decision::Skip);
    }

    #[test]
    fn blank_dsn_values_are_a_noop() {
        assert_eq!(decide(Some("   "), Some("  "), None, None), Decision::Skip);
    }

    #[test]
    fn compiled_in_dsn_initializes() {
        assert_eq!(
            decide(Some(DSN), None, None, None),
            Decision::Init(DSN.to_string())
        );
    }

    #[test]
    fn env_dsn_initializes_when_no_compiled_dsn() {
        assert_eq!(
            decide(None, Some(DSN), None, None),
            Decision::Init(DSN.to_string())
        );
    }

    #[test]
    fn compiled_in_dsn_takes_precedence_over_env() {
        assert_eq!(
            decide(Some(DSN), Some("https://other@x.sentry.io/1"), None, None),
            Decision::Init(DSN.to_string())
        );
    }

    #[test]
    fn no_telemetry_optout_wins_over_dsn() {
        assert_eq!(decide(Some(DSN), None, Some("1"), None), Decision::Skip);
        assert_eq!(decide(Some(DSN), None, Some("true"), None), Decision::Skip);
    }

    #[test]
    fn do_not_track_optout_wins_over_dsn() {
        assert_eq!(decide(Some(DSN), None, None, Some("1")), Decision::Skip);
    }

    #[test]
    fn falsy_optout_values_do_not_disable() {
        for falsy in ["0", "false", "no", "off", "", "  "] {
            assert_eq!(
                decide(Some(DSN), None, Some(falsy), Some(falsy)),
                Decision::Init(DSN.to_string()),
                "opt-out value {falsy:?} must not disable telemetry",
            );
        }
    }

    #[test]
    fn explicit_empty_env_dsn_disables_compiled_in_dsn() {
        // Even with a DSN baked in, `DARKRUN_SENTRY_DSN=""` opts out.
        assert_eq!(decide(Some(DSN), Some(""), None, None), Decision::Skip);
        assert_eq!(decide(Some(DSN), Some("   "), None, None), Decision::Skip);
    }
}
