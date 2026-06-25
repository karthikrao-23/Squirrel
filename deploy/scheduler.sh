#!/usr/bin/env bash
# Create/update the hourly Cloud Scheduler job that drives the alert cycle (and
# expired-session reaping) by POSTing to the internal endpoint.
#
# Auth model: the Cloud Run service is public (it serves the SPA + the Plaid
# webhook), so Cloud Run IAM can't gate `/api/internal/*` per-path. We therefore
# authenticate at the app layer with INTERNAL_API_TOKEN, sent here as a bearer
# header. (If you later split the internal endpoint into its own
# authenticated-only service, switch this job to `--oidc-service-account-email`
# with the run.invoker role and drop the header.)

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_vars BASE_URL INTERNAL_API_TOKEN

JOB="squirrel-alerts-hourly"
URI="${BASE_URL}/api/internal/alerts/run"
SCHEDULE="0 * * * *"   # top of every hour

log "Configuring Cloud Scheduler job: $JOB → $URI"
common=(
  --location "$REGION"
  --schedule "$SCHEDULE"
  --uri "$URI"
  --http-method POST
  --headers "Authorization=Bearer ${INTERNAL_API_TOKEN}"
  --attempt-deadline 60s
)

if gc scheduler jobs describe "$JOB" --location "$REGION" >/dev/null 2>&1; then
  gc scheduler jobs update http "$JOB" "${common[@]}"
else
  gc scheduler jobs create http "$JOB" "${common[@]}"
fi

log "Scheduler ready. Trigger a one-off run to verify:"
echo "  gcloud --project $PROJECT_ID scheduler jobs run $JOB --location $REGION"
