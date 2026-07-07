# Firebase Auth / Google Identity Platform sign-in config as code.
#
# Why this exists: the sign-in surface (authorized domains, the GitHub built-in
# provider, and the `oidc.gitlab` generic-OIDC provider the web app requests via
# OAuthProvider("oidc.gitlab")) previously lived ONLY in the Firebase console.
# A stray console edit (dropping app.darkrun.ai from the authorized domains,
# disabling a provider, changing the OIDC issuer) silently breaks sign-in with no
# diagnosable source of truth. This module makes the console the RENDER of the
# repo, not the origin, so drift shows up as a plan diff.
#
# Secrets model (mirrors the rest of infra): the config resource carries NO
# secret, so it's always managed. The provider (IdP) resources DO require an
# OAuth client secret, and Terraform would persist that secret into state, which
# the repo's invariant forbids (the OAuth pairs live in Secret Manager only,
# referenced never valued; see infra/main.tf + modules/web/main.tf). So the IdP
# resources are OPT-IN behind var.manage_idp (default off): by default the two
# providers stay console-managed and only their authorized-domain surface is
# IaC-tracked. An operator who accepts the secret-in-state tradeoff can flip
# manage_idp on and supply the client id/secret via SENSITIVE env vars
# (TF_VAR_github_client_secret / TF_VAR_gitlab_client_secret, or TFC sensitive
# workspace variables). Never tfvars, never committed.

# The project-level Identity Platform config singleton. Authorized domains are the
# highest-value drift target: the app's authDomain is app.darkrun.ai, and the
# redirect round-trip also needs the firebaseapp.com / web.app handler origins.
# Pinning the full allowlist here means a console deletion re-appears as a diff.
resource "google_identity_platform_config" "default" {
  project            = var.project
  authorized_domains = var.authorized_domains

  # One account per email address. The app's account-linking UX depends on it:
  # a second provider for the same email raises account-exists-with-different-
  # credential, which the app catches and routes into linkWithRedirect (see
  # web/app/js/firebase-login.js). allow_duplicate_emails = true would silently
  # create parallel accounts and break that flow, so pin it false.
  sign_in {
    allow_duplicate_emails = false
  }
}

# GitHub: a Firebase built-in ("default supported") IdP. idp_id is the fixed
# "github.com". Opt-in (var.manage_idp) because client_secret would land in state.
# When off, GitHub stays console-managed and this is a no-op.
resource "google_identity_platform_default_supported_idp_config" "github" {
  count = var.manage_idp ? 1 : 0

  project       = var.project
  idp_id        = "github.com"
  enabled       = true
  client_id     = var.github_client_id
  client_secret = var.github_client_secret

  depends_on = [google_identity_platform_config.default]
}

# GitLab: a generic OIDC provider. name MUST be "oidc.gitlab" to match the app's
# OAuthProvider("oidc.gitlab"). Same opt-in gate as GitHub (client_secret reaches
# state). issuer is gitlab.com's OIDC discovery origin; the app requests the
# read_api scope (openid is implicit).
resource "google_identity_platform_oauth_idp_config" "gitlab" {
  count = var.manage_idp ? 1 : 0

  project       = var.project
  name          = "oidc.gitlab"
  display_name  = "GitLab"
  issuer        = var.gitlab_issuer
  enabled       = true
  client_id     = var.gitlab_client_id
  client_secret = var.gitlab_client_secret

  depends_on = [google_identity_platform_config.default]
}
