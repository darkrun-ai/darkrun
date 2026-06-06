//! On-disk feedback document model.
//!
//! A feedback item is a markdown file with a small YAML-ish frontmatter block
//! delimited by `---` fences, followed by the markdown body:
//!
//! ```text
//! ---
//! id: FB-01
//! station: frame
//! status: pending
//! origin: user-visual
//! title: Tighten the spec
//! author: user
//! created_at: 2026-05-30T00:00:00Z
//! visit: 0
//! closed_by:
//! ---
//! The body markdown goes here.
//! ```
//!
//! This module owns parsing and rendering that format plus minting the next
//! `FB-NN` id. It deliberately keeps the parser forgiving: missing fields fall
//! back to sane defaults so a hand-edited or legacy file still loads, and an
//! unknown `status`/`origin` token folds onto a known variant rather than
//! failing the read.

use darkrun_api::{FeedbackItem, FeedbackOrigin, FeedbackSeverity, FeedbackStatus};

/// The parsed form of a feedback sidecar file.
#[derive(Debug, Clone)]
pub struct FeedbackDoc {
    /// The `FB-NN` id (taken from the file stem, authoritative).
    pub id: String,
    /// Short title.
    pub title: String,
    /// Markdown body.
    pub body: String,
    /// Lifecycle status.
    pub status: FeedbackStatus,
    /// Where the item originated.
    pub origin: FeedbackOrigin,
    /// Finding severity, when classified.
    pub severity: Option<FeedbackSeverity>,
    /// Targeted station, if recorded.
    pub station: Option<String>,
    /// Author handle.
    pub author: String,
    /// Creation timestamp (ISO-8601).
    pub created_at: String,
    /// Station-visit counter at creation time.
    pub visit: u32,
    /// Back-reference to the origin artifact, if any.
    pub source_ref: Option<String>,
    /// Unit slug that certified closure, if any.
    pub closed_by: Option<String>,
    /// Reply thread (`author: text` lines, rendered into wire replies on read).
    pub replies: Vec<String>,
}

impl FeedbackDoc {
    /// Build a fresh user-authored doc with sensible defaults for an HTTP
    /// create. The caller supplies the minted id, station, and content.
    pub fn new_user(id: String, station: String, title: String, body: String) -> Self {
        FeedbackDoc {
            id,
            title,
            body,
            status: FeedbackStatus::Pending,
            origin: FeedbackOrigin::UserVisual,
            severity: None,
            station: Some(station),
            author: "user".to_string(),
            created_at: now_iso8601(),
            visit: 0,
            source_ref: None,
            closed_by: None,
            replies: Vec::new(),
        }
    }

    /// Parse a raw document. `id` is the authoritative file stem; any `id:`
    /// field in the frontmatter is ignored in favour of it.
    pub fn parse(id: &str, raw: &str) -> Self {
        let (front, body) = split_frontmatter(raw);

        let mut title = String::new();
        let mut status = FeedbackStatus::Pending;
        let mut origin = FeedbackOrigin::UserVisual;
        let mut severity = None;
        let mut station = None;
        let mut author = String::from("user");
        let mut created_at = String::new();
        let mut visit = 0u32;
        let mut source_ref = None;
        let mut closed_by = None;
        let mut replies = Vec::new();

        let mut in_replies = false;
        for line in front.lines() {
            // Reply list items: indented `- "..."`.
            let trimmed = line.trim_start();
            if in_replies && trimmed.starts_with('-') {
                let item = unquote(trimmed.trim_start_matches('-').trim());
                if !item.is_empty() {
                    replies.push(item);
                }
                continue;
            }

            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = unquote(value.trim());
            in_replies = false;

            match key {
                "title" => title = value,
                "status" => status = FeedbackStatus::canonicalize(&value),
                "origin" => {
                    origin = parse_origin(&value).unwrap_or(FeedbackOrigin::UserVisual);
                }
                "severity" => severity = parse_severity(&value),
                "station" => station = (!value.is_empty()).then_some(value),
                "author" => {
                    if !value.is_empty() {
                        author = value;
                    }
                }
                "created_at" => created_at = value,
                "visit" => visit = value.parse().unwrap_or(0),
                "source_ref" => source_ref = (!value.is_empty()).then_some(value),
                "closed_by" => closed_by = (!value.is_empty()).then_some(value),
                "replies" => in_replies = true,
                _ => {}
            }
        }

        FeedbackDoc {
            id: id.to_string(),
            title,
            body: body.trim().to_string(),
            status,
            origin,
            severity,
            station,
            author,
            created_at,
            visit,
            source_ref,
            closed_by,
            replies,
        }
    }

