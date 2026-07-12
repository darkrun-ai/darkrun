variable "project" {
  description = "GCP / Firebase project id the Identity Platform config belongs to."
  type        = string
}

variable "authorized_domains" {
  description = "Firebase Auth authorized domains (the origins allowed to complete the OAuth redirect). Must include the app's authDomain (app.darkrun.ai) and the firebaseapp.com / web.app handler origins, or sign-in breaks."
  type        = list(string)
  default = [
    "localhost",
    "darkrun.firebaseapp.com",
    "darkrun.web.app",
    "app.darkrun.ai",
    "darkrun.ai",
  ]
}

variable "manage_idp" {
  description = "Also manage the GitHub + GitLab sign-in providers in Terraform. Off by default because the provider resources require the OAuth client SECRET, which Terraform persists to state (the repo keeps OAuth secrets out of state; see infra/main.tf). Turn on only if you accept that tradeoff AND supply the ids/secrets via sensitive env vars (TF_VAR_*), never tfvars. When off, the providers stay console-managed and only authorized_domains is IaC-tracked."
  type        = bool
  default     = false
}

variable "github_client_id" {
  description = "GitHub OAuth app client id for the GitHub sign-in provider. Non-secret; supply via tfvars or env. Used only when manage_idp = true."
  type        = string
  default     = ""
}

variable "github_client_secret" {
  description = "GitHub OAuth app client SECRET. Supply via TF_VAR_github_client_secret (a TFC sensitive workspace var or shell env), NEVER tfvars (it lands in state). Used only when manage_idp = true."
  type        = string
  default     = ""
  sensitive   = true
}

variable "gitlab_client_id" {
  description = "GitLab OIDC application id for the oidc.gitlab sign-in provider. Non-secret; supply via tfvars or env. Used only when manage_idp = true."
  type        = string
  default     = ""
}

variable "gitlab_client_secret" {
  description = "GitLab OIDC application SECRET. Supply via TF_VAR_gitlab_client_secret (a TFC sensitive workspace var or shell env), NEVER tfvars (it lands in state). Used only when manage_idp = true."
  type        = string
  default     = ""
  sensitive   = true
}

variable "gitlab_issuer" {
  description = "OIDC issuer for the oidc.gitlab provider (the GitLab instance's discovery origin)."
  type        = string
  default     = "https://gitlab.com"
}
