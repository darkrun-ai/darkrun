//! SEO artifacts: sitemap.xml, robots.txt, and the RSS / Atom / JSON feeds.
//!
//! These are pure string builders over the embedded corpora and the route
//! table, so they are unit-testable and compile to wasm. The native static-site
//! generator ([`crate::bin`]-side) writes their output to disk.

use crate::content::{self, CONCEPTS, DOCS, GUIDES, POSTS};
use crate::route::Route;

/// Canonical site origin (no trailing slash).
pub const SITE_URL: &str = "https://darkrun.ai";

/// The site's human name.
pub const SITE_NAME: &str = "darkrun";

/// One-line site description for feed metadata.
pub const SITE_DESCRIPTION: &str = "An agentic assembly line for your business.";

/// Build `sitemap.xml` covering every concrete route on the site.
pub fn sitemap() -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for path in Route::all_paths() {
        out.push_str("  <url><loc>");
        out.push_str(SITE_URL);
        out.push_str(&xml_escape(&path));
        out.push_str("</loc></url>\n");
    }
    out.push_str("</urlset>\n");
    out
}

/// Build `robots.txt`: allow everything (including the major AI crawlers) and
/// point at the sitemap and feeds.
pub fn robots() -> String {
    format!(
        "# darkrun robots.txt\n\
         User-agent: *\n\
         Allow: /\n\n\
         # AI crawlers welcome\n\
         User-agent: GPTBot\nAllow: /\n\
         User-agent: ClaudeBot\nAllow: /\n\
         User-agent: anthropic-ai\nAllow: /\n\
         User-agent: Google-Extended\nAllow: /\n\
         User-agent: PerplexityBot\nAllow: /\n\
         User-agent: CCBot\nAllow: /\n\n\
         # Feeds: {site}/feed.xml (RSS) \u{00b7} {site}/atom.xml (Atom) \u{00b7} {site}/feed.json (JSON)\n\
         Sitemap: {site}/sitemap.xml\n",
        site = SITE_URL,
    )
}

