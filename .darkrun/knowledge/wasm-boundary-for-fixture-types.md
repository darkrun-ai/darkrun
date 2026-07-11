---
topic: wasm-boundary-for-fixture-types
created_at: 2026-07-11T15:46:22.291318+00:00
updated_at: 2026-07-11T15:46:22.291318+00:00
---
web/site (crate darkrun-site) depends on darkrun-ui, darkrun-api, darkrun-content, darkrun-core — never darkrun-mcp (unconditional nix/tokio/ureq/rmcp deps make it and anything depending on it, including crates/darkrun-sim, non-wasm). TickResult/RunAction/Position are Serialize-ONLY (no Deserialize, position.rs:69,243,252), so a replay fixture cannot round-trip engine types into the site. Any recorded-transcript payload the site replays needs a hand-rolled wasm-safe serde schema living in a wasm-clean crate (darkrun-core is the established home: its only native dep nix is cfg(unix)-gated for domain types). Site fixture-embedding precedents: include_str! (web/site/src/content.rs, 18+ call sites) and rust_embed (crates/darkrun-content/src/loader.rs:18-30); no JSON/transcript asset precedent exists yet. CI gap: no workflow builds web/site for wasm32 (ci.yml's wasm-app job scopes -p darkrun-app only); a site-consuming feature has no CI gate until deploy-web.yml runs.
