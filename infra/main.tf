# Root composition: project-level API enablement + the three modules. Everything
# targets the single GCP project `darkrun` (var.gcp_project).

locals {
  # The OAuth client id/secret pairs, by Secret Manager secret_id. These exist in
  # Secret Manager only (operator-managed via bootstrap.sh); Terraform references
  # them, never their values. The web service mounts each as an env var of the
  # same name.
  oauth_secret_ids = [
    "GITHUB_CLIENT_ID",
    "GITHUB_CLIENT_SECRET",
    "GITLAB_CLIENT_ID",
    "GITLAB_CLIENT_SECRET",
  ]
}

# Enable the GCP services the stack needs. Non-destroying so a `terraform destroy`
# doesn't disable APIs other things might share.
resource "google_project_service" "services" {
  for_each = toset([
    "run.googleapis.com",
    "artifactregistry.googleapis.com",
    "secretmanager.googleapis.com",
    "iam.googleapis.com",
    "dns.googleapis.com",
    # Hosting deploy for app.darkrun.ai (the deploy-app workflow).
    "firebasehosting.googleapis.com",
    # Firestore: the relay's persistent FCM device registry (the `devices`
    # collection). The `(default)` database itself is OPERATOR-created (gcloud /
    # console) in us-central1 — NOT a Terraform resource: its location is
    # permanent and a stray `terraform destroy` must never be able to drop it,
    # the same reason the OAuth secrets + the Hosting site are operator-managed.
    "firestore.googleapis.com",
    # Firestore Security Rules API: the deploy-app workflow publishes the
    # committed firestore.rules via the Rules REST API so the repo is the source
    # of truth for the ruleset (no console drift).
    "firebaserules.googleapis.com",
  ])
  service            = each.value
  disable_on_destroy = false
}

# Let the existing CI service account (cloudbuild-web, already Workload-Identity-
# bound to this repo in bootstrap-gha.sh) deploy the web app to Firebase Hosting.
# Reusing it keeps the deploy KEYLESS (WIF — no service-account key to store or
# leak) and grants only the least Hosting-deploy permission, far narrower than
# the Firebase Admin SDK SA. The deploy-app workflow authenticates as this SA
# via WIF, exactly like deploy-web.
resource "google_project_iam_member" "app_hosting_deployer" {
  project = var.gcp_project
  role    = "roles/firebasehosting.admin"
  member  = "serviceAccount:cloudbuild-web@${var.gcp_project}.iam.gserviceaccount.com"

  depends_on = [google_project_service.services]
}

# Let the same CI SA publish Firestore Security Rules (the deploy-app workflow
# creates a ruleset from firestore.rules and points the cloud.firestore release
# at it via the Rules REST API). Keeps the committed rules the source of truth,
# keyless via WIF, scoped to just the rules-admin permission.
resource "google_project_iam_member" "firestore_rules_deployer" {
  project = var.gcp_project
  role    = "roles/firebaserules.admin"
  member  = "serviceAccount:cloudbuild-web@${var.gcp_project}.iam.gserviceaccount.com"

  depends_on = [google_project_service.services]
}

# Let the same CI SA roll a new darkrun-web Cloud Run revision (the deploy-web
# workflow runs `gcloud run services update --image` after pushing the image).
# run.developer covers services.get/update; deploying a service that RUNS AS the
# darkrun-web runtime SA also needs actAs on that SA (iam.serviceAccountUser),
# scoped to just that SA rather than project-wide.
resource "google_project_iam_member" "web_run_deployer" {
  project = var.gcp_project
  role    = "roles/run.developer"
  member  = "serviceAccount:cloudbuild-web@${var.gcp_project}.iam.gserviceaccount.com"

  depends_on = [google_project_service.services]
}

