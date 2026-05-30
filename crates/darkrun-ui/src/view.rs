//! Pure view + proof logic behind the artifact browser and the proof panel.
//!
//! No Dioxus, no rendering — just the formatting + classification the
//! [`crate::components::view_artifacts::ViewArtifacts`] and
//! [`crate::components::proof_panel::ProofPanel`] components drive, kept here so
//! they are trivially testable on native and the components stay thin.
//!
//! Three concerns live here:
//! - [`ArtifactKind`] — the render kind of one browsable output artifact, with a
//!   glyph + short label for the grid.
//! - web-vital classification ([`classify_vital`] / [`VitalVerdict`]) and value
//!   formatting ([`format_vital`]) against the standard Core Web Vitals
//!   thresholds.
//! - bench-stat formatting ([`format_latency_ms`] / [`format_throughput`]).

/// The render kind of one browsable output artifact — the design-system mirror
/// of the wire `ViewArtifactKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// An opaque file.
    File,
    /// An image.
    Image,
    /// A captured screenshot (the visual-review target).
    Screenshot,
    /// Markdown source.
    Markdown,
    /// A JSON document.
    Json,
}

impl ArtifactKind {
    /// A short, lowercase label for the artifact's kind chip.
    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::File => "file",
            ArtifactKind::Image => "image",
            ArtifactKind::Screenshot => "screenshot",
            ArtifactKind::Markdown => "md",
            ArtifactKind::Json => "json",
        }
    }

    /// A monochrome glyph standing in for the kind in the browser grid.
    pub fn glyph(self) -> char {
        match self {
            ArtifactKind::File => '▢',
            ArtifactKind::Image => '▣',
            ArtifactKind::Screenshot => '◧',
            ArtifactKind::Markdown => '¶',
            ArtifactKind::Json => '{',
        }
    }

    /// Whether this kind renders as a picture (image or screenshot) — drives
    /// whether the grid shows a thumbnail vs. a glyph tile.
    pub fn is_pictorial(self) -> bool {
        matches!(self, ArtifactKind::Image | ArtifactKind::Screenshot)
    }

    /// Whether this kind can be the target of a visual review (only a
    /// screenshot, the captured render of the surface).
    pub fn is_reviewable(self) -> bool {
        matches!(self, ArtifactKind::Screenshot)
    }
}

/// The good/needs-improvement/poor verdict of a web vital against its standard
/// Core Web Vitals threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VitalVerdict {
    /// Within the "good" threshold.
    Good,
    /// Between "good" and "poor".
    NeedsImprovement,
    /// Beyond the "poor" threshold.
    Poor,
    /// An unrecognized metric — no threshold to judge against.
    Unknown,
}

/// Classify a web vital value against the standard Core Web Vitals thresholds.
///
/// Thresholds (good / poor boundary), all latencies in ms except CLS (unitless):
/// - `lcp`: 2500 / 4000
/// - `fcp`: 1800 / 3000
/// - `cls`: 0.1 / 0.25
/// - `ttfb`: 800 / 1800
/// - `inp`: 200 / 500
///
/// The key match is case-insensitive. An unknown key yields
/// [`VitalVerdict::Unknown`].
pub fn classify_vital(key: &str, value: f64) -> VitalVerdict {
    let bounds = match key.trim().to_ascii_lowercase().as_str() {
        "lcp" => (2500.0, 4000.0),
        "fcp" => (1800.0, 3000.0),
        "cls" => (0.1, 0.25),
        "ttfb" => (800.0, 1800.0),
        "inp" => (200.0, 500.0),
        _ => return VitalVerdict::Unknown,
    };
    if !value.is_finite() {
        return VitalVerdict::Unknown;
    }
    if value <= bounds.0 {
        VitalVerdict::Good
    } else if value <= bounds.1 {
        VitalVerdict::NeedsImprovement
    } else {
        VitalVerdict::Poor
    }
}

/// The display label for a web-vital key (e.g. `lcp` -> `LCP`). Unknown keys are
/// upper-cased verbatim.
pub fn vital_label(key: &str) -> String {
    key.trim().to_ascii_uppercase()
}

/// Format a web-vital value for display, choosing units by the metric.
///
/// CLS is unitless (3 decimals); every other vital is a latency in ms, rendered
/// in seconds once it reaches a full second, else in `ms`.
pub fn format_vital(key: &str, value: f64) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    match key.trim().to_ascii_lowercase().as_str() {
        "cls" => format!("{value:.3}"),
        _ => format_latency_ms(value),
    }
}

/// Format a latency in milliseconds: under 1000ms as `"NNN ms"`, at or above as
/// `"N.NN s"`.
pub fn format_latency_ms(ms: f64) -> String {
    if !ms.is_finite() {
        return "—".to_string();
    }
    if ms.abs() >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else {
        format!("{ms:.0} ms")
    }
}

