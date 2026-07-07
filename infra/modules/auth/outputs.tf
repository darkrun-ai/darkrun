output "authorized_domains" {
  description = "The Firebase Auth authorized domains now tracked as code."
  value       = google_identity_platform_config.default.authorized_domains
}

output "managed_providers" {
  description = "Sign-in providers Terraform manages (empty when manage_idp = false; they stay console-managed then)."
  value       = var.manage_idp ? ["github.com", "oidc.gitlab"] : []
}
