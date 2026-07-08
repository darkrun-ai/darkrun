# The darkrun-web Cloud Run service: the OAuth broker + the static site in one
# container. Runs as a dedicated least-privilege service account, scales to zero,
# mounts all config + secrets from Secret Manager, and is publicly invocable
# (a website + an OAuth callback the browser hits).
#
# Secrets model: the OAuth client id/secret pairs live ENTIRELY in Secret Manager
# (operator-managed; bootstrap.sh / `gcloud secrets versions add`). Terraform only
# references them by name — no Terraform variable carries a value, so nothing
# sensitive lands in tfvars or state. The web Sentry DSN is the one exception: a
# public ingest key derived from the sentry module, written here directly.

resource "google_service_account" "web" {
  account_id   = "darkrun-web"
  display_name = "darkrun-web Cloud Run service account"
}

# Reference the operator-managed OAuth secrets (must already exist). A missing one
# fails the plan loudly — the right signal that bootstrap wasn't run.
data "google_secret_manager_secret" "external" {
  for_each  = toset(var.external_secret_ids)
  secret_id = each.value
}

# The web Sentry DSN: created + versioned by Terraform (public key, derived from
# the sentry module). Only when Sentry is enabled.
resource "google_secret_manager_secret" "sentry_dsn" {
  count     = var.enable_sentry ? 1 : 0
  secret_id = "DARKRUN_SENTRY_DSN"
  replication {
    auto {}
  }
}

# Gated on the static enable_sentry only — the DSN value isn't known until apply
# (it's read from the freshly-created Sentry project), so it can't drive `count`.
# When Sentry is enabled the DSN is always populated, so this is safe.
resource "google_secret_manager_secret_version" "sentry_dsn" {
  count       = var.enable_sentry ? 1 : 0
  secret      = google_secret_manager_secret.sentry_dsn[0].id
  secret_data = var.sentry_dsn
}

locals {
  # env var name => Secret Manager secret_id mounted into Cloud Run.
  secret_env = merge(
    { for s in var.external_secret_ids : s => s },
    var.enable_sentry ? { DARKRUN_SENTRY_DSN = "DARKRUN_SENTRY_DSN" } : {},
  )

  # Relay + FCM remote push are enabled when a Firebase project is configured.
  fcm_enabled = var.firebase_project != ""
  # Where the Admin SDK key is mounted; GOOGLE_APPLICATION_CREDENTIALS points here.
  fcm_mount_dir = "/secrets/fcm"
  fcm_key_path  = "${local.fcm_mount_dir}/key.json"

  # The cross-instance frame bus (Step 1c) is enabled when the relay is on AND a
  # Pub/Sub topic is wired — then DARKRUN_PUBSUB_TOPIC turns it on in the service.
  frame_bus_enabled = local.fcm_enabled && var.pubsub_topic != ""
}

# The FCM Admin SDK key (operator-managed; created by infra/set-fcm-env.sh). Like
# the OAuth secrets, referenced not created — a missing one fails the plan loudly.
data "google_secret_manager_secret" "fcm" {
  count     = local.fcm_enabled ? 1 : 0
  secret_id = var.fcm_secret_id
}

resource "google_secret_manager_secret_iam_member" "fcm_accessor" {
  count     = local.fcm_enabled ? 1 : 0
  secret_id = data.google_secret_manager_secret.fcm[0].secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.web.email}"
}

# Grant the service account accessor on every secret it consumes.
resource "google_secret_manager_secret_iam_member" "accessor" {
  for_each = merge(
    { for s in var.external_secret_ids : s => data.google_secret_manager_secret.external[s].id },
    var.enable_sentry ? { DARKRUN_SENTRY_DSN = google_secret_manager_secret.sentry_dsn[0].id } : {},
  )
  secret_id = each.value
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.web.email}"
}