    /// Render back to the on-disk markdown-with-frontmatter form.
    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        push_field(&mut out, "id", &self.id);
        push_field(&mut out, "station", self.station.as_deref().unwrap_or(""));
        push_field(&mut out, "status", self.status.as_str());
        push_field(&mut out, "origin", origin_token(self.origin));
        push_field(
            &mut out,
            "severity",
            self.severity.map(severity_token).unwrap_or(""),
        );
        push_field(&mut out, "title", &self.title);
        push_field(&mut out, "author", &self.author);
        push_field(&mut out, "created_at", &self.created_at);
        out.push_str(&format!("visit: {}\n", self.visit));
        push_field(
            &mut out,
            "source_ref",
            self.source_ref.as_deref().unwrap_or(""),
        );
        push_field(&mut out, "closed_by", self.closed_by.as_deref().unwrap_or(""));
        out.push_str("replies:\n");
        for r in &self.replies {
            out.push_str(&format!("  - {}\n", quote(r)));
        }
        out.push_str("---\n");
        if !self.body.is_empty() {
            out.push_str(&self.body);
            out.push('\n');
        }
        out
    }

    /// Whether this doc belongs to `station`. Items with no recorded station
    /// match every station (legacy-tolerant).
    pub fn matches_station(&self, station: &str) -> bool {
        match &self.station {
            Some(s) => s == station,
            None => true,
        }
    }

    /// Project to the wire shape.
    pub fn to_item(&self) -> FeedbackItem {
        FeedbackItem {
            feedback_id: self.id.clone(),
            title: self.title.clone(),
            body: self.body.clone(),
            status: self.status,
            origin: self.origin,
            severity: self.severity,
            author: self.author.clone(),
            author_type: self.origin.author_type(),
            created_at: self.created_at.clone(),
            visit: self.visit,
            source_ref: self.source_ref.clone(),
            closed_by: self.closed_by.clone(),
            resolution: None,
            replies: Vec::new(),
            inline_anchor: None,
            scope: None,
            iterations: Vec::new(),
            closure_reply: None,
            closure_reply_unread: None,
        }
    }
}

/// Mint the next `FB-NN` id given the existing ids. Picks `max(N)+1`,
/// zero-padded to two digits, so a sparse or out-of-order set still advances.
pub fn next_id<'a, I>(existing: I) -> String
where
    I: IntoIterator<Item = &'a String>,
{
    let max = existing
        .into_iter()
        .filter_map(|id| {
            id.strip_prefix("FB-")
                .or_else(|| id.strip_prefix("fb-"))
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);
    format!("FB-{:02}", max + 1)
}

/// Best-effort ISO-8601 UTC timestamp without a chrono dependency: seconds
/// since the Unix epoch formatted as an RFC-3339-ish marker. The exact format
/// is opaque to the wire contract (any `String` is valid `created_at`).
fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key}: {}\n", quote(value)));
}

fn origin_token(o: FeedbackOrigin) -> &'static str {
    match o {
        FeedbackOrigin::AdversarialReview => "adversarial-review",
        FeedbackOrigin::StudioReview => "studio-review",
        FeedbackOrigin::EngineReview => "engine-review",
        FeedbackOrigin::Drift => "drift",
        FeedbackOrigin::Discovery => "discovery",
        FeedbackOrigin::ExternalPr => "external-pr",
        FeedbackOrigin::ExternalMr => "external-mr",
        FeedbackOrigin::UserVisual => "user-visual",
        FeedbackOrigin::UserChat => "user-chat",
        FeedbackOrigin::UserQuestion => "user-question",
        FeedbackOrigin::UserRevisit => "user-revisit",
        FeedbackOrigin::Agent => "agent",
    }
}

