# TaxLossApp — Implementation Plan

A personal investment tracker built to **learn Rust** on the backend, designed to be
productized later. This file is the source of truth for scope and direction — edit it freely.

> Status: **M1 complete** (backend skeleton + DB). Plan review done — **M2 (Plaid
> integration) is the active next step.** See §8 for the decisions that came out of review.

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

### Stack
- **Backend:** Rust — Axum 0.8, Tokio, SQLx 0.8 (compile-time-checked queries), rust_decimal
  (never floats for money), reqwest (Plaid), lettre (email), tokio-cron-scheduler (jobs)
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
  reconstruct them from the transaction feed. Lots are depleted **FIFO** when replaying
  *historical* sells (to match broker records), but each **open** lot is **individually
  selectable** for harvesting/sell simulation (see §4). This powers holding-period and
  gain/loss math.

---

## 4. Tax engine (`crates/domain`)

Pure Rust, heavily unit-tested:

- **Holding period:** > 365 days = long-term, else short-term
- **Federal rates:** LT brackets 0/15/20% by filing status + income; ST at ordinary rate;
  + NIIT 3.8%. Brackets stored as year-keyed data so they're easy to update annually.
- **Tax-loss harvesting:** flag unrealized losses; detect **wash sales** (same security bought
  within ±30 days). Operates on **user-selected specific lots** among the open lots for a
  security (FIFO is only the historical-reconstruction default — see §3, §8).
- **"Good time to sell" signal:** for each selected lot, compare after-tax proceeds *now* vs.
  *after it crosses the 1-year long-term boundary*, using **end-of-day** prices — surface lots
  about to become long-term (selling now wastes the lower rate) or gains clearing a threshold

> Scope decision: **federal only, no state tax in v1** (state tax is a future layer). This is
> decision-support, **not tax advice** — a UI disclaimer should be added.

---

## 5. Security

Handling brokerage data and Plaid access tokens, so security is a first-class concern even
while single-user. **Bold** = already in place (M1); the rest is designed and scheduled.

**In place now (M1):**
- **Plaid access tokens encrypted at rest.** Stored as `plaid_items.access_token_encrypted`
  (`BYTEA`) — never the raw token. AES-GCM (authenticated encryption) with a key from
  `TOKEN_ENCRYPTION_KEY`; crypto deps (`aes-gcm`, `base64`, `rand`) are already in the workspace.
- **Secrets via env, never committed.** `.env` / `*.local` are gitignored; only `.env.example`
  (empty values) is tracked. Config **fails fast** if `DATABASE_URL` is missing.
- **No SQL injection by construction.** SQLx compile-time-checked, parameterized queries — no
  string-built SQL.
- **TLS everywhere.** `reqwest` and `sqlx` both pinned to `rustls` (Plaid API + DB over TLS).
- **Tenant isolation designed in.** Every user-owned table carries `user_id` with
  `ON DELETE CASCADE`, so per-row ownership scoping (and clean "delete my data") works without a
  schema rewrite when auth lands.
- **Sandbox-by-default for Plaid.** `PlaidEnv` falls back to Sandbox on any unrecognized value —
  no accidental production access from a misconfigured env.
- **Money is `NUMERIC` / `rust_decimal`,** never floats — integrity of tax/gain math.

**Scheduled with the relevant milestone:**
- **M2:** wire `TOKEN_ENCRYPTION_KEY` into `Config` + an encrypt/decrypt helper; **verify Plaid
  webhook signatures**; ensure access tokens and webhook `payload` JSON are never logged.
- **M6:** lock down **CORS** (tower-http cors is available) to the frontend origin.
- **M7 (productization):** real **authentication + authorization**, per-user row scoping enforced
  at query time, secrets manager instead of `.env`, and a hardened deploy. The full
  localhost→production runbook lives in **`DEPLOYMENT.md`**.

**Known gaps to decide on before any real (non-sandbox) data:**
- `BIND_ADDR` defaults to `0.0.0.0:8080` with no auth in front — fine for local Docker, not for
  exposed deploys.
- No rate limiting or request-size limits on the Axum layer yet.
- Plaid creds are optional at startup (convenient for M1) — add a production-env guard.

## 6. Milestones (each teaches specific Rust)

| | Milestone | Rust concepts | Status |
|---|---|---|---|
| M1 | Skeleton + DB | cargo, modules, async/Tokio, Result/`?` | ✅ done |
| M2 | Plaid integration | serde, traits, error propagation | next |
| M3 | Portfolio + lots API | iterators, ownership, decimal math | |
| M4 | Tax engine | pure functions, testing, enums/pattern matching | |
| M5 | Alerts + scheduler + email | background tasks, shared state | |
| M6 | React frontend | (TS/React, not Rust) | |
| M7 | Productization | auth, multi-user, secrets, deploy | |

---

## 7. Verification per milestone

End-to-end checks, not just "it compiles":

- **M1:** `docker compose up -d`, run `api`, `curl /health` → 200, migrations applied ✅
- **M2:** Plaid **Sandbox** connect flow; holdings + transactions land in Postgres; webhook updates data
- **M3/M4:** `cargo test -p domain` with hand-checked fixtures (e.g. lot held 364 vs 366 days →
  ST vs LT; buy within 30 days → wash-sale flag); endpoint outputs vs manual calcs
- **M5:** trigger scheduler job → `alerts` row created + test email delivered (Mailtrap)
- **M6:** `npm run dev`; connect via Plaid sandbox; dashboard/charts/harvest/alerts render real data

---

## 8. Decisions & assumptions

Decisions from the post-M1 review (these resolve the prior open questions):

1. **Cost basis — two layers.** Historical realized sells are reconstructed by depleting lots
   **FIFO** (Plaid's feed doesn't say which lots the broker actually used, so FIFO is the
   matching anchor). For harvesting and sell *simulation*, the user picks **specific open
   lots** to sell, and we compute per-lot gain/loss, holding period, and after-tax proceeds.
   The existing `tax_lots` schema (`remaining_quantity`, `cost_basis_per_share`, `open_date`,
   `status`) already supports this — no migration needed.
2. **Tax scope — federal only** in v1 (LT 0/15/20% + NIIT 3.8%). State capital-gains tax is a
   future layer, not in scope now.
3. **Prices — end-of-day** from Plaid. Day-granularity is sufficient for LT/ST boundary
   alerts; intraday is a later optional add and avoids a second market-data integration now.
4. **Milestones — unchanged** (M2 → M7 as in §6).

Baked-in assumptions:
- Plaid Investments covers **US + Canada** institutions only
- Plaid prices are **end-of-day**, not real-time
- Need a free **Plaid sandbox** account (client_id + secret) before testing M2; SMTP creds
  (Mailtrap) before M5

Still open / future (deliberately deferred):
- State capital-gains tax
- Intraday / real-time prices
- Additional cost-basis methods (e.g. average cost) as a configurable option

---

## 9. Dev setup quick reference

- Rust installed via rustup at `~/.cargo` (source `~/.cargo/env` per shell)
- `docker compose up -d` starts Postgres (creds `taxloss`/`taxloss`, db `taxloss`)
- `cargo run -p api` (or `./target/debug/api`) — migrations auto-run on startup
- `.env` is gitignored; `.env.example` documents all vars
