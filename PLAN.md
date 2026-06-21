# TaxLossApp — Implementation Plan

A personal investment tracker built to **learn Rust** on the backend, designed to be
productized later. This file is the source of truth for scope and direction — edit it freely.

> Status: **M1 complete** — Cargo workspace + Postgres schema (all 8 tables) +
> `/health` + auto-migrations on boot. Everything past this is intentional scaffold:
> the Plaid client has no endpoints yet, `domain` holds only `FilingStatus`, and `db`
> has row models but no query layer. Those land in M2–M5. Post-M1 review decisions are
> recorded in §8.

---

## 1. What we're building & why

Track investments with these capabilities:

- **Portfolio tracking** — holdings, cost basis, current value, realized/unrealized gains
- **Performance & analytics** — returns over time, allocation breakdowns
- **Tax-loss harvesting** — find positions at a loss to sell, with wash-sale awareness
- **Dividends & income** tracking
- **Tax-aware sell alerts** (headline feature) — notify when it's a good time to sell a
  position, factoring in short- vs long-term capital-gains tax

---

## 2. Architecture

```
React frontend ──HTTP/JSON──► Axum backend ──SQLx──► Postgres
   Plaid Link UI                  │ reqwest ──► Plaid API (holdings, txns, webhooks)
                                  ├─► tax engine (pure Rust)
                                  └─► alert engine ──► email
```

**Cargo workspace** with 4 crates (the separation is itself a Rust learning win):

| Crate | Responsibility | Why separate |
|---|---|---|
| `domain` | Pure logic: tax math, lot reconstruction, alert rules | No DB/HTTP → trivially unit-testable |
| `db` | SQLx queries, models, migrations | Isolates data access |
| `plaid` | Plaid REST client | Isolates the external API |
| `api` | Axum web server wiring it all together | The deployable binary |

### Module structure (single-responsibility, future-proof)

The crate boundary is the first layer of separation — the compiler enforces that
`domain` can't reach for the DB or HTTP. Within each crate, **one module per
responsibility**, so adding a feature means adding a module/file rather than editing
existing logic (open/closed):

```
crates/
  domain/src/   lib.rs · tax (M4) · lots (M3) · alerts (M5)        ← pure functions, one concern each
  db/src/       lib.rs (connect/migrate) · models.rs · queries per entity (added per milestone)
  plaid/src/    lib.rs (client/env) · endpoint modules: link, holdings, transactions, webhooks (M2)
  api/src/      main · config · error · state · routes/{health, plaid, portfolio, tax, alerts}
```

How this stays easy to extend:
- **New feature = new module + new route file**, then merged into the router in
  `routes/mod.rs` — existing handlers are untouched (the M1 scaffold already does this
  for `health`).
- **`domain` holds zero I/O**, so tax/lot/alert rules are pure functions that are unit-
  tested in isolation and reused by any caller (API now, a CLI or batch job later).
- **Data access is funnelled through `db`** (typed `FromRow` models + per-entity query
  modules), so a schema or query change has one home.
- **External APIs are quarantined** in `plaid` behind a thin client, so swapping or
  mocking the provider doesn't ripple outward.

### Stack
- **Backend:** Rust — Axum 0.8, Tokio, SQLx 0.8 (never floats for money via rust_decimal),
  reqwest (Plaid). `lettre` (email) and `tokio-cron-scheduler` (jobs) are **added at M5** —
  not yet in `Cargo.toml`.
- **DB:** PostgreSQL 16 (via Docker Compose locally)
- **Frontend:** React + TypeScript + Vite + TanStack Query + Recharts
- **Data source:** Plaid Investments API

---

## 3. Data model (Postgres)

8 tables: `users`, `plaid_items`, `accounts`, `securities`, `holdings`, `transactions`,
`tax_lots`, `alerts`. Two key decisions:

- **Every user-owned table carries `user_id` now**, even though v1 is single-user — so going
  multi-user later is an auth change, not a schema rewrite.
- **`tax_lots` are derived, not from Plaid.** Plaid doesn't provide per-purchase tax lots, so we
  reconstruct them from the transaction feed. This powers holding-period and gain/loss math.
  Reconstruction is **FIFO** (matches how brokers report realized gains); the harvest/sell
  simulator then lets you pick **specific lots** (see §4).

Each table has a matching `FromRow` model in `crates/db/src/models.rs` (including
`PlaidItem`); query methods are added per milestone as endpoints need them.

---

## 4. Tax engine (`crates/domain`)

Pure Rust. Target is heavy unit-test coverage at M3/M4 (today: a single `FilingStatus`
serde test — the math below is not built yet).

- **Holding period:** > 365 days = long-term, else short-term
- **Federal rates:** LT brackets 0/15/20% by filing status + income; ST at ordinary rate;
  + NIIT 3.8%.
- **California rates:** CA has **no preferential capital-gains rate** — gains are taxed as
  **ordinary income** at CA brackets (top ~13.3%), regardless of holding period.
- Federal *and* CA brackets are stored as **year-keyed data** so both update annually.
  Estimated tax = federal (LT/ST + NIIT) **+** CA ordinary.
- **Cost basis:** lots are reconstructed **FIFO** for historical realized gains; the
  harvest / "good time to sell" simulator lets you select **specific lots** to model the
  best outcome.
- **Tax-loss harvesting:** flag unrealized losses; detect **wash sales** (same security bought
  within ±30 days)
- **"Good time to sell" signal:** for each lot, compare after-tax proceeds *now* vs. *after it
  crosses the 1-year long-term boundary* — surface lots about to become long-term (selling now
  wastes the lower rate) or gains clearing a threshold