fn parse_origin(s: &str) -> Option<FeedbackOrigin> {
    Some(match s {
        "adversarial-review" => FeedbackOrigin::AdversarialReview,
        "studio-review" => FeedbackOrigin::StudioReview,
        "engine-review" => FeedbackOrigin::EngineReview,
        "drift" => FeedbackOrigin::Drift,
        "discovery" => FeedbackOrigin::Discovery,
        "external-pr" => FeedbackOrigin::ExternalPr,
        "external-mr" => FeedbackOrigin::ExternalMr,
        "user-visual" => FeedbackOrigin::UserVisual,
        "user-chat" => FeedbackOrigin::UserChat,
        "user-question" => FeedbackOrigin::UserQuestion,
        "user-revisit" => FeedbackOrigin::UserRevisit,
        "agent" => FeedbackOrigin::Agent,
        _ => return None,
    })
}

fn severity_token(s: FeedbackSeverity) -> &'static str {
    match s {
        FeedbackSeverity::Blocker => "blocker",
        FeedbackSeverity::High => "high",
        FeedbackSeverity::Medium => "medium",
        FeedbackSeverity::Low => "low",
    }
}

fn parse_severity(s: &str) -> Option<FeedbackSeverity> {
    Some(match s {
        "blocker" => FeedbackSeverity::Blocker,
        "high" => FeedbackSeverity::High,
        "medium" => FeedbackSeverity::Medium,
        "low" => FeedbackSeverity::Low,
        _ => return None,
    })
}

/// Split a raw document into its frontmatter block and body. When no `---`
/// fence is present the whole input is treated as body.
fn split_frontmatter(raw: &str) -> (&str, &str) {
    let rest = match raw.strip_prefix("---\n") {
        Some(r) => r,
        None => match raw.strip_prefix("---\r\n") {
            Some(r) => r,
            None => return ("", raw),
        },
    };
    if let Some((front_end, body_start)) = find_closing_fence(rest) {
        (&rest[..front_end], &rest[body_start..])
    } else {
        ("", raw)
    }
}

/// Locate the closing `---` fence. Returns `(front_end, body_start)` byte
/// offsets into `rest`.
fn find_closing_fence(rest: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            return Some((offset, offset + line.len()));
        }
        offset += line.len();
    }
    None
}

/// Strip a single layer of surrounding double quotes.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].replace("\\\"", "\"")
    } else {
        s.to_string()
    }
}

