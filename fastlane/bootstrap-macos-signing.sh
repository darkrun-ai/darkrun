#!/usr/bin/env bash
# bootstrap-macos-signing.sh — one-shot Mac App Store (MAS) signing setup for the
# darkrun macOS-appstore workflow (macos-appstore.yml).
#
# It:
#   1. ensures the private `match` certs repo exists (the SAME repo iOS uses),
#   2. reuses the SAME match passphrase as iOS (MATCH_PASSWORD — required; this
#      script does NOT mint a new one, since the repo is already encrypted),
#   3. runs `fastlane match` for the THREE MAS signing types — a Mac App
#      Distribution cert, a Mac Installer Distribution cert, and a Mac App Store
#      provisioning profile for ai.darkrun.app (with the App Group
#      group.ai.darkrun + Associated Domains capabilities) — and pushes them
#      (encrypted) to that repo,
#   4. sets/refreshes the same GitHub Actions secrets the workflow reads.
#
# Run this AFTER bootstrap-ios-signing.sh — it reuses the iOS match repo, ASC API
# key, and MATCH_PASSWORD rather than creating new ones. You run it on your Mac.
# The App Store Connect API key (.p8) and your GitHub token flow straight from
# disk / `gh` into Secret Manager — they are never printed. Re-runnable; match
# reuses existing certs/profiles rather than minting new ones.
#
# Usage:
#   MATCH_PASSWORD=<iOS passphrase> ./fastlane/bootstrap-macos-signing.sh [path-to-AuthKey.p8]
#
# Overridable via env (defaults shown):
#   ASC_KEY_ID=VWYTA9334U
#   ASC_ISSUER_ID=69a6de78-2049-47e3-e053-5b8c7c11a4d1
#   CERTS_REPO=darkrun-ai/certs        # the private match repo (shared with iOS)
#   APP_REPO=darkrun-ai/darkrun        # where the Actions secrets are set
#   MATCH_PASSWORD=<required>          # the SAME passphrase iOS bootstrap printed
#   MATCH_CI_TOKEN=<gh auth token>     # token CI uses to READ the certs repo
set -euo pipefail

ASC_KEY_ID="${ASC_KEY_ID:-VWYTA9334U}"
ASC_ISSUER_ID="${ASC_ISSUER_ID:-69a6de78-2049-47e3-e053-5b8c7c11a4d1}"
ASC_KEY_P8_FILE="${1:-$HOME/Downloads/AuthKey_${ASC_KEY_ID}.p8}"
CERTS_REPO="${CERTS_REPO:-darkrun-ai/certs}"
APP_REPO="${APP_REPO:-darkrun-ai/darkrun}"
MATCH_GIT_URL="https://github.com/${CERTS_REPO}.git"

# Resolve to the repo root regardless of where this is invoked from.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Don't let fastlane regenerate fastlane/README.md (it would clobber the
# hand-written docs with an auto lanes table).
export FASTLANE_SKIP_DOCS=1

die() { echo "error: $*" >&2; exit 1; }

# ── Preconditions ────────────────────────────────────────────────────────────
command -v gh >/dev/null 2>&1 || die "gh (GitHub CLI) not found"
command -v bundle >/dev/null 2>&1 || die "bundler not found (gem install bundler)"
command -v openssl >/dev/null 2>&1 || die "openssl not found"
gh auth status >/dev/null 2>&1 || die "not logged in — run 'gh auth login'"
[ -f "$ASC_KEY_P8_FILE" ] || die "no App Store Connect key at: $ASC_KEY_P8_FILE
  pass the path as the first argument, or set ASC_KEY_ID so the default resolves."
# The MAS certs go in the SAME (already-encrypted) repo as iOS, so the passphrase
# must match the one bootstrap-ios-signing.sh printed — we never generate a new
# one here (that would mean the repo couldn't be decrypted).
[ -n "${MATCH_PASSWORD:-}" ] || die "MATCH_PASSWORD is required — pass the SAME passphrase
  bootstrap-ios-signing.sh printed (the certs repo is already encrypted with it):
      MATCH_PASSWORD=<iOS passphrase> ./fastlane/bootstrap-macos-signing.sh"

echo "▶ key id     : $ASC_KEY_ID"
echo "▶ issuer id  : $ASC_ISSUER_ID"
echo "▶ p8 file    : $ASC_KEY_P8_FILE"
echo "▶ certs repo : $CERTS_REPO"
echo "▶ app repo   : $APP_REPO"

# ── A modern Ruby ────────────────────────────────────────────────────────────
# macOS system Ruby (2.6) is EOL, links LibreSSL, and breaks match's cert
# encryption ("Error encrypting ...p12"); fastlane 2.235+ also requires Ruby 3.0+.
# Prefer a Homebrew Ruby if one is installed; bail with guidance otherwise.
ruby_old() { ruby -e 'exit(Gem::Version.new(RUBY_VERSION) < Gem::Version.new("3.0"))' 2>/dev/null; }
if ruby_old; then
  if command -v brew >/dev/null 2>&1 && [ -x "$(brew --prefix ruby 2>/dev/null)/bin/ruby" ]; then
    export PATH="$(brew --prefix ruby)/bin:$PATH"
    gem list -i bundler >/dev/null 2>&1 || gem install bundler --no-document >/dev/null 2>&1 || true
  fi
