#!/usr/bin/env bash
# One-time (idempotent) project setup: APIs, Artifact Registry, Cloud SQL, the
# runtime service account, and its IAM. Safe to re-run.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

log "Enabling required APIs"
gc services enable \
  run.googleapis.com \
  artifactregistry.googleapis.com \
  sqladmin.googleapis.com \
  secretmanager.googleapis.com \
  cloudscheduler.googleapis.com \
  cloudbuild.googleapis.com

log "Artifact Registry repo: $AR_REPO"
if ! gc artifacts repositories describe "$AR_REPO" --location "$REGION" >/dev/null 2>&1; then
  gc artifacts repositories create "$AR_REPO" \
    --repository-format=docker --location "$REGION" \
    --description="Squirrel container images"
fi

log "Cloud SQL instance: $SQL_INSTANCE (Postgres 16, $SQL_TIER)"
if ! gc sql instances describe "$SQL_INSTANCE" >/dev/null 2>&1; then
  gc sql instances create "$SQL_INSTANCE" \
    --database-version=POSTGRES_16 \
    --tier="${SQL_TIER:-db-g1-small}" \
    --region="$REGION" \
    --storage-auto-increase \
    --availability-type=ZONAL
fi

log "Database + application user"
gc sql databases describe "$DB_NAME" --instance "$SQL_INSTANCE" >/dev/null 2>&1 \
  || gc sql databases create "$DB_NAME" --instance "$SQL_INSTANCE"

require_vars DB_PASSWORD
if gc sql users list --instance "$SQL_INSTANCE" --format="value(name)" | grep -qx "$DB_USER"; then
  gc sql users set-password "$DB_USER" --instance "$SQL_INSTANCE" --password "$DB_PASSWORD"
else
  gc sql users create "$DB_USER" --instance "$SQL_INSTANCE" --password "$DB_PASSWORD"
fi

log "Runtime service account: $RUN_SA_EMAIL"
gc iam service-accounts describe "$RUN_SA_EMAIL" >/dev/null 2>&1 \
  || gc iam service-accounts create "$RUN_SA" --display-name="Squirrel Cloud Run runtime"

log "Granting the runtime SA Cloud SQL client + Secret accessor"
gc projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:${RUN_SA_EMAIL}" \
  --role="roles/cloudsql.client" --condition=None >/dev/null
gc projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:${RUN_SA_EMAIL}" \
  --role="roles/secretmanager.secretAccessor" --condition=None >/dev/null

log "Infra ready. Next: secrets.sh, then deploy.sh."
