# Quick start — run Squirrel locally 🐿️

Get the app running on your machine in one command. For the full feature tour and
architecture, see [`README.md`](README.md).

**Supported platforms:** macOS and Linux (`apt` / `dnf` / `pacman` / `zypper`) run
the scripts natively. On **Windows**, use **WSL2** (recommended — then follow the
Linux path) or **Git Bash** with [winget](https://learn.microsoft.com/windows/package-manager/winget/)
installed; `setup.sh` detects all of these.

> ⚠️ **Not tax advice.** Every number is a decision-support estimate. Tax scope is
> federal + California in v1.

## 1. Prerequisites

Install these first (the script checks for them and links to each if missing):

| Tool | Why | Install |
|---|---|---|
| **Rust** (cargo) | builds + runs the backend | <https://rustup.rs> |
| **Node 20+** (node, npm) | builds + runs the frontend | <https://nodejs.org> or `nvm` |
| **Docker** | runs Postgres locally | <https://www.docker.com/products/docker-desktop> |
| **openssl** | generates a local encryption key | usually preinstalled |

Make sure **Docker Desktop is running** before you start.

> **Don't have these?** After cloning, run **`./setup.sh`** — it checks each tool
> and offers to install whatever's missing (Rust via rustup, Node/openssl via your
> package manager, Docker via Homebrew cask / the official Linux script). Use
> `./setup.sh --check` to just see what's missing, or `--yes` to install
> unattended.

## 2. Get the code and run it

```bash
git clone https://github.com/karthikrao-23/Squirrel.git
cd Squirrel
./setup.sh     # optional: check + install any missing prerequisites
./run.sh
```

`./run.sh` does everything:

1. checks your prerequisites,
2. creates a local `.env` (and generates an encryption key),
3. starts **Postgres** in Docker,
4. installs the frontend dependencies,
5. applies migrations and provisions a non-superuser DB role,
6. starts the **backend** (`:8080`) and **frontend** (`:5173`).

> Step 5 makes local dev mirror production: the app runs as a **non-superuser**
> role so Postgres **Row-Level Security** (tenant isolation) actually enforces —
> connecting as the default superuser owner would silently bypass it. `.env`'s
> `DATABASE_URL` stays the owner connection, used only to run migrations.

When it's up, open **<http://localhost:5173>**, **sign up**, then connect a brokerage.

Stop everything with **Ctrl-C**. Postgres keeps running in the background; stop it
with `docker compose down`.

## 3. Connecting a brokerage (Plaid)

Squirrel imports holdings and transactions through [Plaid](https://plaid.com).
Plaid has two environments, and which one you use decides whether you're working
with fake or real accounts:

| | **Sandbox** (recommended to start) | **Production** (real accounts) |
|---|---|---|
| What connects | Plaid's fake test banks | your actual brokerage |
| Login in Plaid Link | `user_good` / `pass_good` | your real bank credentials |
| Cost / access | free, instant | needs Plaid **production access** (a trial grants this) |
| `.env` setting | `PLAID_ENV=sandbox` | `PLAID_ENV=production` |

> **The key gotcha:** your **`client_id` is the same** in both environments, but the
> **`secret` is different** — the Plaid dashboard gives you a separate *Sandbox*
> secret and *Production* secret. `PLAID_SECRET` must match `PLAID_ENV`, or Plaid
> rejects the calls.

Get both at <https://dashboard.plaid.com> → **Developers → Keys**, then:

**Sandbox** (try it with fake data):
```bash
PLAID_ENV=sandbox
PLAID_CLIENT_ID=your_client_id
PLAID_SECRET=your_sandbox_secret
```
Restart with `./run.sh`, click **Connect a brokerage**, pick any institution, and
log in with **`user_good` / `pass_good`**. (In dev there's also a one-click
**"Dev: sandbox shortcut"** button that skips the Plaid Link UI.)

**Production / Plaid trial** (connect your real brokerage — works locally too):
```bash
PLAID_ENV=production
PLAID_CLIENT_ID=your_client_id
PLAID_SECRET=your_production_secret
```
Restart with `./run.sh` and connect your actual account through Plaid Link. Leave
`APP_ENV` unset (development) — that keeps local cookies working; you're only
switching *Plaid*, not the app's security posture.

The app runs fine without Plaid keys — you just can't import a portfolio until you
add them.

## Useful commands

```bash
./run.sh --setup     # set everything up but don't start the servers
docker compose down  # stop Postgres
docker compose down -v   # stop Postgres AND delete its data (fresh start)
```

Prefer to run the pieces yourself? See the **Getting started** section in
[`README.md`](README.md) for the individual `docker compose` / `cargo run` /
`npm run dev` steps.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `the Docker daemon isn't running` | Start Docker Desktop, then re-run `./run.sh`. |
| Backend exits with a `DATABASE_URL` error | Postgres isn't up yet — re-run `./run.sh` (it waits for the DB). |
| Login/connect feels stuck the first time | The initial `cargo` build can take a minute or two; watch the log for `backend healthy on :8080`. |
| Port `5432` / `8080` / `5173` already in use | Stop whatever is using it (e.g. another Postgres), or `docker compose down`. |
| Want a clean database | `docker compose down -v` then `./run.sh` (migrations re-run on startup). |