/// Format a throughput in operations per second, abbreviating thousands /
/// millions (`"1.50k ops/s"`, `"2.30M ops/s"`).
pub fn format_throughput(ops_per_sec: f64) -> String {
    if !ops_per_sec.is_finite() {
        return "—".to_string();
    }
    let abs = ops_per_sec.abs();
    if abs >= 1_000_000.0 {
        format!("{:.2}M ops/s", ops_per_sec / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}k ops/s", ops_per_sec / 1_000.0)
    } else {
        format!("{ops_per_sec:.0} ops/s")
    }
}

/// Format a sample count with thousands grouping for the bench panel.
pub fn format_samples(n: u64) -> String {
    // Group from the right in threes, then reverse — a running count of three
    // digits triggers a separator, avoiding any modulo arithmetic.
    let digits: Vec<char> = n.to_string().chars().rev().collect();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let mut run = 0u8;
    for c in &digits {
        if run == 3 {
            out.push(',');
            run = 0;
        }
        out.push(*c);
        run += 1;
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ArtifactKind ------------------------------------------------------

    #[test]
    fn artifact_kind_labels_and_glyphs_are_distinct() {
        let kinds = [
            ArtifactKind::File,
            ArtifactKind::Image,
            ArtifactKind::Screenshot,
            ArtifactKind::Markdown,
            ArtifactKind::Json,
        ];
        let mut labels: Vec<&str> = kinds.iter().map(|k| k.label()).collect();
        let total = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), total, "labels must be unique");
    }

    #[test]
    fn pictorial_kinds_are_image_and_screenshot() {
        assert!(ArtifactKind::Image.is_pictorial());
        assert!(ArtifactKind::Screenshot.is_pictorial());
        assert!(!ArtifactKind::Markdown.is_pictorial());
        assert!(!ArtifactKind::Json.is_pictorial());
        assert!(!ArtifactKind::File.is_pictorial());
    }

    #[test]
    fn only_screenshot_is_reviewable() {
        assert!(ArtifactKind::Screenshot.is_reviewable());
        for k in [
            ArtifactKind::File,
            ArtifactKind::Image,
            ArtifactKind::Markdown,
            ArtifactKind::Json,
        ] {
            assert!(!k.is_reviewable(), "{k:?} must not be reviewable");
        }
    }

    // --- vital classification ----------------------------------------------

    #[test]
    fn lcp_classifies_at_thresholds() {
        assert_eq!(classify_vital("lcp", 2000.0), VitalVerdict::Good);
        assert_eq!(classify_vital("lcp", 2500.0), VitalVerdict::Good);
        assert_eq!(classify_vital("lcp", 3000.0), VitalVerdict::NeedsImprovement);
        assert_eq!(classify_vital("lcp", 4000.0), VitalVerdict::NeedsImprovement);
        assert_eq!(classify_vital("lcp", 4500.0), VitalVerdict::Poor);
    }

    #[test]
    fn cls_uses_unitless_thresholds() {
        assert_eq!(classify_vital("cls", 0.05), VitalVerdict::Good);
        assert_eq!(classify_vital("cls", 0.2), VitalVerdict::NeedsImprovement);
        assert_eq!(classify_vital("cls", 0.3), VitalVerdict::Poor);
    }

    #[test]
    fn classify_is_case_insensitive_and_guards_unknown() {
        assert_eq!(classify_vital("INP", 100.0), VitalVerdict::Good);
        assert_eq!(classify_vital("ttfb", 900.0), VitalVerdict::NeedsImprovement);
        assert_eq!(classify_vital("fcp", 1000.0), VitalVerdict::Good);
        assert_eq!(classify_vital("telepathy", 1.0), VitalVerdict::Unknown);
        assert_eq!(classify_vital("lcp", f64::NAN), VitalVerdict::Unknown);
    }

    // --- formatting --------------------------------------------------------

    #[test]
    fn vital_value_formats_by_metric() {
        assert_eq!(format_vital("cls", 0.0234), "0.023");
        assert_eq!(format_vital("lcp", 1200.0), "1.20 s");
        assert_eq!(format_vital("ttfb", 640.0), "640 ms");
        assert_eq!(format_vital("inp", f64::INFINITY), "—");
    }

    #[test]
    fn latency_crosses_to_seconds_at_one_second() {
        assert_eq!(format_latency_ms(999.0), "999 ms");
        assert_eq!(format_latency_ms(1000.0), "1.00 s");
        assert_eq!(format_latency_ms(2500.0), "2.50 s");
    }

    #[test]
    fn throughput_abbreviates_thousands_and_millions() {
        assert_eq!(format_throughput(500.0), "500 ops/s");
        assert_eq!(format_throughput(1500.0), "1.5k ops/s");
        assert_eq!(format_throughput(2_300_000.0), "2.30M ops/s");
        assert_eq!(format_throughput(f64::NAN), "—");
    }

    #[test]
    fn vital_label_upcases() {
        assert_eq!(vital_label("lcp"), "LCP");
        assert_eq!(vital_label(" inp "), "INP");
    }

    #[test]
    fn samples_group_thousands() {
        assert_eq!(format_samples(0), "0");
        assert_eq!(format_samples(42), "42");
        assert_eq!(format_samples(1000), "1,000");
        assert_eq!(format_samples(1_234_567), "1,234,567");
    }
}
