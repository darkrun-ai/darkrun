variable "enable" {
  description = "Create the managed zone + records. False yields no DNS resources."
  type        = bool
  default     = true
}

variable "domain" {
  description = "The apex domain to manage (e.g. darkrun.ai), no trailing dot."
  type        = string
}

variable "zone_name" {
  description = "The Cloud DNS managed-zone resource name ([a-z0-9-])."
  type        = string
  default     = "darkrun-ai"
}

variable "manage_www" {
  description = "Also create a www CNAME -> Cloud Run (needs the www domain mapping to serve)."
  type        = bool
  default     = true
}

variable "manage_relay" {
  description = "Also create a relay CNAME -> Cloud Run (needs the relay domain mapping in the web module to serve). relay.<domain> is the wss:// base the engine dials by default, so this should be on for the default remote dial to resolve."
  type        = bool
  default     = true
}

variable "relay_subdomain" {
  description = "Subdomain label for the relay CNAME (default \"relay\" => relay.<domain>)."
  type        = string
  default     = "relay"
}

variable "app_subdomain" {
  description = "Subdomain label for the Firebase-Hosting web app A record (default \"app\" => app.<domain>)."
  type        = string
  default     = "app"
}

variable "firebase_hosting_a_records" {
  description = "A records for the app.<domain> Firebase Hosting custom domain. COPY THESE FROM THE FIREBASE CONSOLE when you connect app.<domain> (Hosting -> Add custom domain): the console issues an ownership TXT challenge and provisions the managed cert, then shows the exact A record IPs. Empty (default) leaves app.<domain> unmanaged in DNS until you supply them."
  type        = list(string)
  default     = []
}

variable "ttl" {
  description = "TTL (seconds) for the records."
  type        = number
  default     = 3600
}

# Google's anycast front-end IPs for Cloud Run / GCLB custom-domain mappings.
# The apex can't be a CNAME, so it points at these A/AAAA records; Cloud Run's
# domain mapping then routes the verified domain to the service.
variable "cloud_run_a_records" {
  description = "Apex A records (Google domain-mapping anycast IPs)."
  type        = list(string)
  default     = ["216.239.32.21", "216.239.34.21", "216.239.36.21", "216.239.38.21"]
}

variable "cloud_run_aaaa_records" {
  description = "Apex AAAA records (Google domain-mapping anycast IPs)."
  type        = list(string)
  default = [
    "2001:4860:4802:32::15",
    "2001:4860:4802:34::15",
    "2001:4860:4802:36::15",
    "2001:4860:4802:38::15",
  ]
}
