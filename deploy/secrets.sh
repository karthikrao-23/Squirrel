#!/usr/bin/env bash
# Push application secrets into Secret Manager. Values are read from the
# environment (export them in your shell first) so they never land in a file or
# on a deploy command line. Re-running adds a new version of each secret.
#
# The full DATABASE_URL is stored as one secret (rather than assembling it on the
# Cloud Run deploy command) so the DB password never appears in any command,
# shell history, or process list.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

require_vars DB_PASSWORD TOKEN_ENCRYPTION_KEY PLAID_CLIENT_ID PLAID_SECRET INTERNAL_API_TOKEN

# `sslmode=disable` is correct here — and ONLY here: the connection is the Cloud
# SQL unix socket (`host=/cloudsql/...`), which is a loopback-local socket inside
# the Cloud Run sandbox, not a TCP link. There is no network to encrypt. (For a
# TCP/IP connection you would use `sslmode=require`/`verify-full`; see
# DEPLOYMENT.md §3/§11.)
DATABASE_URL="postgres://${DB_USER}:${DB_PASSWORD}@/${DB_NAME}?host=/cloudsql/${SQL_CONN}&sslmode=disable"

log "Writing secrets to Secret Manager"
printf '%s' "$DATABASE_URL"            | upsert_secret squirrel-database-url
printf '%s' "$TOKEN_ENCRYPTION_KEY"    | upsert_secret squirrel-token-encryption-key
printf '%s' "$PLAID_CLIENT_ID"         | upsert_secret squirrel-plaid-client-id
printf '%s' "$PLAID_SECRET"            | upsert_secret squirrel-plaid-secret
printf '%s' "$INTERNAL_API_TOKEN"      | upsert_secret squirrel-internal-api-token

# Optional SMTP — only written if SMTP_HOST is set.
if [[ -n "${SMTP_HOST:-}" ]]; then
  printf '%s' "$SMTP_HOST"             | upsert_secret squirrel-smtp-host
  printf '%s' "${SMTP_USERNAME:-}"     | upsert_secret squirrel-smtp-username
  printf '%s' "${SMTP_PASSWORD:-}"     | upsert_secret squirrel-smtp-password
fi

log "Secrets written. The runtime SA was granted access in setup-infra.sh."
