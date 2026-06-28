# Deploy — Cloud Run + Cloud SQL

Idempotent `gcloud` scripts that stand up Squirrel on GCP as a single public
Cloud Run service (SPA + `/api` same-origin) backed by Cloud SQL Postgres. Full
narrative in [`../DEPLOYMENT.md`](../DEPLOYMENT.md) §GCP runbook.

## Prerequisites

- `gcloud` authenticated (`gcloud auth login`) with Owner/Editor on the project.
- A billing-enabled GCP project.

## Database roles (least privilege)

Two Postgres roles, so a compromised service can't alter the schema:

- **`DB_OWNER`** — owns the schema and runs migrations (DDL). A Cloud SQL admin
  user, used *only* by `migrate.sh`.
- **`DB_USER`** — the role the Cloud Run service connects as. A *native* Postgres
  role (not a Cloud SQL admin user, so never in `cloudsqlsuperuser`) with
  `SELECT/INSERT/UPDATE/DELETE` only. Created + granted by `migrate.sh`.

The service runs with `RUN_MIGRATIONS=false`; schema changes go through the
`migrate.sh` job as the owner. Encryption at rest uses a **CMEK** key you control
when `USE_CMEK=true` (set at instance-create time only).

## One-time

```bash
cd deploy
cp config.env.example config.env        # then edit it (gitignored)

# Secrets come from your shell env, never from a file:
export DB_OWNER_PASSWORD="$(openssl rand -base64 24)"   # migration/owner role
export DB_PASSWORD="$(openssl rand -base64 24)"         # runtime (DML-only) role
export TOKEN_ENCRYPTION_KEY="$(openssl rand -base64 32)"
export INTERNAL_API_TOKEN="$(openssl rand -base64 32)"
export PLAID_CLIENT_ID="..." PLAID_SECRET="..."
# optional: SMTP_HOST / SMTP_USERNAME / SMTP_PASSWORD

./setup-infra.sh     # APIs, (CMEK key), Artifact Registry, Cloud SQL, owner role, runtime SA + IAM
./secrets.sh         # assemble owner+runtime DATABASE_URLs + push all secrets to Secret Manager
./migrate.sh         # one-off Cloud Run Job: apply migrations + provision the DML-only runtime role
```

## Each release

```bash
./migrate.sh         # only when the release adds a migration (idempotent otherwise)
./deploy.sh          # build (Cloud Build, tagged by git SHA) → push → deploy revision
./scheduler.sh       # (first time / when BASE_URL changes) hourly alert cron
```

`deploy.sh` prints the service URL. If you map a custom domain, set `BASE_URL`
in `config.env` to it and re-run `deploy.sh` so `APP_ORIGIN` (CSRF) and
`PLAID_WEBHOOK_URL` match, then `./scheduler.sh`.
