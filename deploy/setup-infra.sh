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

# CMEK (optional): a Cloud KMS key you control, used to encrypt the Cloud SQL
# instance at rest. Must exist + be granted to the Cloud SQL service agent BEFORE
# the instance is created — CMEK can only be set at instance-create time.
sql_enc_arg=()
if [[ "${USE_CMEK:-false}" == "true" ]]; then
  require_vars KMS_KEYRING KMS_KEY
  log "CMEK: Cloud KMS key $KMS_KEYRING/$KMS_KEY + Cloud SQL service-agent grant"
  gc services enable cloudkms.googleapis.com
  gc kms keyrings describe "$KMS_KEYRING" --location "$REGION" >/dev/null 2>&1 \
    || gc kms keyrings create "$KMS_KEYRING" --location "$REGION"
  gc kms keys describe "$KMS_KEY" --keyring "$KMS_KEYRING" --location "$REGION" >/dev/null 2>&1 \
    || gc kms keys create "$KMS_KEY" --keyring "$KMS_KEYRING" --location "$REGION" --purpose=encryption
  # Materialize the Cloud SQL service agent, then let it use the key.
  sql_sa="$(gcloud beta services identity create --service=sqladmin.googleapis.com \
    --project="$PROJECT_ID" --format='value(email)')"
  gc kms keys add-iam-policy-binding "$KMS_KEY" \
    --keyring "$KMS_KEYRING" --location "$REGION" \
    --member="serviceAccount:${sql_sa}" \
    --role=roles/cloudkms.cryptoKeyEncrypterDecrypter --condition=None >/dev/null
  sql_enc_arg+=(--disk-encryption-key="projects/${PROJECT_ID}/locations/${REGION}/keyRings/${KMS_KEYRING}/cryptoKeys/${KMS_KEY}")
fi

log "Cloud SQL instance: $SQL_INSTANCE (Postgres 16, $SQL_TIER${USE_CMEK:+, CMEK=$USE_CMEK})"
if ! gc sql instances describe "$SQL_INSTANCE" >/dev/null 2>&1; then
  gc sql instances create "$SQL_INSTANCE" \
    --database-version=POSTGRES_16 \
    --tier="${SQL_TIER:-db-g1-small}" \
    --region="$REGION" \
    --storage-auto-increase \
    --availability-type=ZONAL \
    ${sql_enc_arg[@]+"${sql_enc_arg[@]}"}
fi

log "Database + owner role"
gc sql databases describe "$DB_NAME" --instance "$SQL_INSTANCE" >/dev/null 2>&1 \
  || gc sql databases create "$DB_NAME" --instance "$SQL_INSTANCE"

# Only the OWNER role is created here (it runs migrations / owns the schema). The
# DML-only runtime role ($DB_USER) is a *native* Postgres role created by
# migrate.sh, so it never gains cloudsqlsuperuser membership.
require_vars DB_OWNER DB_OWNER_PASSWORD
if gc sql users list --instance "$SQL_INSTANCE" --format="value(name)" | grep -qx "$DB_OWNER"; then
  gc sql users set-password "$DB_OWNER" --instance "$SQL_INSTANCE" --password "$DB_OWNER_PASSWORD"
else
  gc sql users create "$DB_OWNER" --instance "$SQL_INSTANCE" --password "$DB_OWNER_PASSWORD"
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

log "Infra ready. Next: secrets.sh → migrate.sh → deploy.sh."
