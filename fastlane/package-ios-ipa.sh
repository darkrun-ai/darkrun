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
# Home-screen / App Store display name. dx names it from the package
# (DarkrunDesktop); the product is "Darkrun AI".
plist_set CFBundleDisplayName "Darkrun AI" string
plist_set CFBundleName "Darkrun AI" string
# Export-compliance self-classification. Without this key App Store Connect
# prompts for the encryption declaration on EVERY build. darkrun's only crypto is
# standard HTTPS/TLS (rustls/aws-lc for GitHub/GitLab/relay/git-over-HTTPS), which
# is exempt under the secure-channel exemption — so declare it false once and the
# prompt stops for good.
plist_set ITSAppUsesNonExemptEncryption false bool

# Marketing version + build number. CI passes these (DARKRUN_MARKETING_VERSION =
# the in-dev/tag version, DARKRUN_BUILD_NUMBER = the monotonic commit count) so
# every TestFlight build is a uniquely-numbered RC; both default to the dx-stamped
# plist values when unset, so a local package run still produces a valid bundle.
[ -n "${DARKRUN_MARKETING_VERSION:-}" ] && plist_set CFBundleShortVersionString "$DARKRUN_MARKETING_VERSION" string
[ -n "${DARKRUN_BUILD_NUMBER:-}" ] && plist_set CFBundleVersion "$DARKRUN_BUILD_NUMBER" string
echo "version:  CFBundleShortVersionString=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$PLIST") CFBundleVersion=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$PLIST")"

# Xcode normally injects the "DT*" build-metadata keys; dx does not, and altool
# rejects the binary for "Missing Info.plist value 'DTPlatformName'". Derive them
# from the active toolchain so the bundle looks like a real Xcode build.
SDK_VER="$(xcrun --sdk iphoneos --show-sdk-version 2>/dev/null || echo 26.0)"
SDK_BUILD="$(xcrun --sdk iphoneos --show-sdk-build-version 2>/dev/null || true)"
XCODE_VER="$(xcodebuild -version 2>/dev/null | awk '/^Xcode/{print $2}')"
XCODE_BUILD="$(xcodebuild -version 2>/dev/null | awk '/^Build version/{print $3}')"
OS_BUILD="$(sw_vers -buildVersion 2>/dev/null || true)"
plist_set DTPlatformName iphoneos string
plist_set DTPlatformVersion "$SDK_VER" string
plist_set DTSDKName "iphoneos${SDK_VER}" string
[ -n "$SDK_BUILD" ] && { plist_set DTSDKBuild "$SDK_BUILD" string; plist_set DTPlatformBuild "$SDK_BUILD" string; }
plist_set DTCompiler com.apple.compilers.llvm.clang.1_0 string
[ -n "$XCODE_VER" ]   && plist_set DTXcode "$(echo "$XCODE_VER" | awk -F. '{printf "%02d%d%d", $1, $2, $3}')" string
[ -n "$XCODE_BUILD" ] && plist_set DTXcodeBuild "$XCODE_BUILD" string
[ -n "$OS_BUILD" ]    && plist_set BuildMachineOSBuild "$OS_BUILD" string
echo "info.plist: CFBundlePackageType=$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$PLIST") MinimumOSVersion=$(/usr/libexec/PlistBuddy -c 'Print :MinimumOSVersion' "$PLIST") DTPlatformName=$(/usr/libexec/PlistBuddy -c 'Print :DTPlatformName' "$PLIST") DTSDKName=$(/usr/libexec/PlistBuddy -c 'Print :DTSDKName' "$PLIST")"

# Universal app: iPhone + iPad (UIDeviceFamily [1,2]). Supporting iPad also makes
# the app available on Apple-Silicon Macs ("Designed for iPad"), so one binary
# covers all three. CFBundleSupportedPlatforms stays a SINGLE value (iPhoneOS —
# the platform value covers iPad too; altool rejects the multi-value list dx
# emits). The iPad icon + UILaunchScreen below satisfy the iPad/multitasking
# requirements this unlocks.
/usr/libexec/PlistBuddy -c "Delete :CFBundleSupportedPlatforms" "$PLIST" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string iPhoneOS" "$PLIST"
/usr/libexec/PlistBuddy -c "Delete :UIDeviceFamily" "$PLIST" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :UIDeviceFamily array" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :UIDeviceFamily:0 integer 1" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :UIDeviceFamily:1 integer 2" "$PLIST"
# Resizable on iPad/Mac: don't require full screen, and support all orientations
# so Split View / Stage Manager / a Mac window can size freely.
/usr/libexec/PlistBuddy -c "Delete :UIRequiresFullScreen" "$PLIST" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :UIRequiresFullScreen bool false" "$PLIST"
set_orientations() { # rebuild $1 as the full 4-orientation array
  /usr/libexec/PlistBuddy -c "Delete :$1" "$PLIST" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :$1 array" "$PLIST"
  local i=0
  for orient in UIInterfaceOrientationPortrait UIInterfaceOrientationPortraitUpsideDown \
                UIInterfaceOrientationLandscapeLeft UIInterfaceOrientationLandscapeRight; do
    /usr/libexec/PlistBuddy -c "Add :$1:$i string $orient" "$PLIST"
    i=$((i+1))
  done
}
set_orientations UISupportedInterfaceOrientations
set_orientations "UISupportedInterfaceOrientations~ipad"
# Modern launch screen (no storyboard needed at MinimumOSVersion >= 14). REQUIRED
# for iPad multitasking / resizable windows.
/usr/libexec/PlistBuddy -c "Add :UILaunchScreen dict" "$PLIST" 2>/dev/null || true

