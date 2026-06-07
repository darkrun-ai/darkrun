output "repository_id" {
  description = "The Artifact Registry repository id."
  value       = google_artifact_registry_repository.this.repository_id
}

output "registry_path" {
  description = "The fully-qualified registry path to push images to."
  value       = "${var.region}-docker.pkg.dev/${var.project}/${google_artifact_registry_repository.this.repository_id}"
}
