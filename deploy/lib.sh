#!/usr/bin/env bash
# Shared helpers for the deploy scripts. Sourced, not executed.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -f "$here/config.env" ]]; then
  echo "error: deploy/config.env not found — copy config.env.example and fill it in" >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$here/config.env"

# Fail early if a required variable is unset/empty.
require_vars() {
  local missing=()
  for v in "$@"; do
    if [[ -z "${!v:-}" ]]; then missing+=("$v"); fi
  done
  if (( ${#missing[@]} > 0 )); then
    echo "error: missing required variables: ${missing[*]}" >&2
    exit 1
  fi
}

# Derived, reused everywhere.
require_vars PROJECT_ID REGION AR_REPO SQL_INSTANCE DB_NAME DB_USER RUN_SERVICE RUN_SA
export SQL_CONN="${PROJECT_ID}:${REGION}:${SQL_INSTANCE}"
export IMAGE_REPO="${REGION}-docker.pkg.dev/${PROJECT_ID}/${AR_REPO}/api"
export RUN_SA_EMAIL="${RUN_SA}@${PROJECT_ID}.iam.gserviceaccount.com"

gc() { gcloud --project "$PROJECT_ID" "$@"; }

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }

# Create a Secret Manager secret if absent, then add a new version from stdin.
upsert_secret() {
  local name="$1"
  if ! gc secrets describe "$name" >/dev/null 2>&1; then
    gc secrets create "$name" --replication-policy=automatic >/dev/null
  fi
  gc secrets versions add "$name" --data-file=- >/dev/null
  echo "  secret $name updated"
}
