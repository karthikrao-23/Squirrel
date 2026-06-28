#!/usr/bin/env bash
# Squirrel — one-command local dev bootstrap.
#
# Sets up and runs everything needed to try the app locally:
#   Postgres (Docker) + Rust API (:8080) + React frontend (:5173)
#
# Usage:
#   ./run.sh            set up if needed, then start the backend + frontend
#   ./run.sh --setup    set up only (prereqs, .env, database, deps) — don't start
#   ./run.sh --help     show this help
#
# Safe to re-run. Stop with Ctrl-C (Postgres keeps running; see the final message).

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

# ---- pretty logging ---------------------------------------------------------
if [[ -t 1 ]]; then
  bold=$'\033[1m'; blue=$'\033[1;34m'; green=$'\033[1;32m'; red=$'\033[1;31m'; dim=$'\033[2m'; rst=$'\033[0m'
else
  bold=''; blue=''; green=''; red=''; dim=''; rst=''
fi
log()  { printf '%s==>%s %s\n' "$blue" "$rst" "$*"; }
ok()   { printf '%s ✓ %s%s\n'  "$green" "$*" "$rst"; }
die()  { printf '%s error:%s %s\n' "$red" "$rst" "$*" >&2; exit 1; }

case "${1:-}" in
  -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
esac
SETUP_ONLY=0
[[ "${1:-}" == "--setup" ]] && SETUP_ONLY=1

# ---- 1. prerequisites -------------------------------------------------------
log "Checking prerequisites"
missing=0
need() {  # need <command> <how-to-install>
  if command -v "$1" >/dev/null 2>&1; then ok "$1"; else
    printf '%s ✗ %s missing%s — %s\n' "$red" "$1" "$rst" "$2"; missing=1
  fi
}
need cargo  "install Rust via https://rustup.rs"
need node   "install Node 20+ via https://nodejs.org (or nvm)"
need npm    "ships with Node.js"
need docker "install Docker Desktop via https://www.docker.com/products/docker-desktop"
need openssl "usually preinstalled; install via your package manager"
(( missing )) && die "install the missing prerequisites above, then re-run ./run.sh"

# docker compose v2 plugin, with a fallback to the legacy docker-compose binary.
if docker compose version >/dev/null 2>&1; then DC=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then DC=(docker-compose)
else die "Docker Compose not found — install Docker Desktop (bundles Compose v2)"; fi

docker info >/dev/null 2>&1 || die "the Docker daemon isn't running — start Docker Desktop and re-run"

# ---- 2. .env ----------------------------------------------------------------
if [[ ! -f .env ]]; then
  log "Creating .env from .env.example"
  cp .env.example .env
  # Generate the token-encryption key so the Plaid connect flow works out of the box.
  key="$(openssl rand -base64 32)"
  if sed --version >/dev/null 2>&1; then            # GNU sed (Linux)
    sed -i "s|^TOKEN_ENCRYPTION_KEY=.*|TOKEN_ENCRYPTION_KEY=${key}|" .env
  else                                              # BSD sed (macOS)
    sed -i '' "s|^TOKEN_ENCRYPTION_KEY=.*|TOKEN_ENCRYPTION_KEY=${key}|" .env
  fi
  ok "wrote .env (generated a TOKEN_ENCRYPTION_KEY)"
  printf '%s    To connect a brokerage, add free Plaid sandbox credentials to .env:\n    PLAID_CLIENT_ID / PLAID_SECRET — get them at https://dashboard.plaid.com%s\n' "$dim" "$rst"
else
  ok ".env already present (left unchanged)"
fi

# ---- 3. Postgres ------------------------------------------------------------
log "Starting Postgres (Docker)"
"${DC[@]}" up -d
log "Waiting for Postgres to accept connections"
for i in $(seq 1 30); do
  if "${DC[@]}" exec -T db pg_isready -U taxloss >/dev/null 2>&1; then ok "Postgres ready"; break; fi
  [[ "$i" -eq 30 ]] && die "Postgres didn't become ready in time — check: ${DC[*]} logs db"
  sleep 1
done

# ---- 4. frontend dependencies ----------------------------------------------
if [[ ! -d frontend/node_modules ]]; then
  log "Installing frontend dependencies (npm install)"
  ( cd frontend && npm install )
  ok "frontend dependencies installed"
else
  ok "frontend dependencies already installed"
fi

if (( SETUP_ONLY )); then
  log "Setup complete. Start the app any time with: ./run.sh"
  exit 0
fi

# ---- 5. run backend + frontend ---------------------------------------------
# Load .env so DATABASE_URL etc. are present for cargo (the binary also reads it).
set -a; source .env; set +a

backend_pid=""
cleanup() {
  [[ -n "$backend_pid" ]] && kill "$backend_pid" >/dev/null 2>&1 || true
  printf '\n'; log "Stopped the app. Postgres is still running — stop it with: ${DC[*]} down"
}
trap cleanup EXIT INT TERM

log "Building + starting the backend (cargo run -p api) — the first build can take a minute"
cargo run -p api &
backend_pid=$!

log "Waiting for the API to come up on :8080"
for i in $(seq 1 180); do
  if curl -fsS http://localhost:8080/health >/dev/null 2>&1; then ok "backend healthy on :8080"; break; fi
  kill -0 "$backend_pid" 2>/dev/null || die "the backend exited before becoming healthy (see the log above)"
  [[ "$i" -eq 180 ]] && die "backend didn't become healthy in time"
  sleep 1
done

printf '\n%sOpen %shttp://localhost:5173%s%s — sign up, then connect a brokerage.%s\n\n' \
  "$bold" "$blue" "$rst" "$bold" "$rst"
log "Starting the frontend (Vite :5173). Press Ctrl-C to stop both."
( cd frontend && npm run dev )
