#!/usr/bin/env bash
# Apply database migrations and provision the DML-only runtime role, as a one-off
# Cloud Run Job that runs the deployed image's `migrate` subcommand.
#
# Why a Job (not the service): migrations need the OWNER role's DDL rights, while
# the public service connects with the least-privilege runtime role. Running them
# here — in-cluster, over the Cloud SQL socket — means no psql/auth-proxy on the
# operator's machine and no DDL grant on the runtime role. Idempotent: re-run on
# every release that ships a migration.
#
# Order: setup-infra.sh → secrets.sh → migrate.sh → deploy.sh. Re-run before
# deploy.sh whenever a release adds a migration.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GIT_SHA="$(git -C "$repo_root" rev-parse --short HEAD)"
IMAGE="${IMAGE_REPO}:${GIT_SHA}"
JOB="${RUN_SERVICE}-migrate"

# migrate runs BEFORE the first deploy (the service can't start until the runtime
# role exists), so build the image here if this SHA isn't in Artifact Registry
# yet. deploy.sh reuses the same tag.
if ! gc artifacts docker images describe "$IMAGE" >/dev/null 2>&1; then
  log "Image $IMAGE not present — building with Cloud Build"
  gc builds submit "$repo_root" --tag "$IMAGE"
fi

# Owner DATABASE_URL drives the migration; RUNTIME_DB_USER/PASSWORD let the job
# create + grant the runtime role. RUN_MIGRATIONS is irrelevant here (the
# `migrate` subcommand always migrates), but we keep the job env minimal.
secrets="DATABASE_URL=squirrel-owner-database-url:latest"
secrets+=",RUNTIME_DB_PASSWORD=squirrel-runtime-db-password:latest"
envs="RUNTIME_DB_USER=${DB_USER},RUST_LOG=info"

log "Configuring migrate job: $JOB ($IMAGE)"
if gc run jobs describe "$JOB" --region "$REGION" >/dev/null 2>&1; then
  gc run jobs update "$JOB" \
    --image "$IMAGE" --region "$REGION" \
    --service-account "$RUN_SA_EMAIL" \
    --add-cloudsql-instances "$SQL_CONN" \
    --args=migrate \
    --set-env-vars "$envs" \
    --set-secrets "$secrets" \
    --max-retries 1 --task-timeout 600
else
  gc run jobs create "$JOB" \
    --image "$IMAGE" --region "$REGION" \
    --service-account "$RUN_SA_EMAIL" \
    --add-cloudsql-instances "$SQL_CONN" \
    --args=migrate \
    --set-env-vars "$envs" \
    --set-secrets "$secrets" \
    --max-retries 1 --task-timeout 600
fi

log "Executing migrate job (waiting for completion)"
gc run jobs execute "$JOB" --region "$REGION" --wait

log "Migrations applied + runtime role provisioned (DML-only). Next: deploy.sh."
