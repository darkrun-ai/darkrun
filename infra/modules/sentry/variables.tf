variable "enable" {
  description = "Provision the Sentry projects. False yields an empty DSN map (deploy Cloud Run before Sentry exists)."
  type        = bool
  default     = true
}

variable "organization" {
  description = "Sentry organization slug."
  type        = string
  default     = ""
}

variable "team" {
  description = "Sentry team slug the projects are created under."
  type        = string
  default     = ""
}