resource "google_cloud_run_v2_service" "web" {
  name     = "darkrun-web"
  location = var.region
  ingress  = "INGRESS_TRAFFIC_ALL"

  template {
    service_account = google_service_account.web.email

    # Long-lived WebSocket sessions (the relay host park + client attach) must not
    # be cut at the 300s default. Raise the request timeout toward Cloud Run's
    # 3600s max so a parked host socket survives.
    timeout = "3600s"

    scaling {
      min_instance_count = var.min_instances
      # With the cross-instance frame bus (Step 1c) a host on one instance and a
      # client on another can now exchange frames, so max_instances can be raised
      # safely — a split pair is no longer stranded. Left as-is here (var-driven).
      max_instance_count = var.max_instances
    }

    containers {
      image = var.web_image

      ports {
        container_port = 8080
      }

      # Non-secret config.
      env {
        name  = "DARKRUN_WEB_ADDR"
        value = "0.0.0.0:8080"
      }
      env {
        name  = "DARKRUN_WEB_BASE"
        value = var.web_base
      }
      env {
        name  = "DARKRUN_ENV"
        value = "production"
      }

      # Relay + FCM: turn the relay on and point the token source at the mounted
      # Admin SDK key. Gated on firebase_project so non-relay deploys stay clean.
      dynamic "env" {
        for_each = local.fcm_enabled ? {
          DARKRUN_FIREBASE_PROJECT       = var.firebase_project
          GOOGLE_APPLICATION_CREDENTIALS = local.fcm_key_path
        } : {}
        content {
          name  = env.key
          value = env.value
        }
      }

      # Cross-instance frame bus (Step 1c): the Pub/Sub topic the relay publishes
      # host↔client frames to. Gated on the relay being on AND a topic being wired.
      dynamic "env" {
        for_each = local.frame_bus_enabled ? {
          DARKRUN_PUBSUB_TOPIC = var.pubsub_topic
        } : {}
        content {
          name  = env.key
          value = env.value
        }
      }

      # Everything from Secret Manager (latest version): the OAuth id/secret pairs
      # and (when enabled) the web Sentry DSN.
      dynamic "env" {
        for_each = local.secret_env
        content {
          name = env.key
          value_source {
            secret_key_ref {
              secret  = env.value
              version = "latest"
            }
          }
        }
      }

      # Mount the Admin SDK key as a FILE (the token source reads a path, so a
      # secret env-value won't do). Paired with the `volumes` block below.
      dynamic "volume_mounts" {
        for_each = local.fcm_enabled ? [1] : []
        content {
          name       = "fcm-key"
          mount_path = local.fcm_mount_dir
        }
      }
    }

    # The FCM key secret volume, surfaced as `key.json` under the mount dir.
    dynamic "volumes" {
      for_each = local.fcm_enabled ? [1] : []
      content {
        name = "fcm-key"
        secret {
          secret = data.google_secret_manager_secret.fcm[0].secret_id
          items {
            version = "latest"
            path    = "key.json"
          }
        }
      }
    }
  }

  depends_on = [
    google_secret_manager_secret_iam_member.accessor,
    google_secret_manager_secret_iam_member.fcm_accessor,
  ]
}

# Public, unauthenticated invocations (a website + OAuth callbacks).
resource "google_cloud_run_v2_service_iam_member" "public" {
  name     = google_cloud_run_v2_service.web.name
  location = google_cloud_run_v2_service.web.location
  role     = "roles/run.invoker"
  member   = "allUsers"
}

# Map the custom domain. Requires the domain verified for the project. Disabled
# when var.web_domain is empty (use the run.app URL).
resource "google_cloud_run_domain_mapping" "web" {
  count    = var.web_domain != "" && var.manage_domain_mapping ? 1 : 0
  name     = var.web_domain
  location = var.region
  metadata {
    namespace = var.project
  }
  spec {
    route_name = google_cloud_run_v2_service.web.name
  }
}

# www subdomain mapping (paired with the www CNAME in the dns module). A
# subdomain under the verified apex needs no separate verification.
resource "google_cloud_run_domain_mapping" "www" {
  count    = var.web_domain != "" && var.manage_www && var.manage_domain_mapping ? 1 : 0
  name     = "www.${var.web_domain}"
  location = var.region
  metadata {
    namespace = var.project
  }
  spec {
    route_name = google_cloud_run_v2_service.web.name
  }
}
