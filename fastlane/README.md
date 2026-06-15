# App Store screenshots — darkrun

The pipeline that turns raw app screens into framed, captioned App Store
screenshots and uploads them to App Store Connect (app `ai.darkrun.app`, already
created there). Built on [fastlane](https://fastlane.tools) `frameit` + `deliver`.

darkrun's app is **Dioxus** (`dx`), not an Xcode UI-test target, so we don't use
`capture_screenshots` (that drives XCUITest). You capture raw screens yourself —
the simplest source is the **web build at app.darkrun.ai** rendered at each
device size (headless browser), since it shares the UI with the native app — then
`frameit` adds the bezel + the dark-brand caption.

## One-time setup

```bash
cd fastlane
bundle install                 # installs fastlane (see Gemfile)
bundle exec fastlane frameit download_frames   # device-frame images (~large, cached in ~/.frameit)
```

Fill the account-specific values in `Appfile` (or export the env vars):
`DARKRUN_APPLE_ID`, `DARKRUN_APPLE_TEAM_ID`, `DARKRUN_ASC_TEAM_ID`.

## Workflow

1. **Capture raw screens** into `fastlane/screenshots/<locale>/`, one PNG per
   (device, screen), at the device's exact pixel size (table below). Name each
   `<device>-<screen>.png` where `<screen>` contains one of the Framefile filters
   (`review`, `dag`, `statusline`, `factory`, `checkpoint`) — frameit matches the
   caption + frame by that substring. Example:
   `iPhone 16 Pro Max-review.png`, `iPad Pro 13-dag.png`.
2. **Frame + caption**: `bundle exec fastlane screenshots`
   → writes `<name>_framed.png` beside each raw file, on the dark-brand
   background with the keyword + title from `*.strings`.
3. **Review** the framed PNGs.
4. **Upload**: `bundle exec fastlane upload_screenshots`
   → `deliver` pushes them to App Store Connect (screenshots only — no binary, no
   metadata, no version bump, no submission).

## Required device sizes (App Store Connect, portrait)

Apple requires the 6.9" iPhone and, for a universal app, the 13" iPad. The
others are accepted and scale down from these. Capture at these exact pixels:

| Device class | Example device | Portrait px |
|---|---|---|
| iPhone 6.9" (required) | iPhone 16 Pro Max / 15 Pro Max | 1320 × 2868 |
| iPhone 6.5" | iPhone 14 Plus / 11 Pro Max | 1284 × 2778 |
| iPad 13" (required if iPad) | iPad Pro 13" (M4) | 2064 × 2752 |
| iPad 12.9" | iPad Pro 12.9" | 2048 × 2732 |
| Mac | — | 2880 × 1800 |

(Single universal bundle `ai.darkrun.app` across iPhone / iPad / Mac — see the
[accounts & distribution direction]; one screenshot set per device class.)

## What's tracked vs generated

Tracked (the config): `Framefile.json`, `screenshots/<locale>/title.strings`,
`screenshots/<locale>/keyword.strings`, `background.png` (dark brand gradient
`#0e1217`→`#07090c`), `Appfile`, `Fastfile`, `Gemfile`.

Generated (git-ignored): the raw `*.png` captures and the `*_framed.png` output.

## Captions

Edit `screenshots/<locale>/keyword.strings` (the small accent line) and
`title.strings` (the headline). Add a locale by creating a sibling directory
(e.g. `screenshots/de-DE/`) with its own `*.strings` + raw captures.
