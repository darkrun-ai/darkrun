# Root inputs. Note what is NOT here: the OAuth client ids/secrets and the Sentry
# auth token. Those never pass through Terraform — the OAuth pairs live in Secret
# Manager (operator-managed), and the Sentry token is a TFC/env credential. So no
# secret can land in tfvars or state.

# ── GCP ──────────────────────────────────────────────────────────────────
variable "gcp_project" {
  description = "GCP project id that hosts the whole stack."
  type        = string
  default     = "darkrun"
}

variable "gcp_region" {
  description = "Region for Cloud Run + Artifact Registry."
  type        = string
  default     = "us-central1"
}

variable "web_image" {
  description = "Fully-qualified container image for darkrun-web. The deploy pipeline overrides this with the freshly-pushed tag."
  type        = string
  default     = "us-central1-docker.pkg.dev/darkrun/darkrun/darkrun-web:latest"
}

variable "web_base" {
  description = "Public base URL the OAuth callbacks are registered against."
  type        = string
  default     = "https://darkrun.ai"
}

variable "web_domain" {
  description = "Custom domain to map the Cloud Run service to. Empty disables the mapping (use the run.app URL)."
  type        = string
  default     = "darkrun.ai"
}

variable "firebase_project" {
  description = "Firebase project id powering the relay (Firebase-ID-token verification) and FCM remote push. Defaults to the GCP project. Empty disables the relay + remote push (the host's local OS notification still fires). Requires the FCM Admin SDK key in Secret Manager (infra/set-fcm-env.sh)."
  type        = string
  default     = "darkrun"
}

variable "min_instances" {
  description = "Cloud Run minimum instances. 0 = scale to zero."
  type        = number
  default     = 0
}

# ── Firebase Auth / Identity Platform ────────────────────────────────────
# The sign-in surface as code. authorized_domains is always managed (no secret).
# The provider ids/secrets are opt-in (manage_auth_idp) because the secret lands
# in state; supply the SECRETS via TF_VAR_* env / TFC sensitive vars, never tfvars.
variable "auth_authorized_domains" {
  description = "Firebase Auth authorized domains (origins allowed to complete the OAuth redirect). Must include app.darkrun.ai + the firebaseapp.com/web.app handler origins."
  type        = list(string)
  default = [
    "localhost",
    "darkrun.firebaseapp.com",
    "darkrun.web.app",
    "app.darkrun.ai",
    "darkrun.ai",
  ]
}

variable "manage_auth_idp" {
  description = "Also manage the GitHub + GitLab sign-in providers in Terraform. Default false (the provider resources persist the OAuth client SECRET to state, which the repo otherwise avoids). When off, only authorized_domains is tracked and the providers stay console-managed."
  type        = bool
  default     = false
}

variable "github_oauth_client_id" {
  description = "GitHub OAuth app client id for the GitHub sign-in provider (non-secret). Used only when manage_auth_idp = true."
  type        = string
  default     = ""
}

variable "github_oauth_client_secret" {
  description = "GitHub OAuth app client SECRET. Supply via TF_VAR_github_oauth_client_secret (sensitive TFC var / env), NEVER tfvars. Used only when manage_auth_idp = true."
  type        = string
  default     = ""
  sensitive   = true
}

variable "gitlab_oauth_client_id" {
  description = "GitLab OIDC application id for the oidc.gitlab provider (non-secret). Used only when manage_auth_idp = true."
  type        = string
  default     = ""
}

variable "gitlab_oauth_client_secret" {
  description = "GitLab OIDC application SECRET. Supply via TF_VAR_gitlab_oauth_client_secret (sensitive TFC var / env), NEVER tfvars. Used only when manage_auth_idp = true."
  type        = string
  default     = ""
  sensitive   = true
}

# ── DNS ──────────────────────────────────────────────────────────────────
variable "manage_dns" {
  description = "Create the Cloud DNS zone + apex/www records for web_domain. Set false to manage DNS elsewhere."
  type        = bool
  default     = true
}

variable "dns_zone_name" {
  description = "The Cloud DNS managed-zone resource name ([a-z0-9-])."
  type        = string
  default     = "darkrun-ai"
}

variable "manage_www" {
  description = "Map + point the www subdomain at the service (web mapping + dns CNAME)."
  type        = bool
  default     = true
}

variable "manage_relay" {
  description = "Map + point relay.<web_domain> at the darkrun-web service (web mapping + dns CNAME). relay.<web_domain> is the wss:// base the engine dials by default (web/server DEFAULT_RELAY_PUBLIC_URL), so this must resolve for the default remote dial to work. The DNS CNAME is published whenever manage_dns is on; the Cloud Run mapping still respects manage_domain_mapping (the verified-owner step)."
  type        = bool
  default     = true
}

variable "firebase_hosting_a_records" {
  description = "A records for app.<web_domain>, the Firebase-Hosting web app (site darkrun-app). Firebase Hosting custom domains are console/API-driven: connect app.<web_domain> in the Firebase console (Hosting -> Add custom domain), which issues an ownership TXT challenge, provisions the managed TLS cert, and then shows the exact A record IPs — paste those here. Empty (default) leaves app.<web_domain> unmanaged in DNS so Terraform never points it at a guessed/stale Firebase IP."
  type        = list(string)
  default     = []
}

variable "manage_domain_mapping" {
  description = "Let Terraform create the Cloud Run domain mappings. Default false — the mappings need a verified domain owner, which the TFC service account can't be, so they're created out-of-band by a verified human and Terraform leaves them alone (DNS records stay in Terraform)."
  type        = bool
  default     = false
}

variable "max_instances" {
  description = "Cloud Run maximum instances."
  type        = number
  default     = 4
}

# ── Sentry ───────────────────────────────────────────────────────────────
# The auth token is NOT a variable — the provider reads SENTRY_AUTH_TOKEN from the
# environment (a TFC workspace variable, or your shell). Only the non-secret slugs
# live here.
variable "sentry_organization" {
  description = "Sentry organization slug. Unused when enable_sentry = false."
  type        = string
  default     = ""
}

variable "sentry_team" {
  description = "Sentry team slug the projects are created under. Unused when enable_sentry = false."
  type        = string
  default     = ""
}

variable "enable_sentry" {
  description = "Provision the Sentry projects. Set false to deploy Cloud Run before Sentry is set up."
  type        = bool
  default     = true
}

# ── GitHub (release-secrets wiring) ──────────────────────────────────────
# Used only to push the cli/desktop Sentry DSNs into the repo's Actions secrets.
# The token is the GITHUB_TOKEN env (a TFC workspace variable), never a TF var.
variable "github_owner" {
  description = "GitHub org/user that owns the repo whose Actions secrets are set."
  type        = string
  default     = "darkrun-ai"
}

variable "github_repository" {
  description = "Repo name (under github_owner) whose Actions secrets receive the DSNs."
  type        = string
  default     = "darkrun"
}

variable "manage_release_secrets" {
  description = "Have Terraform push the cli/desktop Sentry DSNs into the repo's GitHub Actions secrets. Needs GITHUB_TOKEN set. Turn off to manage those secrets by hand."
  type        = bool
  default     = true
}