> Scope: **federal + California** in v1; other states deferred (the engine stays
> state-aware so adding them is data, not a rewrite). This is decision-support, **not tax
> advice** — a UI disclaimer should be added.

---

## 5. API interface, onboarding & notifications

REST/JSON over HTTP, all routes under `/api`. v1 is single-user (the user is resolved
server-side); real auth arrives with productization. Endpoints land alongside their
milestone:

| Resource | Endpoint | Milestone |
|---|---|---|
| Onboarding | `POST /api/plaid/link-token` → Plaid Link token | M2 |
| | `POST /api/plaid/exchange` → swap public_token, store encrypted item, trigger initial sync | M2 |
| | `POST /api/plaid/webhook` → holdings/transaction updates from Plaid | M2 |
| User | `GET/PATCH /api/profile` → filing status + taxable income (needed for tax math) | M3 |
| Portfolio | `GET /api/accounts`, `GET /api/holdings`, `GET /api/transactions`, `GET /api/lots` | M3 |
| Tax | `GET /api/tax/summary` (realized/unrealized + est. federal+CA tax) | M4 |
| | `GET /api/tax/harvest` (loss candidates + wash-sale flags) | M4 |
| | `POST /api/tax/simulate` (specific-lot sell → after-tax proceeds) | M4 |
| Alerts | `GET /api/alerts`, `POST /api/alerts/:id/read` | M5 |

**Onboarding flow:** land → "Connect your brokerage" (Plaid Link) → frontend gets a
link-token, opens Link, returns a public_token → `exchange` stores the item and pulls
holdings + transactions → lots are reconstructed → user sets filing status + taxable
income → dashboard renders.

**Notifications:** two channels. **In-app** — the `alerts` table, surfaced via
`GET /api/alerts` (badge + list in the UI). **Email** — `lettre` at M5 sends tax-aware
sell / harvest alerts; `alerts.emailed_at` tracks delivery. Push/SMS deferred.

**Backend ↔ UI mapping:** each screen is backed by specific endpoints — dashboard →
`holdings` + `tax/summary`; harvest → `tax/harvest` + `tax/simulate`; alerts →
`alerts`; onboarding → `plaid/*`. The React app talks only to `/api` (TanStack Query),
so the UI never depends on Plaid or DB shapes directly.

> **UI/UX design happens before the frontend is built** — see the design milestone in §6.

---

## 6. Milestones (each teaches specific Rust)

| | Milestone | Rust concepts | Status |
|---|---|---|---|
| M1 | Skeleton + DB | cargo, modules, async/Tokio, Result/`?` | ✅ done |
| M2 | Plaid integration | serde, traits, error propagation | next |
| M3 | Portfolio + lots API | iterators, ownership, decimal math | |
| M4 | Tax engine | pure functions, testing, enums/pattern matching | |
| M5 | Alerts + scheduler + email | background tasks, shared state | |
| M6 | UI/UX design (wireframes + Figma mocks) | (design, not Rust) — onboarding, dashboard, harvest, alerts; validates flows before coding | |
| M7 | React frontend | (TS/React, not Rust) | |
| M8 | Productization | auth, multi-user, secrets, deploy | |

> M6 is a design gate: mock the onboarding, dashboard, harvest, and alerts screens (and
> map each to the §5 endpoints) before building the React app in M7, to avoid rework.

---

## 7. Verification per milestone

End-to-end checks, not just "it compiles". **CI** (`.github/workflows/ci.yml`) runs
`fmt` + `clippy` + `build` + `test` on every push/PR — no Postgres needed since queries
are runtime strings, not the `sqlx::query!` macro.

- **M1:** `docker compose up -d`, run `api`, `curl /health` → 200, migrations applied ✅
- **M2:** Plaid **Sandbox** connect flow; holdings + transactions land in Postgres; webhook updates data
- **M3/M4:** `cargo test -p domain` with hand-checked fixtures (e.g. lot held 364 vs 366 days →
  ST vs LT; buy within 30 days → wash-sale flag); endpoint outputs vs manual calcs
- **M5:** trigger scheduler job → `alerts` row created + test email delivered (Mailtrap)
- **M6:** mocks reviewed for all core screens; each screen mapped to its §5 endpoints
- **M7:** `npm run dev`; connect via Plaid sandbox; dashboard/charts/harvest/alerts render real data

---

## 8. Decisions (post-M1 review) & assumptions

Resolved at the post-M1 review:
1. **Cost basis:** FIFO to reconstruct historical realized gains, **plus specific-lot
   selection** in the harvest / sell simulator. Average cost deferred.
2. **Tax scope:** **Federal + California** in v1 (CA gains taxed as ordinary income).
   Other states deferred — engine stays state-aware so adding them is data, not a rewrite.
3. **Pricing:** **End-of-day** prices from Plaid are sufficient for tax-timing alerts.
   Intraday market-data deferred.
4. **Milestone order:** a **UI/UX design gate (M6)** now precedes the React build (M7) —
   Figma/wireframe mocks for the core screens before coding the frontend.
5. **API surface & notifications:** REST/JSON under `/api` (see §5); user notifications
   via in-app alerts **and** email (`lettre`, M5).

Baked-in assumptions:
- Plaid Investments covers **US + Canada** institutions only
- Need a free **Plaid sandbox** account (client_id + secret) before testing M2; SMTP creds
  (Mailtrap) before M5

---

## 9. Dev setup quick reference

- Rust installed via rustup at `~/.cargo` (source `~/.cargo/env` per shell)
- `docker compose up -d` starts Postgres (creds `taxloss`/`taxloss`, db `taxloss`)
- `cargo run -p api` (or `./target/debug/api`) — migrations auto-run on startup
- `.env` is gitignored; `.env.example` documents all vars
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace` mirror what CI enforces — run them before pushing
