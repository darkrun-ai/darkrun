#!/usr/bin/env bash
# package-macos-pkg.sh — sign the dx-built macOS .app for the Mac App Store and
# wrap it in an installer .pkg ready to upload to App Store Connect.
#
# `dx bundle --platform macos --release` emits a `.app` (no Xcode project), so
# this is the macOS sibling of package-ios-ipa.sh: patch Info.plist for App Store
# validity, embed the Mac App Store provisioning profile, codesign the app with
# the App Sandbox entitlements (fastlane/darkrun-mac.entitlements), then
# productbuild a signed installer .pkg.
#
# Signing identities + profile come from the environment (CI provides them via a
# `match macos` bootstrap — Phase 3); the script auto-detects what `match`
# installs and fails clearly if absent. Exports DARKRUN_PKG to $GITHUB_ENV.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# The dx output: target/dx/<package>/<profile>/macos/<App>.app
APP="$(find target/dx -path '*/macos/*.app' -type d -maxdepth 6 2>/dev/null | head -1)"
[ -n "$APP" ] || { echo "error: no .app found under target/dx (did 'dx bundle --platform macos' run?)" >&2; exit 1; }
echo "app:      $APP"

ENTITLEMENTS="$ROOT/fastlane/darkrun-mac.entitlements"
[ -f "$ENTITLEMENTS" ] || { echo "error: $ENTITLEMENTS missing" >&2; exit 1; }

# The Mac App Store provisioning profile match installed (most-recent
# .provisionprofile). DARKRUN_MAC_PROVISION_PROFILE overrides the search.
PROFILE="${DARKRUN_MAC_PROVISION_PROFILE:-}"
if [ -z "$PROFILE" ]; then
  PROFILE="$(find \
    "$HOME/Library/MobileDevice/Provisioning Profiles" \
    "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles" \
    -name '*.provisionprofile' -type f -print0 2>/dev/null \
    | xargs -0 ls -t 2>/dev/null | head -1 || true)"
fi
[ -n "${PROFILE:-}" ] || { echo "error: no Mac App Store .provisionprofile found (run the macOS signing bootstrap first)" >&2; exit 1; }
echo "profile:  $PROFILE"

# Mac App Store needs TWO identities: the app is signed with the application
# distribution cert, the .pkg with the installer cert. Newer certs are named
# "Apple Distribution" / older "3rd Party Mac Developer Application"; the
# installer is "3rd Party Mac Developer Installer" (a.k.a. "Apple Installer").
# `|| true` so a no-match grep doesn't trip set -o pipefail and abort BEFORE the
# `[ -n ]` checks below can report which identity is missing. Dump the available
# identities first so a failure shows exactly what's in the keychain.
ALL_CODESIGN="$(security find-identity -v -p codesigning 2>/dev/null || true)"
# The installer cert ("3rd Party Mac Developer Installer") is NOT a code-signing
# identity, so `find-identity -v` (no policy / codesigning default) does NOT list
# it. Enumerate under the `basic` X.509 policy, which DOES include installer
# certs, so the INSTALLER_IDENTITY lookup below can find it.
ALL_IDENTITIES="$(security find-identity -v -p basic 2>/dev/null || true)"
echo "codesigning identities:"; printf '%s\n' "$ALL_CODESIGN" | sed 's/^/  /'
echo "all identities (basic policy):"; printf '%s\n' "$ALL_IDENTITIES" | sed 's/^/  /'
APP_IDENTITY="${DARKRUN_MAC_APP_IDENTITY:-$(printf '%s\n' "$ALL_CODESIGN" | grep -oE '3rd Party Mac Developer Application[^"]*|Apple Distribution[^"]*' | head -1 || true)}"
INSTALLER_IDENTITY="${DARKRUN_MAC_INSTALLER_IDENTITY:-$(printf '%s\n' "$ALL_IDENTITIES" | grep -oE '3rd Party Mac Developer Installer[^"]*|Mac Installer Distribution[^"]*' | head -1 || true)}"
[ -n "${APP_IDENTITY:-}" ] || { echo "error: no Mac App distribution identity in the keychain (see list above)" >&2; exit 1; }
[ -n "${INSTALLER_IDENTITY:-}" ] || { echo "error: no Mac Installer distribution identity in the keychain (see list above) — the Mac Installer Distribution cert may not exist; the macOS signing bootstrap must create it" >&2; exit 1; }
echo "app id:   $APP_IDENTITY"
echo "pkg id:   $INSTALLER_IDENTITY"

# Patch Info.plist BEFORE signing (editing after breaks the signature).
PLIST="$APP/Contents/Info.plist"
plist_set() { /usr/libexec/PlistBuddy -c "Set :$1 $2" "$PLIST" 2>/dev/null || /usr/libexec/PlistBuddy -c "Add :$1 $3 $2" "$PLIST"; }
plist_set CFBundleDisplayName "Darkrun AI" string
plist_set CFBundleName "Darkrun AI" string
# Required by the Mac App Store; darkrun is a developer tool.
plist_set LSApplicationCategoryType "public.app-category.developer-tools" string
# Export compliance: standard HTTPS/TLS only (see package-ios-ipa.sh) — exempt.
plist_set ITSAppUsesNonExemptEncryption false bool
# Marketing version + build number (CI passes the RC values; default to whatever
# dx stamped so a local run still produces a valid bundle).
[ -n "${DARKRUN_MARKETING_VERSION:-}" ] && plist_set CFBundleShortVersionString "$DARKRUN_MARKETING_VERSION" string
[ -n "${DARKRUN_BUILD_NUMBER:-}" ] && plist_set CFBundleVersion "$DARKRUN_BUILD_NUMBER" string
echo "version:  CFBundleShortVersionString=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$PLIST") CFBundleVersion=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$PLIST")"

# Embed the Mac App Store provisioning profile.
cp "$PROFILE" "$APP/Contents/embedded.provisionprofile"

# Sign nested code first (frameworks/dylibs/helpers), then the app with the
# sandbox entitlements. --options runtime is harmless for MAS and future-proofs.
find "$APP/Contents" \( -name '*.framework' -o -name '*.dylib' \) -print0 2>/dev/null |
  while IFS= read -r -d '' nested; do
    echo "  sign nested: $nested"
    codesign --force --timestamp -o runtime -s "$APP_IDENTITY" "$nested"
  done
codesign --force --timestamp -o runtime \
  --entitlements "$ENTITLEMENTS" -s "$APP_IDENTITY" "$APP"
codesign --verify --deep --strict "$APP" && echo "codesign verified"

# Build the signed installer .pkg (installs into /Applications).
rm -f "$ROOT/darkrun.pkg"
productbuild --component "$APP" /Applications --sign "$INSTALLER_IDENTITY" "$ROOT/darkrun.pkg"

echo "pkg:      $ROOT/darkrun.pkg"
echo "DARKRUN_PKG=$ROOT/darkrun.pkg" >> "${GITHUB_ENV:-/dev/stdout}"
