# darkrun-web container: the OAuth broker + the static Dioxus site in one image.
#
# Stage 1 builds the wasm SPA (via dx) and the darkrun-web server binary.
# Stage 2 is a slim runtime that serves both. C-free: rustls server (no openssl),
# ca-certificates the only runtime apt package.

# ── Builder ──────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS builder
WORKDIR /app

# wasm target + Dioxus CLI for the SPA build.
RUN rustup target add wasm32-unknown-unknown \
    && cargo install dioxus-cli --version "^0.7" --locked

COPY . .

# Build the static site (wasm + assets). dx 0.7 emits the public bundle under
# target/dx/<app>/release/web/public — adjust the copy below if your dx version
# differs (verify with `dx bundle --platform web` locally).
RUN dx bundle --release --platform web --package darkrun-site

# Build the server binary (rustls, no native-tls; C-free).
RUN cargo build --release --bin darkrun-web

# Normalize the SPA output into /app/site regardless of the exact dx path.
RUN set -eux; \
    src="$(find target/dx -type d -name public | head -1)"; \
    if [ -z "$src" ] && [ -d web/site/dist ]; then src=web/site/dist; fi; \
    mkdir -p /app/site && cp -r "$src/." /app/site/

# Generate the SEO artifacts (robots.txt, sitemap.xml, feed.xml, atom.xml,
# feed.json, routes.txt) INTO the served bundle. `dx bundle` doesn't emit these
# (they come from the darkrun-site-gen binary), so without this step /robots.txt
# and /sitemap.xml fell through to the SPA fallback and served index.html — a
# crawler asking for robots.txt got HTML, which breaks SEO/crawler readouts.
# Same failure mode the /assets copy below fixes, for the SEO files.
RUN cargo run --release --package darkrun-site --bin darkrun-site-gen -- /app/site

# Ensure the literal /assets/* paths resolve. index.html references the social
# card, favicon, and desktop screenshots by fixed path (/assets/og.png, …), but
# `dx bundle` doesn't copy web/site/assets into the public bundle — so those URLs
# 404'd and the SPA fallback served index.html (OG image, favicon, and the site
# screenshots all came back as HTML). Copy the dir in so they serve as files.
# Contents-copy (.../.) merges with any dx-managed assets dir without nesting.
RUN if [ -d web/site/assets ]; then \
        mkdir -p /app/site/assets && cp -r web/site/assets/. /app/site/assets/; \
    fi

# ── Runtime ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /srv
COPY --from=builder /app/target/release/darkrun-web /usr/local/bin/darkrun-web
COPY --from=builder /app/site /srv/site

ENV DARKRUN_SITE_DIR=/srv/site \
    DARKRUN_WEB_ADDR=0.0.0.0:8080 \
    DARKRUN_ENV=production
EXPOSE 8080

# Non-root.
RUN useradd --system --uid 10001 darkrun
USER darkrun

ENTRYPOINT ["/usr/local/bin/darkrun-web"]
