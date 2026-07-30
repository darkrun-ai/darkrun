//! Scratch repro: render the station strip in the exact state of the reported
//! screenshot (frame/specify done, shape current, build/prove/harden pending).

use darkrun_ui::components::pipeline::StationPipeline;
use darkrun_ui::components::station_strip::{StationItem, StationStatus, StationStrip};
use darkrun_ui::kinds::Phase;
use darkrun_ui::tokens;
use dioxus::prelude::*;

fn page() -> Element {
    let stations = vec![
        StationItem::new("frame", StationStatus::Done),
        StationItem::new("specify", StationStatus::Done),
        StationItem::new("shape", StationStatus::Current),
        StationItem::new("build", StationStatus::Pending),
        StationItem::new("prove", StationStatus::Pending),
        StationItem::new("harden", StationStatus::Pending),
    ];
    rsx! {
        style { "{tokens::THEME_CSS}" }
        div {
            style: "padding:32px;background:var(--dr-surface-base);min-height:100vh;",
            StationStrip { stations }
            div { style: "text-align:center;color:var(--dr-text);font-family:var(--dr-font-mono);",
                "station: shape"
            }
            StationPipeline { dots: darkrun_ui::components::pipeline::strip_for(Some(Phase::Manufacture)), labels: true }
        }
    }
}

fn main() {
    let mut dom = VirtualDom::new(page);
    dom.rebuild_in_place();
    let body = dioxus_ssr::render(&dom);
    let html = format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body style=\"margin:0\">{body}</body></html>");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/strip_repro.html");
    std::fs::write(&path, html).expect("write repro");
    println!("{}", path.display());
}
