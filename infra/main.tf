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

  # The darkrun GitHub App credentials that light up the signed-in workspace
  # endpoints (web/server/src/github_app.rs::from_env reads GITHUB_APP_ID +
  # GITHUB_APP_PRIVATE_KEY). Same model as the OAuth pairs: the values live in
  # Secret Manager ONLY (operator-seeded — `gcloud secrets versions add
  # GITHUB_APP_ID` / `GITHUB_APP_PRIVATE_KEY`), Terraform just references them by
  # name and mounts each as an env var of the same name. GITHUB_APP_PRIVATE_KEY is
  # the App's PEM (multi-line, or with literal \n escapes — from_env normalizes).
  # Absent secrets fail the plan loudly, the same "bootstrap wasn't run" signal as
  # the OAuth pairs; once seeded, the workspace lights up with no code change.
  github_app_secret_ids = [
    "GITHUB_APP_ID",
    "GITHUB_APP_PRIVATE_KEY",
  ]

  # Everything the web service references from Secret Manager and mounts as env.
  web_secret_ids = concat(local.oauth_secret_ids, local.github_app_secret_ids)
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
    # Pub/Sub: the relay's cross-instance frame bus (Step 1c). A host on one Cloud
    # Run instance and a client on another exchange frames over the `relay_frames`
    # topic (web/server/src/relay_bus.rs). Per-instance subscriptions are created
    # at RUNTIME with an expiration policy — never in Terraform.
    "pubsub.googleapis.com",
    # Identity Platform: backs Firebase Auth. The auth module tracks the sign-in
    # config (authorized domains + the GitHub/GitLab providers) as code so console
    # drift can't silently break sign-in.
    "identitytoolkit.googleapis.com",
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

# The relay's cross-instance frame bus topic (Step 1c). Every relay instance
# publishes host↔client frames here addressed by `to_instance`, and each instance
# pulls from its OWN per-instance subscription (created at runtime with an
# expiration_policy so a dead instance's subscription self-deletes — hence NO
# subscription resource here). Storage is pinned in-region to keep frames close to
# the Cloud Run service and avoid cross-region egress.
resource "google_pubsub_topic" "relay_frames" {
  project = var.gcp_project
  name    = "relay_frames"

  message_storage_policy {
    allowed_persistence_regions = [var.gcp_region]
  }

  depends_on = [google_project_service.services]
}

# Let the darkrun-web runtime SA PUBLISH frames to the topic (topic-scoped — the
# narrowest publish grant).
resource "google_pubsub_topic_iam_member" "web_frame_publisher" {
  project = var.gcp_project
  topic   = google_pubsub_topic.relay_frames.name
  role    = "roles/pubsub.publisher"
  member  = "serviceAccount:${module.web.service_account}"
}

# The darkrun-web runtime SA creates its OWN per-instance subscription at boot
# (web/server/src/relay_bus.rs — PubSubFrameBus::ensure_subscription does a runtime
# PUT subscriptions.create) and then pulls/acks it. roles/pubsub.subscriber grants
# consume/get/delete but NOT pubsub.subscriptions.create — that lives only in the
# far-broader roles/pubsub.editor. Without create, the boot-time PUT 403s and the
# whole receive path goes silently dark. Grant a LEAST-PRIVILEGE custom role with
# exactly the subscription-lifecycle permissions the runtime needs, nothing more
# (publish stays on the topic-scoped roles/pubsub.publisher above).
resource "google_project_iam_custom_role" "relay_frame_subscriber" {
  project     = var.gcp_project
  role_id     = "relayFrameSubscriber"
  title       = "Relay Frame Subscriber"
  description = "Create, consume, and tear down the relay's per-instance Pub/Sub subscriptions at runtime."
  permissions = [
    "pubsub.subscriptions.create",
    "pubsub.subscriptions.consume",
    "pubsub.subscriptions.get",
    "pubsub.subscriptions.delete",
    "pubsub.topics.attachSubscription",
  ]
}

# Bind the darkrun-web runtime SA to that custom role. Project-level because the
# subscription name is derived per-instance at runtime, not a fixed Terraform
# resource.
resource "google_project_iam_member" "web_frame_subscriber" {
  project = var.gcp_project
  role    = google_project_iam_custom_role.relay_frame_subscriber.id
  member  = "serviceAccount:${module.web.service_account}"

  depends_on = [google_project_service.services]
}

# Let the darkrun-web runtime SA read+write Firestore (the relay's device
# registry, relay-token broker, and session registry — Step 1a/1b — all write
# `(default)` with this SA via the REST API). Was implicitly relied on; grant it
# explicitly so the relay's Firestore access isn't dependent on a broader role.
resource "google_project_iam_member" "web_datastore_user" {
  project = var.gcp_project
  role    = "roles/datastore.user"
  member  = "serviceAccount:${module.web.service_account}"

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
  external_secret_ids = local.web_secret_ids
  manage_www          = var.manage_www
  manage_relay        = var.manage_relay

  # Relay + FCM remote push run on this Firebase project. Empty disables both.
  firebase_project = var.firebase_project

  # The cross-instance frame bus topic (Step 1c) — turns DARKRUN_PUBSUB_TOPIC on in
  # the service when the relay is enabled, so a split host/client pair can exchange
  # frames across instances.
  pubsub_topic = google_pubsub_topic.relay_frames.name

  manage_domain_mapping = var.manage_domain_mapping

  depends_on = [google_project_service.services]
}

# Firebase Auth / Identity Platform sign-in config as code: the authorized
# domains (always) and, opt-in, the GitHub + GitLab providers. Captures what used
# to live only in the Firebase console so console drift surfaces as a plan diff.
module "auth" {
  source = "./modules/auth"

  project            = var.firebase_project != "" ? var.firebase_project : var.gcp_project
  authorized_domains = var.auth_authorized_domains

  # Off by default: the provider resources require the OAuth client SECRET, which
  # Terraform persists to state (the repo keeps OAuth secrets out of state). When
  # off, only authorized_domains is tracked and the providers stay console-managed.
  manage_idp           = var.manage_auth_idp
  github_client_id     = var.github_oauth_client_id
  github_client_secret = var.github_oauth_client_secret
  gitlab_client_id     = var.gitlab_oauth_client_id
  gitlab_client_secret = var.gitlab_oauth_client_secret

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

  # relay.<domain> is the wss:// base the engine dials by default; publish its
  # CNAME so the default remote dial resolves (the Cloud Run mapping lives in the
  # web module, gated on manage_domain_mapping).
  manage_relay = var.manage_relay

  # app.<domain> (Firebase Hosting) A records — operator-supplied, copied from the
  # Firebase console when the custom domain is connected. Empty leaves app.
  # unmanaged in DNS (see the variable's doc).
  firebase_hosting_a_records = var.firebase_hosting_a_records

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
