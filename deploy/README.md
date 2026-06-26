# Deploy — Cloud Run + Cloud SQL

Idempotent `gcloud` scripts that stand up Squirrel on GCP as a single public
Cloud Run service (SPA + `/api` same-origin) backed by Cloud SQL Postgres. Full
narrative in [`../DEPLOYMENT.md`](../DEPLOYMENT.md) §GCP runbook.

## Prerequisites

- `gcloud` authenticated (`gcloud auth login`) with Owner/Editor on the project.
- A billing-enabled GCP project.

## One-time

```bash
cd deploy
cp config.env.example config.env        # then edit it (gitignored)

# Secrets come from your shell env, never from a file:
export DB_PASSWORD="$(openssl rand -base64 24)"
export TOKEN_ENCRYPTION_KEY="$(openssl rand -base64 32)"
export INTERNAL_API_TOKEN="$(openssl rand -base64 32)"
export PLAID_CLIENT_ID="..." PLAID_SECRET="..."
# optional: SMTP_HOST / SMTP_USERNAME / SMTP_PASSWORD

./setup-infra.sh     # APIs, Artifact Registry, Cloud SQL, runtime SA + IAM
./secrets.sh         # assemble DATABASE_URL + push all secrets to Secret Manager
```

## Each release

```bash
./deploy.sh          # build (Cloud Build, tagged by git SHA) → push → deploy revision
./scheduler.sh       # (first time / when BASE_URL changes) hourly alert cron
```

`deploy.sh` prints the service URL. If you map a custom domain, set `BASE_URL`
in `config.env` to it and re-run `deploy.sh` so `APP_ORIGIN` (CSRF) and
`PLAID_WEBHOOK_URL` match, then `./scheduler.sh`.
