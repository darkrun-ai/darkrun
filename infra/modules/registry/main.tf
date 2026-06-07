# Docker repository for the darkrun-web image. Persists across service redeploys,
# so it lives in its own module separate from the (re-rolled) Cloud Run service.
resource "google_artifact_registry_repository" "this" {
  location      = var.region
  repository_id = var.repository_id
  format        = "DOCKER"
  description   = "darkrun container images (darkrun-web)."
}
