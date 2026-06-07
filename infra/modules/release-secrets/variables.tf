variable "enable" {
  description = "Whether to manage the release Actions secrets (needs a GitHub token)."
  type        = bool
  default     = true
}

variable "repository" {
  description = "The repo (name only; owner comes from the github provider) whose Actions secrets are set."
  type        = string
}

variable "cli_dsn" {
  description = "The CLI-surface Sentry DSN to bake into the release build."
  type        = string
  default     = ""
}

variable "desktop_dsn" {
  description = "The desktop-surface Sentry DSN to bake into the release build."
  type        = string
  default     = ""
}
