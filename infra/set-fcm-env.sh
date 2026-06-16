#!/usr/bin/env bash
# set-fcm-env.sh — point the darkrun-web relay at its FCM (Firebase Cloud
# Messaging) credentials, so it can mint access tokens and push remote
# notifications to registered devices.
#
# The relay's ServiceAccountTokenSource reads GOOGLE_APPLICATION_CREDENTIALS as
# a FILE PATH. Cloud Run's normal Secret Manager env mounts expose a secret's
# VALUE, not a path — so this mounts the Admin SDK key as a secret VOLUME (which
# yields a real file) and points GOOGLE_APPLICATION_CREDENTIALS at it.
#
# You supply your own Admin SDK service-account key file; this script never
# stores or echoes its contents — it hands the file straight to Secret Manager.
#
# Usage:
#   ./infra/set-fcm-env.sh /path/to/admin-sdk-key.json
#
# Optional overrides (env):
#   DARKRUN_GCP_PROJECT   (default: darkrun)
#   DARKRUN_GCP_REGION    (default: us-central1)
#
# NOTE on Terraform drift: infra/modules/web is HCP-Terraform-managed and applies
# on merge to main. This `gcloud run services update` takes effect immediately,
# but a later `terraform apply` can revert it. For a permanent setup, fold the
# secret + env vars into the web module instead of relying on this script.
set -euo pipefail

KEY_FILE="${1:-}"
if [[ -z "$KEY_FILE" ]]; then
  echo "usage: $0 <path-to-admin-sdk-key.json>" >&2
  exit 64
fi
if [[ ! -f "$KEY_FILE" ]]; then
  echo "error: no such file: $KEY_FILE" >&2
  exit 66
fi

PROJECT="${DARKRUN_GCP_PROJECT:-darkrun}"
REGION="${DARKRUN_GCP_REGION:-us-central1}"
SERVICE="darkrun-web"
SECRET_ID="FCM_SA_KEY"
WEB_SA="darkrun-web@${PROJECT}.iam.gserviceaccount.com"
MOUNT_PATH="/secrets/fcm/key.json"  # GOOGLE_APPLICATION_CREDENTIALS points here

command -v gcloud >/dev/null 2>&1 || {
  echo "error: gcloud not found — install the Google Cloud SDK first" >&2
  exit 69
}
if ! gcloud auth print-access-token >/dev/null 2>&1; then
  echo "error: not authenticated — run 'gcloud auth login' first" >&2
  exit 77
fi

echo "▶ project=${PROJECT} region=${REGION} service=${SERVICE}"
echo "▶ key file: ${KEY_FILE}"

# 1) Create the secret once, then add this key as a new version (idempotent).
if ! gcloud secrets describe "$SECRET_ID" --project "$PROJECT" >/dev/null 2>&1; then
  echo "▶ creating secret ${SECRET_ID}"
  gcloud secrets create "$SECRET_ID" --project "$PROJECT" --replication-policy=automatic
fi
echo "▶ adding a new secret version from ${KEY_FILE}"
gcloud secrets versions add "$SECRET_ID" --project "$PROJECT" --data-file="$KEY_FILE"

# 2) Let the Cloud Run service account read it.
echo "▶ granting ${WEB_SA} secretAccessor on ${SECRET_ID}"
gcloud secrets add-iam-policy-binding "$SECRET_ID" --project "$PROJECT" \
  --member="serviceAccount:${WEB_SA}" \
  --role="roles/secretmanager.secretAccessor" >/dev/null

# 3) Mount the key as a file and set both env vars on the service.
echo "▶ updating ${SERVICE}: mount ${MOUNT_PATH} + set GOOGLE_APPLICATION_CREDENTIALS, DARKRUN_FIREBASE_PROJECT"
gcloud run services update "$SERVICE" --project "$PROJECT" --region "$REGION" \
  --update-secrets="${MOUNT_PATH}=${SECRET_ID}:latest" \
  --update-env-vars="GOOGLE_APPLICATION_CREDENTIALS=${MOUNT_PATH},DARKRUN_FIREBASE_PROJECT=${PROJECT}"

echo "✓ ${SERVICE} now mints FCM tokens from ${MOUNT_PATH}"
echo "  (DARKRUN_FIREBASE_PROJECT=${PROJECT}) — remote push is live on the next request."
