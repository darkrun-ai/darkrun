# Provider configuration. Auth is ambient — no credentials in code:
#   - google: Application Default Credentials locally (`gcloud auth application-
#     default login`); on TFC runners, GCP dynamic provider credentials (Workload
#     Identity) or a service-account key set as a workspace variable.
#   - sentry: the SENTRY_AUTH_TOKEN environment variable (a TFC workspace variable,
#     or your shell). Kept out of Terraform variables so it never lands in state.

provider "google" {
  project = var.gcp_project
  region  = var.gcp_region
}

provider "sentry" {
  # token read from SENTRY_AUTH_TOKEN in the environment.
}

provider "github" {
  # Owner (the org); token read from GITHUB_TOKEN in the environment (a TFC
  # workspace variable). Used only to push the cli/desktop Sentry DSNs into the
  # repo's Actions secrets. The token needs repo admin / "secrets: write".
  owner = var.github_owner
}
