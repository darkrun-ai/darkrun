# The authoritative Cloud DNS zone for the domain + the records that point the
# apex and www at the Cloud Run service. After apply, set the registrar's
# nameservers to this zone's name_servers (the module output).
#
# DNS is decoupled from the Cloud Run domain mapping on purpose: the zone +
# records can exist before the domain is verified, so you can provision DNS first.
# The mapping (in the web module) handles routing/TLS once the domain is verified.

resource "google_dns_managed_zone" "primary" {
  count       = var.enable ? 1 : 0
  name        = var.zone_name
  dns_name    = "${var.domain}."
  description = "darkrun authoritative zone for ${var.domain}"
}

# Apex -> Cloud Run (A + AAAA; the apex cannot be a CNAME).
resource "google_dns_record_set" "apex_a" {
  count        = var.enable ? 1 : 0
  managed_zone = google_dns_managed_zone.primary[0].name
  name         = "${var.domain}."
  type         = "A"
  ttl          = var.ttl
  rrdatas      = var.cloud_run_a_records
}

resource "google_dns_record_set" "apex_aaaa" {
  count        = var.enable ? 1 : 0
  managed_zone = google_dns_managed_zone.primary[0].name
  name         = "${var.domain}."
  type         = "AAAA"
  ttl          = var.ttl
  rrdatas      = var.cloud_run_aaaa_records
}

# www -> Cloud Run via CNAME (Cloud Run serves www once the www domain mapping
# in the web module is in place).
resource "google_dns_record_set" "www" {
  count        = var.enable && var.manage_www ? 1 : 0
  managed_zone = google_dns_managed_zone.primary[0].name
  name         = "www.${var.domain}."
  type         = "CNAME"
  ttl          = var.ttl
  rrdatas      = ["ghs.googlehosted.com."]
}

# relay.<domain> -> Cloud Run via CNAME. This is the wss:// base the engine dials
# by DEFAULT (web/server DEFAULT_RELAY_PUBLIC_URL = "wss://relay.darkrun.ai"),
# served by the same darkrun-web Cloud Run service. Without this record the
# default remote dial fails to resolve. Paired with the relay Cloud Run domain
# mapping in the web module; as a subdomain of the verified apex it needs no
# separate domain verification (same as www). The CNAME is published here
# whenever DNS is managed; the mapping is the one operator-verified step (gated on
# manage_domain_mapping in the web module).
resource "google_dns_record_set" "relay" {
  count        = var.enable && var.manage_relay ? 1 : 0
  managed_zone = google_dns_managed_zone.primary[0].name
  name         = "${var.relay_subdomain}.${var.domain}."
  type         = "CNAME"
  ttl          = var.ttl
  rrdatas      = ["ghs.googlehosted.com."]
}

# app.<domain> -> Firebase Hosting (the darkrun-app site), NOT Cloud Run. Firebase
# Hosting custom domains are CONSOLE/API-driven: connecting app.<domain> in the
# Firebase console (Hosting -> Add custom domain) issues an ownership TXT
# challenge, provisions the managed TLS cert, and then hands back the exact A
# record IPs to publish. Supply those via var.firebase_hosting_a_records and this
# record set appears; empty leaves app.<domain> unmanaged so Terraform never
# points it at a guessed/stale Firebase IP. That console connection is the one
# operator step; once the records are supplied, DNS is one apply away.
resource "google_dns_record_set" "app" {
  count        = var.enable && length(var.firebase_hosting_a_records) > 0 ? 1 : 0
  managed_zone = google_dns_managed_zone.primary[0].name
  name         = "${var.app_subdomain}.${var.domain}."
  type         = "A"
  ttl          = var.ttl
  rrdatas      = var.firebase_hosting_a_records
}