# Build + compile the app-icon asset catalog. dx doesn't produce one, so altool
# rejects the bundle for a missing CFBundleIconName + missing 120/152px icons.
# actool compiles Assets.car into the app and emits the CFBundleIcons keys.
ICON_SRC="$ROOT/desktop/assets/icon.png"
if [ -f "$ICON_SRC" ]; then
  ICONSET="/tmp/dr-assets.xcassets/AppIcon.appiconset"
  rm -rf /tmp/dr-assets.xcassets; mkdir -p "$ICONSET"
  for spec in 40:icon-20@2x 60:icon-20@3x 58:icon-29@2x 87:icon-29@3x \
              80:icon-40@2x 120:icon-40@3x 120:icon-60@2x 180:icon-60@3x 1024:icon-1024 \
              20:icon-ipad-20@1x 40:icon-ipad-20@2x 29:icon-ipad-29@1x 58:icon-ipad-29@2x \
              40:icon-ipad-40@1x 80:icon-ipad-40@2x 76:icon-ipad-76@1x 152:icon-ipad-76@2x \
              167:icon-ipad-83.5@2x; do
    px="${spec%%:*}"; name="${spec##*:}"
    sips -z "$px" "$px" "$ICON_SRC" --out "$ICONSET/${name}.png" >/dev/null
  done
  cat > "$ICONSET/Contents.json" <<'JSON'
{
  "images": [
    {"idiom":"iphone","size":"20x20","scale":"2x","filename":"icon-20@2x.png"},
    {"idiom":"iphone","size":"20x20","scale":"3x","filename":"icon-20@3x.png"},
    {"idiom":"iphone","size":"29x29","scale":"2x","filename":"icon-29@2x.png"},
    {"idiom":"iphone","size":"29x29","scale":"3x","filename":"icon-29@3x.png"},
    {"idiom":"iphone","size":"40x40","scale":"2x","filename":"icon-40@2x.png"},
    {"idiom":"iphone","size":"40x40","scale":"3x","filename":"icon-40@3x.png"},
    {"idiom":"iphone","size":"60x60","scale":"2x","filename":"icon-60@2x.png"},
    {"idiom":"iphone","size":"60x60","scale":"3x","filename":"icon-60@3x.png"},
    {"idiom":"ipad","size":"20x20","scale":"1x","filename":"icon-ipad-20@1x.png"},
    {"idiom":"ipad","size":"20x20","scale":"2x","filename":"icon-ipad-20@2x.png"},
    {"idiom":"ipad","size":"29x29","scale":"1x","filename":"icon-ipad-29@1x.png"},
    {"idiom":"ipad","size":"29x29","scale":"2x","filename":"icon-ipad-29@2x.png"},
    {"idiom":"ipad","size":"40x40","scale":"1x","filename":"icon-ipad-40@1x.png"},
    {"idiom":"ipad","size":"40x40","scale":"2x","filename":"icon-ipad-40@2x.png"},
    {"idiom":"ipad","size":"76x76","scale":"1x","filename":"icon-ipad-76@1x.png"},
    {"idiom":"ipad","size":"76x76","scale":"2x","filename":"icon-ipad-76@2x.png"},
    {"idiom":"ipad","size":"83.5x83.5","scale":"2x","filename":"icon-ipad-83.5@2x.png"},
    {"idiom":"ios-marketing","size":"1024x1024","scale":"1x","filename":"icon-1024.png"}
  ],
  "info": {"version":1,"author":"xcode"}
}
JSON
  xcrun actool /tmp/dr-assets.xcassets \
    --compile "$APP" \
    --app-icon AppIcon \
    --platform iphoneos \
    --target-device iphone \
    --target-device ipad \
    --minimum-deployment-target 15.0 \
    --output-partial-info-plist /tmp/dr-actool.plist \
    --output-format human-readable-text >/dev/null
  plist_set CFBundleIconName AppIcon string
  /usr/libexec/PlistBuddy -c "Merge /tmp/dr-actool.plist" "$PLIST" 2>/dev/null || true
  echo "icons:    Assets.car compiled, CFBundleIconName=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconName' "$PLIST")"
else
  echo "warning: $ICON_SRC not found — skipping icon catalog (upload will fail icon validation)" >&2
fi

# Embed the profile and derive entitlements from it (so the signed app's
# entitlements match what the profile authorizes — app id, team, etc.).
cp "$PROFILE" "$APP/embedded.mobileprovision"
security cms -D -i "$PROFILE" > /tmp/dr-profile.plist
/usr/libexec/PlistBuddy -x -c 'Print :Entitlements' /tmp/dr-profile.plist > /tmp/dr-entitlements.plist

# Universal Links: declare the Associated Domains so a tapped app.darkrun.ai link
# opens the app (web app is the fallback). The Associated Domains capability is
# enabled on the ai.darkrun.app App ID; the profile must be REGENERATED to carry
# it (match profiles are immutable snapshots) — if upload fails with "entitlement
# not in profile", run `bundle exec fastlane match appstore --force`.
ENT="/tmp/dr-entitlements.plist"
/usr/libexec/PlistBuddy -c "Delete :com.apple.developer.associated-domains" "$ENT" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :com.apple.developer.associated-domains array" "$ENT"
/usr/libexec/PlistBuddy -c "Add :com.apple.developer.associated-domains:0 string applinks:app.darkrun.ai" "$ENT"
/usr/libexec/PlistBuddy -c "Add :com.apple.developer.associated-domains:1 string webcredentials:app.darkrun.ai" "$ENT"
echo "entitlements: associated-domains=$(/usr/libexec/PlistBuddy -c 'Print :com.apple.developer.associated-domains:0' "$ENT")"

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
