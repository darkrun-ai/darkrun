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

These are the only two unknowns — they come from your signing accounts, not the
codebase:

1. **`TEAMID`** in `apple-app-site-association` → your 10-character Apple
   Developer Team ID (Apple Developer portal → Membership). The full appID is
   `TEAMID.ai.darkrun.app`.
2. **`REPLACE_WITH_APP_SIGNING_CERT_SHA256_FINGERPRINT`** in `assetlinks.json` →
   the SHA-256 fingerprint of the Android app-signing certificate
   (`keytool -list -v -keystore <keystore>` → SHA256, or the Play Console →
   App integrity → App signing key certificate).
