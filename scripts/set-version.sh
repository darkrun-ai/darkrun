#!/usr/bin/env bash
# Stamp ONE version across every place darkrun tracks it, so the Cargo workspace
# and the npm packages never drift:
#   - Cargo.toml [workspace.package].version  (all crates inherit via version.workspace)
#   - plugin/package.json .version + its @darkrun/* optionalDependencies pins
#   - npm/<arch>/package.json .version  (the 5 per-arch binary packages)
#
# Idempotent. Used by the release workflow (stamping from the release tag) and
# available for manual bumps.
#   ./scripts/set-version.sh 0.2.0
set -euo pipefail

VERSION="${1:?usage: set-version.sh X.Y.Z}"
VERSION="${VERSION#v}" # tolerate a leading v from a tag name

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

ARCH_PKGS=(darwin-arm64 darwin-x64 linux-x64 linux-arm64 win32-x64)

# 1. Cargo workspace version — the first `version = "..."` after [workspace.package].
perl -0pi -e 's/(\[workspace\.package\]\nversion\s*=\s*")[^"]*(")/${1}'"$VERSION"'${2}/' Cargo.toml

# 2. Main package + its optionalDependencies pins (kept equal to the version).
jq --arg v "$VERSION" '
  .version = $v
  | .optionalDependencies |= with_entries(.value = $v)
' plugin/package.json > plugin/package.json.tmp && mv plugin/package.json.tmp plugin/package.json

# 3. Per-arch binary packages.
for a in "${ARCH_PKGS[@]}"; do
  jq --arg v "$VERSION" '.version = $v' "npm/$a/package.json" > "npm/$a/package.json.tmp" \
    && mv "npm/$a/package.json.tmp" "npm/$a/package.json"
done

echo "Stamped $VERSION: Cargo.toml + plugin + ${#ARCH_PKGS[@]} arch packages."
