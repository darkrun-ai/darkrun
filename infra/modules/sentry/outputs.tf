output "dsns" {
  description = "Per-surface public DSNs (web/cli/desktop/site). Empty map when disabled."
  value       = local.dsns
}
