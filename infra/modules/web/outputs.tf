output "web_url" {
  description = "The Cloud Run service URL (the run.app URL; the custom domain serves the same)."
  value       = google_cloud_run_v2_service.web.uri
}

output "service_account" {
  description = "The darkrun-web Cloud Run service account email."
  value       = google_service_account.web.email
}
