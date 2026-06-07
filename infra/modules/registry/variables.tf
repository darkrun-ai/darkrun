variable "project" {
  description = "GCP project id (used to build the registry path output)."
  type        = string
}

variable "region" {
  description = "Region for the Artifact Registry repository."
  type        = string
}

variable "repository_id" {
  description = "Artifact Registry repository id."
  type        = string
  default     = "darkrun"
}
