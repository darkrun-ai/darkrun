//! darkrun-site-gen — the native static-site generator.
//!
//! Writes the SEO artifacts (sitemap.xml, robots.txt, feed.xml, atom.xml,
//! feed.json) into an output directory alongside a `routes.txt` manifest of
//! every concrete route. Reuses the pure builders in [`darkrun_site::seo`], so
//! it stays in lockstep with the website's route table and content corpus.
//!
//! Usage: `darkrun-site-gen [OUT_DIR]` (defaults to `web/site/dist`).

use std::fs;
use std::path::PathBuf;

use darkrun_site::route::Route;
use darkrun_site::seo;

fn main() -> std::io::Result<()> {
    let out: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("web/site/dist"));
    fs::create_dir_all(&out)?;

    let files: [(&str, String); 5] = [
        ("sitemap.xml", seo::sitemap()),
        ("robots.txt", seo::robots()),
        ("feed.xml", seo::feed_rss()),
        ("atom.xml", seo::feed_atom()),
        ("feed.json", seo::feed_json()),
    ];
    for (name, body) in &files {
        fs::write(out.join(name), body)?;
        println!("wrote {}", out.join(name).display());
    }

    // A manifest of every concrete route, for downstream pre-rendering / checks.
    let manifest = Route::all_paths().join("\n");
    fs::write(out.join("routes.txt"), format!("{manifest}\n"))?;
    println!("wrote {} ({} routes)", out.join("routes.txt").display(), Route::all_paths().len());

    Ok(())
}
