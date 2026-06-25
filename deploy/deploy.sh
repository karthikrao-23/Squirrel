#!/usr/bin/env bash
# Build the image (Cloud Build, tagged by git SHA), push to Artifact Registry,
# and deploy the Cloud Run service with all env + secrets + Cloud SQL wired in.
# Idempotent: re-run to ship a new revision.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_vars APP_ENV PLAID_ENV BASE_URL RUN_MEMORY RUN_CPU RUN_CONCURRENCY \
  RUN_MIN_INSTANCES RUN_MAX_INSTANCES

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GIT_SHA="$(git -C "$repo_root" rev-parse --short HEAD)"
IMAGE="${IMAGE_REPO}:${GIT_SHA}"

log "Building image with Cloud Build: $IMAGE"
gc builds submit "$repo_root" --tag "$IMAGE"

# Secret-backed env. Required ones must already exist (secrets.sh).
secrets="DATABASE_URL=squirrel-database-url:latest"
secrets+=",TOKEN_ENCRYPTION_KEY=squirrel-token-encryption-key:latest"
secrets+=",PLAID_CLIENT_ID=squirrel-plaid-client-id:latest"
secrets+=",PLAID_SECRET=squirrel-plaid-secret:latest"
secrets+=",INTERNAL_API_TOKEN=squirrel-internal-api-token:latest"
if gc secrets describe squirrel-smtp-host >/dev/null 2>&1; then
  secrets+=",SMTP_HOST=squirrel-smtp-host:latest"
  secrets+=",SMTP_USERNAME=squirrel-smtp-username:latest"
  secrets+=",SMTP_PASSWORD=squirrel-smtp-password:latest"
fi

# Plain env. STATIC_DIR is also baked into the image; set explicitly for clarity.
# SCHEDULER_ENABLED=false — Cloud Scheduler drives the cycle in prod.
envs="APP_ENV=${APP_ENV}"
envs+=",PLAID_ENV=${PLAID_ENV}"
envs+=",RUST_LOG=info"
envs+=",STATIC_DIR=/app/dist"
envs+=",SCHEDULER_ENABLED=false"
envs+=",APP_ORIGIN=${BASE_URL}"
envs+=",PLAID_WEBHOOK_URL=${BASE_URL}/api/plaid/webhook"

log "Deploying Cloud Run service: $RUN_SERVICE"
# Public (SPA + Plaid webhook need unauthenticated access). The internal endpoint
# is gated at the app layer by INTERNAL_API_TOKEN — see scheduler.sh.
gc run deploy "$RUN_SERVICE" \
  --image "$IMAGE" \
  --region "$REGION" \
  --service-account "$RUN_SA_EMAIL" \
  --add-cloudsql-instances "$SQL_CONN" \
  --set-env-vars "$envs" \
  --set-secrets "$secrets" \
  --port 8080 \
  --memory "$RUN_MEMORY" \
  --cpu "$RUN_CPU" \
  --concurrency "$RUN_CONCURRENCY" \
  --min-instances "$RUN_MIN_INSTANCES" \
  --max-instances "$RUN_MAX_INSTANCES" \
  --ingress all \
  --allow-unauthenticated

url="$(gc run services describe "$RUN_SERVICE" --region "$REGION" --format='value(status.url)')"
log "Deployed. Service URL: $url"
echo "  - If BASE_URL ($BASE_URL) differs from the service URL, map your domain"
echo "    (see DEPLOYMENT.md), then re-run deploy.sh so APP_ORIGIN/PLAID_WEBHOOK_URL match."
echo "  - Set the Plaid dashboard webhook to: ${BASE_URL}/api/plaid/webhook"
echo "  - Liveness/readiness: point a Cloud Run HTTP probe at /health (stays public)."
