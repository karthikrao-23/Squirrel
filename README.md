# Squirrel 🐿️

**Squirrel away more of your gains.**

Squirrel is a personal investment tracker with a tax-aware brain. It pulls your
brokerage holdings and transactions, reconstructs your tax lots, and tells you
**when it's a good time to sell** — factoring in short- vs long-term capital-gains
tax, tax-loss-harvesting opportunities, and wash-sale risk.

It's built primarily to learn **backend Rust**, with an eye toward eventually
productizing it.

> ⚠️ **Not tax advice.** Every number is a decision-support estimate. Tax scope is
> **federal + California** in v1. Consult a professional before trading.

[![CI](https://github.com/karthikrao-23/Squirrel/actions/workflows/ci.yml/badge.svg)](https://github.com/karthikrao-23/Squirrel/actions/workflows/ci.yml)

---

## What it does

- **Portfolio tracking** — accounts, holdings, cost basis, current value, unrealized gain/loss.
- **Tax lots, reconstructed** — Plaid doesn't expose per-purchase lots, so Squirrel
  rebuilds them **FIFO** from the transaction feed. This powers holding-period and
  gain/loss math.
- **Tax engine** — estimates the tax (or saving) of realizing a gain/loss:
  - **Federal** long-term brackets (0 / 15 / 20%) by filing status + income; short-term at ordinary rates.
  - **NIIT** 3.8% surtax above the MAGI threshold.
  - **California** taxes all gains as ordinary income (no preferential rate).
- **Tax-loss harvesting** — surfaces lots trading below cost, flags **wash-sale risk**
  (same security bought within ±30 days), and runs a **specific-lot sell simulator**
  showing after-tax proceeds.
- **Tax-aware sell alerts** (the headline feature) — a nightly job watches for:
  - lots about to cross the **1-year long-term boundary** (selling now wastes the lower rate), and
  - **harvestable losses** worth realizing.
  Delivered **in-app** (badge + list) and by **email**.

---

## Architecture

```
React frontend ──HTTP/JSON──► Axum backend ──SQLx──► Postgres
  TanStack Query                 │ reqwest ──► Plaid API (holdings, txns, webhooks)
  Recharts                       ├─► tax engine (pure Rust)
                                 └─► alert engine ──► email (lettre) + cron scheduler
```

The backend is a **Cargo workspace** of four crates — the boundary is compiler-enforced,
so pure logic can never reach for the DB or network:

| Crate | Responsibility | Why separate |
|---|---|---|
| `domain` | Pure logic: tax math, FIFO lot reconstruction, alert rules | No I/O → trivially unit-testable (96–100% covered) |
| `db` | SQLx queries, `FromRow` models, migrations | One home for data access |
| `plaid` | Plaid REST client | Quarantines the external API |
| `api` | Axum server wiring it together (the deployable binary) | Split into `lib` + `bin` so tests drive the real router in-process |

The React app talks **only** to `/api`, so the UI never depends on Plaid or database
shapes directly.

### Data model (Postgres)

9 tables: `users`, `sessions`, `plaid_items`, `accounts`, `securities`,
`holdings`, `transactions`, `tax_lots`, `alerts`. Two deliberate decisions:

- **Every user-owned table carries `user_id`**, so the app is **multi-user**: each
  request is scoped to the authenticated user, and Plaid uniques are composite
  (`(user_id, plaid_*_id)`) so tenants can't collide.
- **`tax_lots` are derived**, not from Plaid — reconstructed FIFO from transactions.

---

## API reference

REST/JSON, all under `/api`. Requests are authenticated with an opaque,
`HttpOnly`, `SameSite=Strict` session cookie; mutating routes additionally require
a CSRF header + Origin check. Every data route is scoped to the signed-in user.

| Area | Endpoint | Purpose |
|---|---|---|
| Health | `GET /health` | liveness + DB/Plaid status |
| Auth | `POST /api/auth/signup` · `POST /api/auth/login` | create account / sign in (sets session cookie) |
| | `POST /api/auth/logout` · `POST /api/auth/logout-all` · `GET /api/auth/me` | end session(s) · current user |
| Onboarding | `POST /api/plaid/link-token` | mint a Plaid Link token |
| | `POST /api/plaid/exchange` | swap `public_token`, store encrypted item, initial sync |
| | `POST /api/plaid/sandbox/connect` | dev shortcut: mint + exchange + sync (sandbox) |
| | `POST /api/plaid/webhook` | Plaid pushes holdings/transaction updates |
| Profile | `GET` / `PATCH /api/profile` | filing status + taxable income (drives the tax math) |
| Portfolio | `GET /api/accounts` · `GET /api/holdings` · `GET /api/transactions` · `GET /api/lots` | read-only views |
| | `POST /api/lots/rebuild` | re-run FIFO reconstruction |
| Tax | `GET /api/tax/summary` | unrealized gain + estimated federal/CA tax if sold now |
| | `GET /api/tax/harvest` | loss candidates + wash-sale flags |
| | `POST /api/tax/simulate` | specific-lot sell → after-tax proceeds |
| Alerts | `GET /api/alerts` · `POST /api/alerts/:id/read` | list + mark read |
| | `POST /api/alerts/evaluate` | run the alert rules now (test hook) |

> **Money values** are `rust_decimal::Decimal` serialized **as strings** (the
> `serde-with-str` feature) to avoid float precision loss — clients parse them.

---

## Frontend

A single-page app that implements four screens, each backed by the endpoints above:

- **Onboarding** — connect a brokerage (Plaid) → sync → set filing status + income.
- **Dashboard** — value/unrealized/ST-LT/estimated-tax tiles, holdings table, allocation donut.
- **Harvest** — loss candidates (with wash-sale chips) → live sell simulator.
- **Alerts** — tax-timing & harvest signals, unread filter, mark-read.

The design was prototyped first as HTML mocks (`design/` + `DESIGN.md`) on
[Claude Design](https://claude.ai/design), then built in React. The design tokens in
`frontend/src/styles.css` mirror those mocks.

---

## Tech stack

**Backend (Rust)** — Axum 0.8 · Tokio · SQLx 0.8 (Postgres, runtime-checked queries) ·
`rust_decimal` (never floats for money) · `reqwest` (Plaid) · `lettre` (email) ·
`tokio-cron-scheduler` (jobs) · `anyhow` / `thiserror` · `tracing`.

**Database** — PostgreSQL 16 (Docker Compose locally).

**Frontend** — React 18 · TypeScript · Vite 5 · TanStack Query 5 · Recharts ·
React Router 6.

**Data source** — Plaid Investments API (US + Canada institutions).

**Tooling** — GitHub Actions CI (`fmt` + `clippy` + `build` + `test` with a Postgres
service; `tsc --noEmit` + `vite build` for the frontend) · `cargo-llvm-cov` coverage.

---

## Getting started

### Quick start (one command)

```bash
git clone https://github.com/karthikrao-23/Squirrel.git
cd Squirrel
./setup.sh     # checks + installs missing prerequisites (Rust, Node, Docker, openssl)
./run.sh       # creates .env, starts Postgres, installs frontend deps, runs both servers
```

Then open **http://localhost:5173**, **sign up**, and connect a brokerage. Stop
with **Ctrl-C** (Postgres keeps running; `docker compose down` stops it).

- Runs natively on **macOS** and **Linux** (`apt`/`dnf`/`pacman`/`zypper`). On
  **Windows**, use **WSL2** (recommended) or **Git Bash + winget**.
- `./setup.sh --check` reports what's missing without installing; `--yes` installs
  unattended. `./run.sh --setup` prepares everything but doesn't start the servers.
- Full walkthrough, the Plaid-sandbox step, and troubleshooting:
  [`QUICKSTART.md`](QUICKSTART.md).

### Prerequisites

`./setup.sh` installs these for you; listed here if you'd rather do it yourself:

- Rust (via [rustup](https://rustup.rs)) · Node 20+ · Docker (for Postgres) · openssl
- A free [Plaid sandbox](https://dashboard.plaid.com) account (client_id + secret)
  for the connect flow — add `PLAID_CLIENT_ID` / `PLAID_SECRET` to `.env`
- *(optional)* SMTP creds (e.g. [Mailtrap](https://mailtrap.io)) to actually send alert emails

### Manual setup

Equivalent to what `./run.sh` does, broken out.

**Backend**
```bash
docker compose up -d                 # Postgres (taxloss/taxloss, db taxloss)
cp .env.example .env                 # add PLAID_*; generate a key:
                                     #   openssl rand -base64 32  → TOKEN_ENCRYPTION_KEY
cargo run -p api                     # migrations auto-run on startup; serves :8080
```

**Frontend**
```bash
cd frontend
npm install
npm run dev                          # Vite :5173, proxies /api → :8080
```

Open http://localhost:5173, **sign up**, then **Connect a brokerage** (Plaid Link).
In sandbox, pick any institution and log in with **`user_good` / `pass_good`**.

### Checks (mirror CI)
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace               # needs DATABASE_URL set (Postgres up)
cd frontend && npm run build         # tsc --noEmit + vite build
bash -n setup.sh && bash -n run.sh   # shell scripts (also linted in CI on macOS + Ubuntu)
```

---

## Project status

| Milestone | | Status |
|---|---|---|
| M1 | Skeleton + Postgres schema + migrations | ✅ |
| M2 | Plaid integration (sandbox-verified) | ✅ |
| M3 | Portfolio + FIFO lot reconstruction API | ✅ |
| M4 | Tax engine (federal + CA, NIIT) | ✅ |
| M5 | Alerts + cron scheduler + email | ✅ |
| M6 | UI/UX design gate (mocks + endpoint map) | ✅ |
| M7 | React frontend | ✅ |
| M8 | Productization: auth, multi-user, secrets, deploy | ✅ |

**Done since:** real Plaid Link onboarding, DB-backed auth + per-user tenant
isolation, Plaid webhook signature verification, and container + Cloud Run /
Cloud SQL deploy scripts.

**Known follow-ups:** a portfolio value-history endpoint to back the dashboard's
performance chart, and realized-gains tracking (v1 reconstructs open lots only).

---

*Decision-support, not tax advice.*