fi
if ruby_old; then
  die "Ruby $(ruby -e 'print RUBY_VERSION') is too old — match's encryption needs 3.0+
  (macOS system Ruby links LibreSSL and fails). Install a current Ruby and re-run:
      brew install ruby"
fi
echo "▶ ruby       : $(ruby -e 'print RUBY_VERSION')"

# ── Real OpenSSL on PATH (belt + suspenders) ─────────────────────────────────
# match also shells out to `openssl enc`; keep a real OpenSSL ahead of Apple's
# LibreSSL on PATH.
if openssl version 2>/dev/null | grep -qi libressl; then
  if command -v brew >/dev/null 2>&1; then
    for f in openssl@3 openssl@1.1; do
      if brew --prefix "$f" >/dev/null 2>&1; then
        export PATH="$(brew --prefix "$f")/bin:$PATH"
        break
      fi
    done
  fi
  if openssl version 2>/dev/null | grep -qi libressl; then
    die "openssl is LibreSSL ($(openssl version)); match's encryption needs real OpenSSL.
  Install it and re-run:  brew install openssl@3"
  fi
fi
echo "▶ openssl    : $(openssl version)"

# ── fastlane deps ────────────────────────────────────────────────────────────
# Drop any stale Gemfile.lock — one written by an older Ruby/bundler (e.g. macOS
# system Ruby 2.6 pins bundler 1.17.2, which crashes on Ruby 3.2+ with
# "undefined method 'untaint'"). It's gitignored, so it regenerates cleanly under
# the current bundler. Also make sure a modern bundler is present.
rm -f "$ROOT/fastlane/Gemfile.lock"
gem install bundler --no-document --conservative >/dev/null 2>&1 || true
echo "▶ installing fastlane (bundler)..."
( cd "$ROOT/fastlane" && bundle install --quiet )

# ── 1. ensure the private certs repo exists (shared with iOS) ────────────────
if gh repo view "$CERTS_REPO" >/dev/null 2>&1; then
  echo "▶ certs repo exists"
else
  echo "▶ creating private certs repo $CERTS_REPO"
  gh repo create "$CERTS_REPO" --private --description "darkrun fastlane match signing certs"
fi

# ── 2. CI token + basic-auth blob so CI can READ the certs repo over HTTPS ────
CI_TOKEN="${MATCH_CI_TOKEN:-$(gh auth token)}"
GH_USER="$(gh api user -q .login)"
MATCH_GIT_BASIC_AUTHORIZATION="$(printf '%s' "${GH_USER}:${CI_TOKEN}" | base64 | tr -d '\n')"

# ── 3. create + store the MAS signing material via fastlane match ────────────
ASC_KEY_P8="$(cat "$ASC_KEY_P8_FILE")"
export ASC_KEY_ID ASC_ISSUER_ID ASC_KEY_P8
export MATCH_GIT_URL MATCH_PASSWORD MATCH_GIT_BASIC_AUTHORIZATION

echo "▶ running fastlane match for the MAS cert types + profile (creates + pushes)..."
echo "  (Mac App Distribution cert, Mac Installer Distribution cert, and the"
echo "   ai.darkrun.app Mac App Store profile — App Group + Associated Domains.)"
( cd "$ROOT/fastlane" && bundle exec fastlane mac certs )

# ── 4. load the GitHub Actions secrets the workflow reads ────────────────────
# Same secret names as iOS — the macos-appstore.yml workflow reads the identical
# ASC_* + MATCH_* set. Refreshing them here is idempotent.
echo "▶ setting Actions secrets on ${APP_REPO}..."
gh secret set ASC_KEY_ID --repo "$APP_REPO" --body "$ASC_KEY_ID"
gh secret set ASC_ISSUER_ID --repo "$APP_REPO" --body "$ASC_ISSUER_ID"
gh secret set ASC_KEY_P8 --repo "$APP_REPO" < "$ASC_KEY_P8_FILE"
gh secret set MATCH_GIT_URL --repo "$APP_REPO" --body "$MATCH_GIT_URL"
gh secret set MATCH_PASSWORD --repo "$APP_REPO" --body "$MATCH_PASSWORD"
gh secret set MATCH_GIT_BASIC_AUTHORIZATION --repo "$APP_REPO" --body "$MATCH_GIT_BASIC_AUTHORIZATION"

echo
echo "✓ macOS (MAS) signing bootstrapped. The six secrets are set on $APP_REPO."
echo
echo "  Next: uncomment the 'push:' trigger in .github/workflows/macos-appstore.yml,"
echo "  or kick the first build with:"
echo "      gh workflow run macos-appstore.yml"
echo
echo "  Phase 4 (yours, in App Store Connect — can't be scripted): add the macOS"
echo "  platform to the existing ai.darkrun.app app record, and register the App"
echo "  Group group.ai.darkrun on the App ID so the profile above validates."
echo
echo "  And move ${ASC_KEY_P8_FILE} out of Downloads — Apple only lets you"
echo "  download the .p8 once; keep it in your password manager."
