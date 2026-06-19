# Universal Links / App Links — server side

These two files complete the deep-link handshake configured on the app side in
[`desktop/Dioxus.toml`](../../desktop/Dioxus.toml) (`[deep_links] hosts =
["app.darkrun.ai"]`). The **app** side asserts it can handle `app.darkrun.ai`
links; these **server** files are how `app.darkrun.ai` asserts the reverse, so
Apple and Google trust the association.

## Deployment

`app.darkrun.ai` (the web build of the darkrun app) MUST serve both files from
its `/.well-known/` path:

| File | Served at | Content-Type | Notes |
|---|---|---|---|
| `apple-app-site-association` | `https://app.darkrun.ai/.well-known/apple-app-site-association` | `application/json` | **No** `.json` extension. **No** redirects. Must be HTTPS. |
| `assetlinks.json` | `https://app.darkrun.ai/.well-known/assetlinks.json` | `application/json` | |

Apple fetches the AASA via its CDN on app install; Android verifies
`assetlinks.json` at install time.

## Required values (fill before shipping)

1. ✅ **`TEAMID`** in `apple-app-site-association` → filled in: the appID is
   `8H7HVPHS87.ai.darkrun.app`.
2. **`REPLACE_WITH_APP_SIGNING_CERT_SHA256_FINGERPRINT`** in `assetlinks.json` →
   the SHA-256 fingerprint of the Android app-signing certificate (only needed
   when an Android app ships; `keytool -list -v -keystore <keystore>` → SHA256,
   or Play Console → App integrity → App signing key certificate).

## Client side (the app capability) — required for the link to open the app

The server files above are necessary but NOT sufficient. The app must also ship
the **Associated Domains** entitlement
(`com.apple.developer.associated-domains` = `applinks:app.darkrun.ai`,
`webcredentials:app.darkrun.ai`), which Apple only honors if:

1. The **Associated Domains capability is enabled on the `ai.darkrun.app` App
   ID** (Apple Developer portal → Identifiers → ai.darkrun.app → Capabilities),
   then the provisioning profile is regenerated (`fastlane match appstore
   --force` / `match macos`).
2. The signed app's entitlements list those domains. **macOS:** wired in
   `fastlane/darkrun-mac.entitlements`. **iOS:** NOT yet added to
   `fastlane/package-ios-ipa.sh` — adding it before step 1 would fail App Store
   upload ("entitlement not in profile"), so enable the capability + regen the
   profile FIRST, then add the `applinks:`/`webcredentials:` domains to the
   derived entitlements there.

Without step 1, a tapped `https://app.darkrun.ai/...` link just opens the web app
(the intended fallback) — it never hands off to the installed app.