/// Quote a value when it contains characters that would break the flat
/// frontmatter parser (a colon, leading dash, or quotes).
fn quote(s: &str) -> String {
    if s.contains(':') || s.contains('"') || s.starts_with('-') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_id_starts_at_one() {
        let empty: Vec<String> = vec![];
        assert_eq!(next_id(empty.iter()), "FB-01");
    }

    #[test]
    fn next_id_advances_past_max() {
        let ids = ["FB-01".to_string(), "FB-03".to_string()];
        assert_eq!(next_id(ids.iter()), "FB-04");
    }

    #[test]
    fn roundtrips_through_render_and_parse() {
        let mut doc = FeedbackDoc::new_user(
            "FB-07".into(),
            "frame".into(),
            "Fix: the spec".into(),
            "Body line one.\nBody line two.".into(),
        );
        doc.replies.push("user: please clarify".into());
        let rendered = doc.render();
        let back = FeedbackDoc::parse("FB-07", &rendered);
        assert_eq!(back.id, "FB-07");
        assert_eq!(back.title, "Fix: the spec");
        assert_eq!(back.status, FeedbackStatus::Pending);
        assert_eq!(back.station.as_deref(), Some("frame"));
        assert_eq!(back.author, "user");
        assert_eq!(back.replies, vec!["user: please clarify".to_string()]);
        assert!(back.body.contains("Body line one."));
        assert!(back.body.contains("Body line two."));
    }

    #[test]
    fn parses_body_only_document() {
        let doc = FeedbackDoc::parse("FB-09", "Just a body, no frontmatter.");
        assert_eq!(doc.body, "Just a body, no frontmatter.");
        // No station recorded → matches any station.
        assert!(doc.matches_station("anything"));
        assert_eq!(doc.status, FeedbackStatus::Pending);
    }

    #[test]
    fn to_item_derives_author_type() {
        let doc = FeedbackDoc::new_user("FB-01".into(), "frame".into(), "t".into(), "b".into());
        let item = doc.to_item();
        // user-visual origin → human author.
        assert_eq!(item.author_type, darkrun_api::AuthorType::Human);
        assert_eq!(item.origin, FeedbackOrigin::UserVisual);
    }

    #[test]
    fn station_filter() {
        let doc = FeedbackDoc::new_user("FB-01".into(), "build".into(), String::new(), String::new());
        assert!(doc.matches_station("build"));
        assert!(!doc.matches_station("frame"));
    }

    #[test]
    fn every_origin_token_round_trips() {
        use FeedbackOrigin::*;
        for o in [
            AdversarialReview, StudioReview, EngineReview, Drift, Discovery, ExternalPr,
            ExternalMr, UserVisual, UserChat, UserQuestion, UserRevisit, Agent,
        ] {
            assert_eq!(parse_origin(origin_token(o)), Some(o), "{o:?} round-trips");
        }
        assert_eq!(parse_origin("not-a-known-origin"), None);
    }

    #[test]
    fn every_severity_token_round_trips() {
        use FeedbackSeverity::*;
        for s in [Blocker, High, Medium, Low] {
            assert_eq!(parse_severity(severity_token(s)), Some(s), "{s:?} round-trips");
        }
        assert_eq!(parse_severity("catastrophic"), None);
    }

    #[test]
    fn quote_and_unquote_handle_special_chars() {
        // quote() wraps values that would break the flat parser.
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("has: colon"), "\"has: colon\"");
        assert_eq!(quote("-leading-dash"), "\"-leading-dash\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("line\nbreak"), "\"line\nbreak\"");
        // unquote() strips one layer + unescapes; passes bare values through.
        assert_eq!(unquote("\"wrapped\""), "wrapped");
        assert_eq!(unquote("\"a\\\"b\""), "a\"b");
        assert_eq!(unquote("bare"), "bare");
        assert_eq!(unquote("\""), "\""); // a lone quote is too short to strip
    }

    #[test]
    fn split_frontmatter_handles_crlf_and_unclosed_fences() {
        // CRLF-fenced frontmatter parses its fields.
        let crlf = "---\r\nstatus: pending\r\n---\r\nbody here";
        let doc = FeedbackDoc::parse("FB-1", crlf);
        assert_eq!(doc.body, "body here");
        // An opened-but-unclosed fence falls back to treating it all as body.
        let unclosed = "---\ntitle: x\nstill front, no close";
        let d2 = FeedbackDoc::parse("FB-2", unclosed);
        assert!(d2.body.contains("still front"));
        assert!(d2.title.is_empty(), "no closing fence → nothing parsed as frontmatter");
    }

    #[test]
    fn parse_reads_all_fields_and_tolerates_junk() {
        let raw = "---\n\
            title: \"Wire: importer\"\n\
            status: addressed\n\
            origin: drift\n\
            severity: high\n\
            station: build\n\
            author: agent\n\
            created_at: 2026-06-06T00:00:00Z\n\
            visit: 4\n\
            source_ref: payment.rs:42\n\
            closed_by: u3\n\
            bogus_key: ignored\n\
            visit_again: nope\n\
            replies:\n  - \"first reply\"\n  - \"second\"\n\
            ---\nThe finding body.";
        let doc = FeedbackDoc::parse("FB-12", raw);
        assert_eq!(doc.title, "Wire: importer");
        assert_eq!(doc.origin, FeedbackOrigin::Drift);
        assert_eq!(doc.severity, Some(FeedbackSeverity::High));
        assert_eq!(doc.station.as_deref(), Some("build"));
        assert_eq!(doc.author, "agent");
        assert_eq!(doc.visit, 4);
        assert_eq!(doc.source_ref.as_deref(), Some("payment.rs:42"));
        assert_eq!(doc.closed_by.as_deref(), Some("u3"));
        assert_eq!(doc.replies, vec!["first reply".to_string(), "second".to_string()]);
        assert_eq!(doc.body, "The finding body.");
        // An unknown origin token falls back to the user-visual default.
        let fb = FeedbackDoc::parse("FB-13", "---\norigin: mystery\nvisit: oops\n---\n");
        assert_eq!(fb.origin, FeedbackOrigin::UserVisual);
        assert_eq!(fb.visit, 0); // unparseable visit → 0
    }
}