/// Build an RSS 2.0 feed of the blog posts.
pub fn feed_rss() -> String {
    let mut items = String::new();
    for post in POSTS {
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        let pub_date = rfc2822_date(post.date);
        items.push_str(&format!(
            "    <item>\n      <title>{title}</title>\n      <link>{link}</link>\n      <guid>{link}</guid>\n      <pubDate>{pub_date}</pubDate>\n      <description>{summary}</description>\n    </item>\n",
            title = xml_escape(post.title),
            link = xml_escape(&link),
            pub_date = xml_escape(&pub_date),
            summary = xml_escape(post.summary),
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\">\n  <channel>\n    <title>{name}</title>\n    <link>{site}</link>\n    <description>{desc}</description>\n{items}  </channel>\n</rss>\n",
        name = xml_escape(SITE_NAME),
        site = SITE_URL,
        desc = xml_escape(SITE_DESCRIPTION),
    )
}

/// Build an Atom 1.0 feed of the blog posts.
pub fn feed_atom() -> String {
    let feed_updated = POSTS
        .iter()
        .map(|post| post.date)
        .max()
        .map(rfc3339_date)
        .map(|d| xml_escape(&d))
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());

    let mut entries = String::new();
    for post in POSTS {
        let link = format!("{SITE_URL}/blog/{}", post.slug);
        entries.push_str(&format!(
            "  <entry>\n    <title>{title}</title>\n    <id>{link}</id>\n    <link href=\"{link}\"/>\n    <updated>{updated}</updated>\n    <summary>{summary}</summary>\n  </entry>\n",
            title = xml_escape(post.title),
            link = xml_escape(&link),
            updated = xml_escape(&rfc3339_date(post.date)),
            summary = xml_escape(post.summary),
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n  <title>{name}</title>\n  <id>{site}/</id>\n  <link href=\"{site}/\"/>\n  <updated>{updated}</updated>\n{entries}</feed>\n",
        name = xml_escape(SITE_NAME),
        site = SITE_URL,
        updated = feed_updated,
    )
}

/// Build a JSON Feed 1.1 document of the blog posts.
pub fn feed_json() -> String {
    let items: Vec<String> = POSTS
        .iter()
        .map(|post| {
            let link = format!("{SITE_URL}/blog/{}", post.slug);
            format!(
                "{{\"id\":{id},\"url\":{url},\"title\":{title},\"summary\":{summary}}}",
                id = json_string(&link),
                url = json_string(&link),
                title = json_string(post.title),
                summary = json_string(post.summary),
            )
        })
        .collect();
    format!(
        "{{\"version\":\"https://jsonfeed.org/version/1.1\",\
         \"title\":{name},\"home_page_url\":{home},\"feed_url\":{feed},\
         \"description\":{desc},\"items\":[{items}]}}",
        name = json_string(SITE_NAME),
        home = json_string(SITE_URL),
        feed = json_string(&format!("{SITE_URL}/feed.json")),
        desc = json_string(SITE_DESCRIPTION),
        items = items.join(","),
    )
}

/// Render a `YYYY-MM-DD` post date as RFC 3339 (midnight UTC) — the format
/// Atom's `<updated>` requires. Non-conforming input passes through unchanged.
fn rfc3339_date(date: &str) -> String {
    if date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-' {
        format!("{date}T00:00:00Z")
    } else {
        date.to_string()
    }
}

/// Render a `YYYY-MM-DD` post date as RFC 2822 (midnight UTC) — the format
/// RSS's `<pubDate>` requires. Non-conforming input passes through unchanged.
fn rfc2822_date(date: &str) -> String {
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    let [y, m, d] = parts[..] else {
        return date.to_string();
    };
    let (Ok(year), Ok(month), Ok(day)) = (y.parse::<i64>(), m.parse::<u32>(), d.parse::<u32>())
    else {
        return date.to_string();
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return date.to_string();
    }
    // Day of week via Zeller's congruence (Gregorian).
    let (zy, zm) = if month < 3 { (year - 1, month + 12) } else { (year, month) };
    let k = zy % 100;
    let j = zy / 100;
    let h = (day as i64 + (13 * (zm as i64 + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    const DOW: [&str; 7] = ["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {} 00:00:00 +0000",
        DOW[h as usize], day, MON[(month - 1) as usize], year
    )
}

/// Minimal XML text escaping.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// JSON string literal escaping (quotes, backslashes, control chars).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── JSON-LD structured data (schema.org) ────────────────────────────────────

/// The site-level JSON-LD: an `Organization` + a `WebSite` carrying a
/// `SearchAction` (the docs search). Embedded statically in `index.html` and
/// kept in sync by a unit test, so the crawler-facing block and the builder
/// can't drift apart.
pub fn json_ld_site() -> String {
    serde_json::json!({
        "@context": "https://schema.org",
        "@graph": [
            {
                "@type": "Organization",
                "@id": format!("{SITE_URL}/#org"),
                "name": SITE_NAME,
                "url": SITE_URL,
                "logo": format!("{SITE_URL}/assets/favicon.png"),
            },
            {
                "@type": "WebSite",
                "@id": format!("{SITE_URL}/#website"),
                "name": SITE_NAME,
                "description": SITE_DESCRIPTION,
                "url": SITE_URL,
                "publisher": { "@id": format!("{SITE_URL}/#org") },
                "potentialAction": {
                    "@type": "SearchAction",
                    "target": format!("{SITE_URL}/docs?q={{search_term_string}}"),
                    "query-input": "required name=search_term_string",
                },
            },
        ],
    })
    .to_string()
}

/// Per-document JSON-LD: a `BlogPosting` for dated posts, a `TechArticle` for
/// docs/concepts/guides. Injected into `<head>` when the page mounts.
pub fn json_ld_article(doc: &crate::content::Doc, path: &str) -> String {
    let kind = if doc.date.is_empty() { "TechArticle" } else { "BlogPosting" };
    let mut obj = serde_json::json!({
        "@context": "https://schema.org",
        "@type": kind,
        "headline": doc.title,
        "description": doc.summary,
        "url": format!("{SITE_URL}{path}"),
        "author": { "@id": format!("{SITE_URL}/#org") },
        "publisher": { "@id": format!("{SITE_URL}/#org") },
    });
    if !doc.date.is_empty() {
        obj["datePublished"] = serde_json::Value::String(doc.date.to_string());
    }
    obj.to_string()
}

// ── Per-page <head> metadata ────────────────────────────────────────────────
//
// The site is a client-rendered SPA served from one `index.html` whose static
// `<head>` names the HOMEPAGE (its canonical, title, description). Without a
// per-route override, every one of the ~50 subpages would ship that same head,
// so each would self-canonicalize to `/` and share the homepage title, which
// erases their SEO (a crawler folds them all into the homepage). The Shell
// drives [`head_sync_script`] on every navigation to give each route its OWN
// canonical + title + description.

/// The homepage `<title>` and description, re-asserted on `/` so the homepage
/// owns its head too (rather than inheriting a stale one from a prior route).
const HOME_TITLE: &str = "darkrun: the dark factory harness";
const HOME_DESCRIPTION: &str =
    "darkrun is a dark factory harness: it runs your agents lights-out as an ordered line of \
     stations that take work from raw intent to a shipped, hardened outcome.";

/// The `<head>` metadata a single route owns: its own canonical URL, `<title>`,
/// and description. Emitting these PER ROUTE is what stops every subpage from
/// self-canonicalizing to the homepage.
#[derive(Debug, Clone, PartialEq)]
pub struct PageMeta {
    /// The absolute canonical URL for this exact path.
    pub canonical: String,
    /// The full `<title>` text.
    pub title: String,
    /// The meta description.
    pub description: String,
}

/// The canonical URL for a route path: the site origin plus the route's OWN
/// path, so each page self-identifies. The homepage is `/`; every other page
/// keeps its own path (trailing slash trimmed so canonicals stay stable).
pub fn canonical_url(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        format!("{SITE_URL}/")
    } else {
        format!("{SITE_URL}{trimmed}")
    }
}

/// Resolve the full `<head>` metadata for a route path. The subpage `<title>`
/// carries a `... · darkrun` suffix; the homepage stands alone.
pub fn page_meta(path: &str) -> PageMeta {
    let is_home = path.trim_end_matches('/').is_empty();
    let (page_title, description) = title_and_description(path);
    let title = if is_home {
        page_title
    } else {
        format!("{page_title} \u{00b7} darkrun")
    };
    PageMeta {
        canonical: canonical_url(path),
        title,
        description,
    }
}

/// The page-specific `(title, description)` for a route path, drawn from the
/// content corpora for the dynamic routes and a small table for the fixed pages.
/// Unknown paths (the catch-all NotFound) fall back to the site defaults.
fn title_and_description(path: &str) -> (String, String) {
    let raw = path.trim_end_matches('/');
    let p = if raw.is_empty() { "/" } else { raw };

    // Dynamic content routes (most specific first) resolve their own metadata
    // straight from the embedded corpus.
    if let Some(slug) = p.strip_prefix("/docs/") {
        if let Some(d) = content::find(DOCS, slug) {
            return (d.title.to_string(), d.summary.to_string());
        }
    }
    if let Some(slug) = p.strip_prefix("/blog/") {
        if let Some(d) = content::find(POSTS, slug) {
            return (d.title.to_string(), d.summary.to_string());
        }
    }
    if let Some(rest) = p.strip_prefix("/factories/") {
        if let Some((factory, station)) = rest.split_once("/stations/") {
            if let Ok(f) = darkrun_content::load_validated(factory) {
                if let Some(st) = f.station(station) {
                    return (
                        format!("{} station", st.label()),
                        format!("The {} station of the {} factory.", st.label(), f.name()),
                    );
                }
            }
            return (
                format!("{} station", humanize(station)),
                format!("A station of the {} factory.", humanize(factory)),
            );
        }
        if let Ok(f) = darkrun_content::load_validated(rest) {
            let desc = f.frontmatter.description.trim();
            let desc = if desc.is_empty() {
                format!("The {} factory and the stations it walks.", f.name())
            } else {
                desc.to_string()
            };
            return (format!("{} factory", f.name()), desc);
        }
        return (
            format!("{} factory", humanize(rest)),
            format!("The {} factory and the stations it walks.", humanize(rest)),
        );
    }
    if let Some(phase) = p.strip_prefix("/methodology/") {
        if let Some(d) = content::find(CONCEPTS, phase) {
            return (d.title.to_string(), d.summary.to_string());
        }
        return (
            format!("The {} phase", humanize(phase)),
            format!(
                "The {} phase of the six-phase machine every station walks.",
                humanize(phase)
            ),
        );
    }

    // The prose guide pages carry their own title/summary from the GUIDES corpus.
    let guide_slug = match p {
        "/start-here" => Some("start-here"),
        "/how-it-works" => Some("how-it-works"),
        "/big-picture" => Some("big-picture"),
        "/workflows" => Some("workflows"),
        "/about" => Some("about"),
        _ => None,
    };
    if let Some(slug) = guide_slug {
        if let Some(d) = content::find(GUIDES, slug) {
            return (d.title.to_string(), d.summary.to_string());
        }
    }

    // Index + standalone pages: a fixed table of real, page-specific copy.
    match p {
        "/" => (HOME_TITLE.to_string(), HOME_DESCRIPTION.to_string()),
        "/factories" => (
            "Factories".to_string(),
            "Every darkrun factory and the stations it walks: the methodologies that drive a run."
                .to_string(),
        ),
        "/docs" => (
            "Documentation".to_string(),
            "Install darkrun, start a run, review at the gates, and ship. The darkrun reference."
                .to_string(),
        ),
        "/methodology" => (
            "Methodology".to_string(),
            "The anti-rework thesis: one universal station slot, and the six-phase machine every \
             station walks."
                .to_string(),
        ),
        "/glossary" => (
            "Glossary".to_string(),
            "The darkrun vocabulary: factories, stations, units, passes, gates, and the rest of \
             the factory model."
                .to_string(),
        ),
        "/lifecycles" => (
            "Lifecycles".to_string(),
            "How a run moves through its stations, from raw intent to a sealed, shipped outcome."
                .to_string(),
        ),
        "/blog" => (
            "Blog".to_string(),
            "Notes on darkrun: the dark factory model, agent orchestration, and shipping with \
             checkpoints instead of babysitting."
                .to_string(),
        ),
        "/changelog" => (
            "Changelog".to_string(),
            "What changed in each darkrun release.".to_string(),
        ),
        "/paper" => (
            "The paper".to_string(),
            "The darkrun thesis in long form: why an ordered assembly line beats an unstructured \
             agent loop."
                .to_string(),
        ),
        "/templates" => (
            "Factory templates".to_string(),
            "Starting-point factory templates you can scaffold and adapt to your own line."
                .to_string(),
        ),
        "/browse" => (
            "Browse a workspace".to_string(),
            "Open a published darkrun workspace and read its runs, stations, and units."
                .to_string(),
        ),
        "/review" => (
            "Review".to_string(),
            "Review runs in the darkrun desktop app: approve at the gates, or route rework as \
             drift."
                .to_string(),
        ),
        "/preview" => (
            "Session preview".to_string(),
            "A gallery of darkrun's review surfaces, rendered read-only from the real API types."
                .to_string(),
        ),
        "/privacy" => (
            "Privacy".to_string(),
            "The darkrun privacy policy.".to_string(),
        ),
        "/terms" => (
            "Terms".to_string(),
            "The darkrun terms of service.".to_string(),
        ),
        _ => (SITE_NAME.to_string(), SITE_DESCRIPTION.to_string()),
    }
}

/// Turn a slug into a display label (`pressure_tester` / `pressure-tester` →
/// `Pressure Tester`), for the dynamic routes that have no corpus entry.
fn humanize(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a small idempotent script that syncs the document `<head>` to this
/// page's metadata: the `<title>`, the single `<link rel="canonical">`, and the
/// description / Open Graph / Twitter tags. It SELECTS the existing element and
/// updates it (creating one only when absent), so the SPA never leaves a second,
/// stale canonical behind the one baked into `index.html`. Every interpolated
/// value is a JSON string literal (via [`json_string`]), which is also a valid
/// JS string, so titles/descriptions with quotes can't break out of the script.
pub fn head_sync_script(meta: &PageMeta) -> String {
    let title = json_string(&meta.title);
    let canonical = json_string(&meta.canonical);
    let description = json_string(&meta.description);
    format!(
        "(function(){{\
           document.title={title};\
           function m(sel,set){{var el=document.head.querySelector(sel);\
             if(!el){{el=document.createElement('meta');set(el);document.head.appendChild(el);}}return el;}}\
           var c=document.head.querySelector('link[rel=\"canonical\"]');\
           if(!c){{c=document.createElement('link');c.setAttribute('rel','canonical');document.head.appendChild(c);}}\
           c.setAttribute('href',{canonical});\
           m('meta[name=\"description\"]',function(e){{e.setAttribute('name','description');}}).setAttribute('content',{description});\
           m('meta[property=\"og:title\"]',function(e){{e.setAttribute('property','og:title');}}).setAttribute('content',{title});\
           m('meta[property=\"og:description\"]',function(e){{e.setAttribute('property','og:description');}}).setAttribute('content',{description});\
           m('meta[property=\"og:url\"]',function(e){{e.setAttribute('property','og:url');}}).setAttribute('content',{canonical});\
           m('meta[name=\"twitter:title\"]',function(e){{e.setAttribute('name','twitter:title');}}).setAttribute('content',{title});\
           m('meta[name=\"twitter:description\"]',function(e){{e.setAttribute('name','twitter:description');}}).setAttribute('content',{description});\
         }})();"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_owns_a_distinct_canonical() {
        // The core fix: each of the ~50 routes must carry its OWN canonical (and
        // title + description), so no subpage self-canonicalizes to the homepage.
        let paths = Route::all_paths();
        let mut seen = std::collections::HashSet::new();
        for path in &paths {
            let meta = page_meta(path);
            assert_eq!(meta.canonical, canonical_url(path), "canonical follows the path: {path}");
            assert!(meta.canonical.starts_with(SITE_URL), "canonical rooted at the origin: {path}");
            assert!(!meta.title.is_empty(), "a title for {path}");
            assert!(!meta.description.is_empty(), "a description for {path}");
            assert!(
                seen.insert(meta.canonical.clone()),
                "duplicate canonical for {path}: {}",
                meta.canonical
            );
            if path != "/" {
                assert_ne!(
                    meta.canonical,
                    format!("{SITE_URL}/"),
                    "subpage {path} self-canonicalizes to the homepage"
                );
            }
        }
        assert_eq!(page_meta("/").canonical, format!("{SITE_URL}/"), "homepage canonical is the origin root");
    }

    #[test]
    fn subpages_carry_their_own_title_and_description_not_the_homepage_one() {
        let home = page_meta("/");
        // A doc page pulls its OWN title/description straight from the corpus.
        let doc = crate::content::DOCS.first().expect("a doc exists");
        let meta = page_meta(&format!("/docs/{}", doc.slug));
        assert!(meta.title.contains(doc.title), "doc title used: {}", meta.title);
        assert!(meta.title.ends_with("\u{00b7} darkrun"), "subpage title suffix: {}", meta.title);
        assert_ne!(meta.title, home.title, "doc title differs from home");
        assert_eq!(meta.description, doc.summary, "doc description is the doc summary");
        assert_ne!(meta.description, home.description, "doc description differs from home");
    }

    #[test]
    fn head_sync_script_updates_one_canonical_and_the_title() {
        let meta = page_meta("/docs/getting-started");
        let js = head_sync_script(&meta);
        // It SELECTS the existing canonical (idempotent), never blindly appends.
        assert!(js.contains("link[rel=\"canonical\"]"), "targets the canonical link: {js}");
        assert!(js.contains("querySelector"), "selects rather than appends: {js}");
        assert!(js.contains("document.title="), "sets the title: {js}");
        // The page's own canonical + title are embedded (JSON-escaped).
        assert!(js.contains("https://darkrun.ai/docs/getting-started"), "carries the page canonical: {js}");
    }

    #[test]
    fn humanize_titlecases_slugs() {
        assert_eq!(humanize("pressure_tester"), "Pressure Tester");
        assert_eq!(humanize("pressure-tester"), "Pressure Tester");
        assert_eq!(humanize("build"), "Build");
    }

    #[test]
    fn sitemap_lists_landing_and_factories_index() {
        let xml = sitemap();
        assert!(xml.contains("<loc>https://darkrun.ai/</loc>"));
        assert!(xml.contains("<loc>https://darkrun.ai/factories</loc>"));
        assert!(xml.trim_end().ends_with("</urlset>"));
    }

    #[test]
    fn sitemap_includes_dynamic_factory_routes() {
        let xml = sitemap();
        // The embedded corpus ships at least the `software` factory.
        assert!(xml.contains("/factories/software"));
    }

    #[test]
    fn robots_allows_all_and_points_at_sitemap() {
        let txt = robots();
        assert!(txt.contains("User-agent: *"));
        assert!(txt.contains("Allow: /"));
        assert!(txt.contains("Sitemap: https://darkrun.ai/sitemap.xml"));
        assert!(txt.contains("ClaudeBot"));
    }

    #[test]
    fn feeds_render_every_post() {
        let rss = feed_rss();
        let atom = feed_atom();
        let json = feed_json();
        for post in POSTS {
            assert!(rss.contains(post.title), "rss missing {}", post.title);
            assert!(atom.contains(post.title), "atom missing {}", post.title);
            assert!(json.contains(post.title), "json missing {}", post.title);
        }
        assert!(rss.starts_with("<?xml"));
        assert!(atom.contains("<feed"));
        assert!(json.contains("jsonfeed.org"));
    }

    #[test]
    fn xml_escaping_is_applied() {
        assert_eq!(xml_escape("a & b < c"), "a &amp; b &lt; c");
    }

    #[test]
    fn json_string_escapes_quotes() {
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn xml_escape_handles_all_five_entities() {
        assert_eq!(
            xml_escape("&<>\"'"),
            "&amp;&lt;&gt;&quot;&apos;"
        );
    }

    #[test]
    fn xml_escape_ampersand_first_avoids_double_escaping() {
        // `<` becomes `&lt;`; the `&` it introduces must not be re-escaped.
        assert_eq!(xml_escape("<"), "&lt;");
        assert_eq!(xml_escape(">"), "&gt;");
        assert_eq!(xml_escape("\""), "&quot;");
        assert_eq!(xml_escape("'"), "&apos;");
    }

    #[test]
    fn xml_escape_passes_through_plain_text() {
        assert_eq!(xml_escape("plain text 123"), "plain text 123");
        assert_eq!(xml_escape(""), "");
    }

    #[test]
    fn xml_escape_preserves_unicode() {
        assert_eq!(xml_escape("café · darkrun"), "café · darkrun");
    }

    #[test]
    fn xml_escape_repeated_specials() {
        assert_eq!(xml_escape("a&&b"), "a&amp;&amp;b");
        assert_eq!(xml_escape("<<>>"), "&lt;&lt;&gt;&gt;");
    }

    #[test]
    fn json_string_wraps_in_quotes() {
        assert_eq!(json_string("hi"), "\"hi\"");
        assert_eq!(json_string(""), "\"\"");
    }

    #[test]
    fn json_string_escapes_backslash() {
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn json_string_escapes_whitespace_controls() {
        assert_eq!(json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string("a\rb"), "\"a\\rb\"");
        assert_eq!(json_string("a\tb"), "\"a\\tb\"");
    }

    #[test]
    fn json_string_escapes_low_control_chars_as_unicode() {
        // A NUL and a vertical tab fall to the \u00xx branch.
        assert_eq!(json_string("\u{0}"), "\"\\u0000\"");
        assert_eq!(json_string("\u{b}"), "\"\\u000b\"");
        assert_eq!(json_string("\u{1f}"), "\"\\u001f\"");
    }

    #[test]
    fn json_string_passes_through_unicode_above_control_range() {
        // 0x20 and above are emitted verbatim (no escaping of normal unicode).
        assert_eq!(json_string("é·🚀"), "\"é·🚀\"");
        assert_eq!(json_string(" "), "\" \"");
    }

    #[test]
    fn json_string_does_not_escape_forward_slash() {
        // Forward slashes are legal unescaped in JSON; our builder leaves them.
        assert_eq!(json_string("a/b"), "\"a/b\"");
    }

    #[test]
    fn json_string_combined() {
        assert_eq!(json_string("\"\\\n"), "\"\\\"\\\\\\n\"");
    }
    // ── JSON-LD ─────────────────────────────────────────────────────────────

    #[test]
    fn site_json_ld_is_valid_and_carries_org_website_and_search() {
        let v: serde_json::Value = serde_json::from_str(&json_ld_site()).expect("valid JSON");
        let graph = v["@graph"].as_array().expect("graph");
        assert!(graph.iter().any(|n| n["@type"] == "Organization"));
        let site = graph.iter().find(|n| n["@type"] == "WebSite").expect("WebSite");
        assert_eq!(site["potentialAction"]["@type"], "SearchAction");
    }

    #[test]
    fn index_html_embeds_the_same_site_json_ld() {
        // The static block in index.html and the builder must agree — compared
        // as parsed JSON so key order can't drift them apart.
        let html = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/index.html"));
        let start = html.find(r#"<script type="application/ld+json" id="ld-site">"#)
            .expect("index.html carries the site JSON-LD block");
        let rest = &html[start..];
        let open = rest.find('>').unwrap() + 1;
        let close = rest.find("</script>").unwrap();
        let embedded: serde_json::Value =
            serde_json::from_str(&rest[open..close]).expect("embedded block parses");
        let built: serde_json::Value = serde_json::from_str(&json_ld_site()).unwrap();
        assert_eq!(embedded, built, "index.html JSON-LD drifted from seo::json_ld_site()");
    }

    #[test]
    fn article_json_ld_distinguishes_posts_from_docs() {
        let post = crate::content::POSTS.first().expect("a post exists");
        let v: serde_json::Value =
            serde_json::from_str(&json_ld_article(post, "/blog/x")).unwrap();
        assert_eq!(v["@type"], "BlogPosting");
        assert!(v["datePublished"].is_string());

        let doc = crate::content::DOCS.first().expect("a doc exists");
        let v: serde_json::Value =
            serde_json::from_str(&json_ld_article(doc, "/docs/x")).unwrap();
        assert_eq!(v["@type"], "TechArticle");
        assert!(v.get("datePublished").is_none());
        assert_eq!(v["url"], format!("{SITE_URL}/docs/x"));
    }


    #[test]
    fn feed_dates_render_in_the_required_formats() {
        assert_eq!(rfc3339_date("2026-06-01"), "2026-06-01T00:00:00Z");
        assert_eq!(rfc2822_date("2026-06-01"), "Mon, 01 Jun 2026 00:00:00 +0000");
        assert_eq!(rfc2822_date("2024-02-29"), "Thu, 29 Feb 2024 00:00:00 +0000");
        // Non-conforming input passes through rather than panicking.
        assert_eq!(rfc3339_date("soon"), "soon");
        assert_eq!(rfc2822_date("soon"), "soon");
        // The feeds carry the fields.
        assert!(feed_rss().contains("<pubDate>"));
        let atom = feed_atom();
        assert!(atom.contains("<updated>"));
        assert!(atom.contains("T00:00:00Z"));
    }

}
