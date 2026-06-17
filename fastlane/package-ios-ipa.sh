#!/usr/bin/env bash
# package-ios-ipa.sh — sign the dx-built iOS .app into an App Store .ipa.
#
# `dx build --platform ios --release` emits a built `.app` (not an Xcode
# project), so there's no `gym` step. This embeds the match provisioning profile,
# re-signs the app with the App Store distribution identity (both installed by
# `fastlane ios install_signing` beforehand), and zips it into a Payload/ .ipa.
#
# Exports DARKRUN_IPA (the .ipa path) to $GITHUB_ENV for the upload lane.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# The dx output: target/dx/<package>/<profile>/ios/<App>.app
APP="$(find target/dx -path '*/ios/*.app' -type d -maxdepth 6 2>/dev/null | head -1)"
[ -n "$APP" ] || { echo "error: no .app found under target/dx (did 'dx build --platform ios' run?)" >&2; exit 1; }
echo "app:      $APP"

# The provisioning profile match installed (most-recent .mobileprovision). Search
# both install locations: the legacy ~/Library/MobileDevice path (older Xcode) and
# ~/Library/Developer/Xcode/UserData (Xcode 16+/26, where match now installs it).
# `|| true` so a missing dir doesn't trip set -e/pipefail before the check below.
PROFILE="$(find \
  "$HOME/Library/MobileDevice/Provisioning Profiles" \
  "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles" \
  -name '*.mobileprovision' -type f -print0 2>/dev/null \
  | xargs -0 ls -t 2>/dev/null | head -1 || true)"
[ -n "${PROFILE:-}" ] || { echo "error: no provisioning profile installed (run 'fastlane ios install_signing' first)" >&2; exit 1; }
echo "profile:  $PROFILE"

# The App Store distribution identity match installed in the keychain.
IDENTITY="$(security find-identity -v -p codesigning | grep -oE 'Apple Distribution[^"]*' | head -1)"
[ -n "${IDENTITY:-}" ] || { echo "error: no 'Apple Distribution' identity in the keychain" >&2; exit 1; }
echo "identity: $IDENTITY"

# Patch the Info.plist for App Store validity BEFORE signing (editing after would
# break the signature). dx's iOS bundle omits/empties these, which altool rejects:
#   - CFBundlePackageType must be APPL
#   - MinimumOSVersion must be a real deployment target (>= 12.0 for arm64)
#   - LSRequiresIPhoneOS marks it an iOS app
PLIST="$APP/Info.plist"
plist_set() { /usr/libexec/PlistBuddy -c "Set :$1 $2" "$PLIST" 2>/dev/null || /usr/libexec/PlistBuddy -c "Add :$1 $3 $2" "$PLIST"; }
plist_set CFBundlePackageType APPL string
plist_set MinimumOSVersion 15.0 string
plist_set LSRequiresIPhoneOS true bool
echo "info.plist: CFBundlePackageType=$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$PLIST") MinimumOSVersion=$(/usr/libexec/PlistBuddy -c 'Print :MinimumOSVersion' "$PLIST")"

# Embed the profile and derive entitlements from it (so the signed app's
# entitlements match what the profile authorizes — app id, team, etc.).
cp "$PROFILE" "$APP/embedded.mobileprovision"
security cms -D -i "$PROFILE" > /tmp/dr-profile.plist
/usr/libexec/PlistBuddy -x -c 'Print :Entitlements' /tmp/dr-profile.plist > /tmp/dr-entitlements.plist

# Sign nested code first (frameworks/dylibs/extensions, if any), then the app.
find "$APP" \( -name '*.framework' -o -name '*.dylib' -o -name '*.appex' \) -print0 |
  while IFS= read -r -d '' nested; do
    echo "  sign nested: $nested"
    codesign --force --timestamp=none -s "$IDENTITY" "$nested"
  done
codesign --force --timestamp=none -s "$IDENTITY" --entitlements /tmp/dr-entitlements.plist "$APP"
codesign --verify --deep --strict "$APP" && echo "codesign verified"

# Package into Payload/<App>.app -> .ipa
rm -rf /tmp/dr-payload "$ROOT/darkrun.ipa"
mkdir -p /tmp/dr-payload/Payload
cp -R "$APP" /tmp/dr-payload/Payload/
# --symlinks preserves the bundle's symlinks; without it the _CodeSignature/
# CodeResources symlink is dereferenced and altool rejects the bundle.
( cd /tmp/dr-payload && zip -qr --symlinks "$ROOT/darkrun.ipa" Payload )

echo "ipa:      $ROOT/darkrun.ipa"
echo "DARKRUN_IPA=$ROOT/darkrun.ipa" >> "${GITHUB_ENV:-/dev/stdout}"
