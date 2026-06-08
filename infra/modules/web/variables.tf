variable "project" {
  description = "GCP project id (used for the domain-mapping namespace)."
  type        = string
}

variable "region" {
  description = "Region for Cloud Run."
  type        = string
}

variable "web_image" {
  description = "Fully-qualified container image for darkrun-web."
  type        = string
}

variable "web_base" {
  description = "Public base URL the OAuth callbacks are registered against."
  type        = string
}

variable "web_domain" {
  description = "Custom domain to map. Empty disables the domain mapping."
  type        = string
  default     = ""
}

variable "manage_www" {
  description = "Also map the www subdomain to the service (paired with the www CNAME in the dns module)."
  type        = bool
  default     = true
}

variable "manage_domain_mapping" {
  description = "Let Terraform create the Cloud Run domain mappings. Default false: a domain mapping requires the CALLER to be a verified owner of the domain, and the TFC/CI service account can't be added as a Search Console owner — so the apex/www mappings are created out-of-band by a verified human (`gcloud run domain-mappings create`) and Terraform leaves them alone. DNS records + everything else stay in Terraform."
  type        = bool
  default     = false
}

variable "min_instances" {
  description = "Cloud Run minimum instances (0 = scale to zero)."
  type        = number
  default     = 0
}

variable "max_instances" {
  description = "Cloud Run maximum instances."
  type        = number
  default     = 4
}

variable "external_secret_ids" {
  description = "Secret Manager secret_ids that already exist (operator-managed; the OAuth id/secret pairs). Mounted into the service as env, never created by Terraform."
  type        = list(string)
}

variable "enable_sentry" {
  description = "Whether the web Sentry DSN secret is provisioned + mounted."
  type        = bool
  default     = true
}

variable "sentry_dsn" {
  description = "The web-surface public DSN (from the sentry module). Empty leaves the secret versionless."
  type        = string
  default     = ""
}