resource "google_service_account_iam_member" "build_sa_actas_web_runtime" {
  service_account_id = "projects/${var.gcp_project}/serviceAccounts/${module.web.service_account}"
  role               = "roles/iam.serviceAccountUser"
  member             = "serviceAccount:cloudbuild-web@${var.gcp_project}.iam.gserviceaccount.com"
}

# Native Firestore TTL for the relay-token broker's `relayBroker` collection: GC
# each parked-token document once its `expiresAt` timestamp passes, server-side.
# This replaces the in-memory broker's timer sweep for the Firestore-backed store
# (web/server/src/relay_broker.rs) so abandoned deposits can't accumulate. Only
# the FIELD TTL policy is Terraform-managed here; the `(default)` database itself
# is operator-created (see the firestore.googleapis.com note above) and must never
# be a destroyable resource. TTL GC is best-effort/lagging, so the store's
# read-time `expiresAt` check stays authoritative regardless.
resource "google_firestore_field" "relay_broker_ttl" {
  project    = var.gcp_project
  database   = "(default)"
  collection = "relayBroker"
  field      = "expiresAt"

  ttl_config {}

  depends_on = [google_project_service.services]
}

# Native Firestore TTL for the relay's `sessions` collection: GC each live-session
# document once its `expiresAt` timestamp passes, server-side. A host heartbeat
# renews `expiresAt` while it lives (web/server/src/relay_registry.rs); a
# crashed/abandoned host's doc goes stale and this GCs it, so single-host-per-
# session frees up. Same caveats as the relayBroker policy: the `(default)`
# database is operator-created (never a destroyable resource here), and TTL GC is
# best-effort/lagging, so the registry's read-time `expiresAt` check stays
# authoritative regardless.
resource "google_firestore_field" "sessions_ttl" {
  project    = var.gcp_project
  database   = "(default)"
  collection = "sessions"
  field      = "expiresAt"

  ttl_config {}

  depends_on = [google_project_service.services]
}

module "sentry" {
  source = "./modules/sentry"

  enable       = var.enable_sentry
  organization = var.sentry_organization
  team         = var.sentry_team
}

# The registry is a bootstrap resource (created by gcloud/bootstrap.sh); this
# module only references it, so no depends_on on API enablement is needed.
module "registry" {
  source = "./modules/registry"

  project = var.gcp_project
  region  = var.gcp_region
}

module "web" {
  source = "./modules/web"

  project             = var.gcp_project
  region              = var.gcp_region
  web_image           = var.web_image
  web_base            = var.web_base
  web_domain          = var.web_domain
  min_instances       = var.min_instances
  max_instances       = var.max_instances
  enable_sentry       = var.enable_sentry
  sentry_dsn          = try(module.sentry.dsns["web"], "")
  external_secret_ids = local.oauth_secret_ids
  manage_www          = var.manage_www

  # Relay + FCM remote push run on this Firebase project. Empty disables both.
  firebase_project = var.firebase_project

  manage_domain_mapping = var.manage_domain_mapping

  depends_on = [google_project_service.services]
}

# The authoritative Cloud DNS zone for the domain + apex/www records pointing at
# Cloud Run. Decoupled from the domain mapping so you can provision DNS first;
# the module output exposes the nameservers to set at your registrar.
module "dns" {
  source = "./modules/dns"

  enable     = var.manage_dns && var.web_domain != ""
  domain     = var.web_domain
  zone_name  = var.dns_zone_name
  manage_www = var.manage_www

  depends_on = [google_project_service.services]
}

# Push the cli/desktop Sentry DSNs into the repo's GitHub Actions secrets so the
# release workflow bakes them into the binaries. Gated on Sentry being on AND the
# toggle (which needs a GITHUB_TOKEN). The DSNs are known after apply.
module "release_secrets" {
  source = "./modules/release-secrets"

  enable      = var.enable_sentry && var.manage_release_secrets
  repository  = var.github_repository
  cli_dsn     = try(module.sentry.dsns["cli"], "")
  desktop_dsn = try(module.sentry.dsns["desktop"], "")
}
