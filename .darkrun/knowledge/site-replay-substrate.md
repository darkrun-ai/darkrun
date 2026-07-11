---
topic: site-replay-substrate
created_at: 2026-07-02T23:52:20.812980+00:00
updated_at: 2026-07-02T23:52:20.812980+00:00
---
web/site is a client-side Dioxus wasm SPA (dioxus-router; darkrun-site-gen emits SEO artifacts only — it is NOT a pre-rendered SSG). Record/replay-without-a-live-engine is its established architecture: `/preview` renders real darkrun-api session payload fixtures with an explicit no-live-feed banner; `/browse` fetches a repo's committed `.darkrun/` tree over CORS HTTP and re-derives state client-side via darkrun-core (which compiles to wasm); the statusline demo embeds an offline ANSI→HTML snapshot. There is no shared normalized statusline state type — the CLI renders ANSI inline from StateStore (`crates/darkrun-cli/src/statusline.rs`), so a web statusline is a new projection best built from shared darkrun-ui components (the station strip / phase pipeline / unit DAG that `/browse` draws), not the CLI renderer. A replay player belongs in web/site as a new Route variant modeled on `/preview`; web/app (app.darkrun.ai) is the separate live-relay wasm app and is NOT where replay belongs.
