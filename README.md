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
[![coverage](https://img.shields.io/endpoint?url=https://karthikrao-23.github.io/Squirrel/coverage-badge.json)](https://karthikrao-23.github.io/Squirrel/)

> 📊 **[Coverage report](https://karthikrao-23.github.io/Squirrel/)** — the badge above shows total line coverage; click it for the full per-file `llvm-cov` HTML report (published from CI on every push to `main`).

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
- **Tax-aware sell alerts** (the headline feature) — watches for:
  - lots about to cross the **1-year long-term boundary** (selling now wastes the lower rate), and
  - **harvestable losses** worth realizing.
  Re-evaluated on every data sync (connecting or re-syncing a brokerage) and on demand
  from the Alerts screen's **Refresh** button, on top of a scheduled cycle. Delivered
  **in-app** (badge + list) and by **email**.
- **Per-account view** — holdings broken out by connected account, collapsible and
  drilled down by tax-lot year, with per-account value / cost-basis / unrealized totals.
- **Taxable vs retirement, done right** — accounts are classified from their Plaid
  subtype (IRA / Roth / 401(k) / …). Retirement accounts are **excluded from harvesting
  and the "tax if sold" math** (no tax benefit there) and get a **performance view**
  instead. A misclassified account can be **manually overridden** (Auto / Taxable /
  Retirement) and the correction flows through every view.
- **Retirement performance** — for tax-advantaged accounts: value, total return ($/%),
  money-weighted **IRR** and time-weighted **TWR**, plus a value-over-time chart.
- **Portfolio value over time** — a daily snapshot job records total / taxable /
  retirement value, backing the dashboard's performance chart.
- **Connections manager** — see every Plaid link, spot accidental duplicates, and
  **remove a connection** (disconnects it on Plaid's side and drops its accounts/lots).
- **Holdings-unavailable accounts** — for institutions Plaid won't share positions for
  (e.g. Fidelity BrokerageLink), the account value is **anchored to Plaid's balance**
  rather than dropped.

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

10 tables: `users`, `sessions`, `plaid_items`, `accounts`, `securities`,
`holdings`, `transactions`, `tax_lots`, `alerts`, `portfolio_snapshots`. Three
deliberate decisions:

- **Every user-owned table carries `user_id`**, so the app is **multi-user**: each
  request is scoped to the authenticated user, and Plaid uniques are composite
  (`(user_id, plaid_*_id)`) so tenants can't collide.
- **`tax_lots` are derived**, not from Plaid — reconstructed FIFO from transactions.
- **Account tax classification is derived** from the Plaid subtype, never stored —
  except an optional `accounts.kind_override` when the user corrects it. Daily
  `portfolio_snapshots` are scoped (`total` / `taxable` / `retirement`) to chart each.

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
| Connections | `GET /api/plaid/items` · `DELETE /api/plaid/items/:id` | list connections (+ duplicate detection) · remove one |
| | `POST /api/plaid/resync` | re-pull holdings/transactions now |
| Profile | `GET` / `PATCH /api/profile` | filing status + taxable income (drives the tax math) |
| Portfolio | `GET /api/accounts` · `GET /api/holdings` · `GET /api/transactions` · `GET /api/lots` | read-only views (accounts tagged taxable/retirement) |
| | `GET /api/accounts/lots` | open lots grouped per account (+ balance-only accounts) |
| | `PATCH /api/accounts/:id/kind` | override an account's tax classification (`taxable` / `retirement` / `null` = auto) |
| | `POST /api/lots/rebuild` | re-run FIFO reconstruction |
| | `GET /api/portfolio/history` | daily total-value snapshots (dashboard chart) |
| Retirement | `GET /api/retirement` | performance view: value, total return, IRR, TWR, value history |
| Tax | `GET /api/tax/summary` | unrealized gain + estimated federal/CA tax if sold now |
| | `GET /api/tax/harvest` | loss candidates + wash-sale flags (retirement accounts excluded) |
| | `POST /api/tax/simulate` | specific-lot sell → after-tax proceeds |
| Alerts | `GET /api/alerts` · `POST /api/alerts/:id/read` | list + mark read |
| | `POST /api/alerts/evaluate` | run the alert rules now (Alerts screen's **Refresh** button; also runs automatically after a sync) |

> **Money values** are `rust_decimal::Decimal` serialized **as strings** (the
> `serde-with-str` feature) to avoid float precision loss — clients parse them.

---

## Frontend

A single-page app, each screen backed by the endpoints above:

- **Onboarding** — connect a brokerage (Plaid) → sync → set filing status + income.
- **Dashboard** — value/unrealized/ST-LT/estimated-tax tiles, value-over-time chart,
  holdings table, allocation donut.
- **Accounts** — holdings per connected account (collapsible, drilled down by lot year)
  with a per-account **tax-type control** (Auto / Taxable / Retirement); plus the
  **Connections** manager for removing links.
- **Retirement** — performance view of tax-advantaged accounts: total return, IRR/TWR,
  value chart.
- **Harvest** — loss candidates (with wash-sale chips), search/sort/filter → live sell simulator.
- **Alerts** — tax-timing & harvest signals, unread filter, mark-read, and a **Refresh**
  button to re-evaluate on demand (they also refresh automatically after a sync).

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
- A free [Plaid](https://dashboard.plaid.com) account for the connect flow. Start in
  **sandbox** (fake data, log in with `user_good` / `pass_good`); switch to
  **production** with a Plaid trial to connect a real brokerage. Your `client_id` is
  the same in both; the **secret differs per environment** and must match `PLAID_ENV`.
  See [`QUICKSTART.md`](QUICKSTART.md#3-connecting-a-brokerage-plaid) for the details.
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
isolation, Plaid webhook signature verification, container + Cloud Run / Cloud SQL
deploy scripts, a per-account **Accounts** page + **Connections** manager, daily
**portfolio value snapshots** with a dashboard performance chart, a **Retirement**
performance view (total return + IRR/TWR), and **taxable/retirement classification**
with a manual per-account override.

**Known follow-ups:** realized-gains tracking (v1 reconstructs open lots only),
cross-account wash-sale detection (a replacement buy in an IRA can disallow a loss
in a taxable account — currently out of scope), and contribution/withdrawal
classification to sharpen the retirement IRR/TWR figures.

---

*Decision-support, not tax advice.*
