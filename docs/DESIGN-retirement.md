# Design: Retirement accounts — performance, not tax

> Status: **shipped.** This started as a proposal; the sections below are updated
> to describe what was actually built. Two calls came out differently from the
> original recommendation — a **dedicated Retirement tab** (not folded into the
> Accounts page) and **IRR/TWR shipped in v1** (not deferred) — both noted inline
> and summarized under [Decisions, as built](#decisions-as-built).

## The problem

Squirrel is built around **tax-aware** decisions: lots, holding periods, ST/LT
splits, loss harvesting, wash sales. That framing is correct for **taxable**
brokerage accounts — but it's **meaningless for retirement accounts** (Traditional
/ Roth IRA, 401(k), 403(b), etc.). Inside a tax-advantaged account:

- Buying/selling is **not a taxable event** → harvesting a loss yields nothing.
- The **short-term vs long-term** distinction doesn't exist.
- "Estimated tax if sold now" is **$0** and misleading to show.

For these accounts the user wants the opposite emphasis: **how is it performing?**

## Classification (taxable vs retirement)

Plaid gives every account a `type` + `subtype`. We classify each into an
`AccountKind`:

| Kind | Plaid `subtype`s (examples) |
|---|---|
| **Retirement** | `ira`, `roth`, `401k`, `roth 401k`, `403b`, `457b`, `sep ira`, `simple ira`, `rollover ira`, `pension`, `keogh`, `tsp`, `401a` |
| **Taxable** | `brokerage`, `stock plan` (ESPP/RSU), `cash management`, `mutual fund`, anything else investment |
| **Excluded** | non-investment: `loan`, `credit card`, `checking`, `savings`, … (already not part of investment views) |

Implementation: a small pure `AccountKind::from_subtype(&str)` helper (a `match`
on a lowercased subtype, default `Taxable` for unknown investment accounts). It's
**derived, not stored** — one unit-tested function. Surface it as a `kind` field
on the `/api/accounts` response and on each lot/holding row.

> **Shipped addition — manual override.** Plaid's subtype is sometimes wrong or
> missing (e.g. a 401(k) that arrives as a generic "brokerage"), so a user can
> correct a single account via `PATCH /api/accounts/{id}/kind`
> (`taxable` / `retirement` / `null` = auto). This is the **one** stored piece:
> a nullable `accounts.kind_override` column (migration `0006`). Reads resolve it
> with `AccountKind::resolve(subtype, override)` — the override wins, else fall
> back to `from_subtype`. The Accounts-page cards expose an Auto/Taxable/Retirement
> control, and the correction flows into harvest exclusion, the retirement view,
> the dashboard split, and alerts.

> **Caveat to document, not solve in v1:** wash sales technically *do* cross
> account boundaries (a replacement buy in an IRA within ±30 days can disallow a
> loss in a taxable account). That's an edge case; v1 simply excludes retirement
> accounts from harvest candidates and notes this for later.

## What changes per kind

**Taxable** — unchanged: lots, harvest candidates, ST/LT, tax-if-sold, wash-sale.

**Retirement** — suppress the tax framing, show performance:
- **Excluded from** `/api/tax/harvest` (no tax benefit) and from the "Est. tax if
  sold" figure. The harvest handler builds the set of retirement account ids
  (resolving each account's kind) and skips any lot held in one.
- **Performance surfaced instead:** current value, total cost basis, **total
  return ($ and %)**, and **value-over-time**.

## Performance metrics — what's feasible with our data

We have holdings (value + cost basis), reconciled tax lots, transactions, and
daily **value snapshots**.

| Metric | Definition | Feasible now? |
|---|---|---|
| Current value | Σ holding market value | ✅ |
| Total return ($) | value − cost basis | ✅ |
| Total return (%) | (value − cost basis) / cost basis | ✅ |
| Value over time | snapshot history (retirement scope) | ✅ |
| **Money-weighted return (IRR)** | return weighting contribution size/timing | ✅ (from lot cash-flows via `xirr`) |
| **Time-weighted return (TWR)** | return independent of contribution timing | ✅ (from the daily snapshot series; null until ≥2 points) |

**How:** total return **plus** IRR and TWR. The original plan deferred
TWR/IRR to v2 for lack of contribution data — but we get a usable **money-weighted
IRR** by treating each lot's cost basis as an outflow on its acquisition date and
current value as the terminal inflow, and a **time-weighted TWR** straight from the
retirement value snapshots. Both are computed in `domain::performance`. The value
chart is backed by **scope-tagged** snapshots (`total` / `taxable` / `retirement`),
not per-account rows (see [Decisions](#decisions-as-built)).

> Note: simple "total return %" reflects unrealized return on *current* holdings,
> not the account's lifetime return; IRR/TWR capture trajectory better. The
> retirement view leads with return but the numbers are labeled precisely. Lots
> with no cost basis (balance-only accounts) count toward value but are excluded
> from the return metrics (`return_excludes`).

## UI (as built)

The original plan was to fold retirement into the Accounts page. It shipped as a
**dedicated Retirement tab** instead — the tax view and the performance view want
different layouts (return %/IRR/TWR + a chart vs. lot tables), and a separate
aggregate screen reads more clearly than kind-switched cards. So:

- **Retirement tab** — an **aggregate performance** view across all retirement
  accounts: total value, total return ($/%), IRR, TWR, a value-over-time chart, and
  a per-account breakdown. Backed by `GET /api/retirement`.
- **Accounts page** — lists **every** account (taxable and retirement) with a
  **kind chip** and the **Auto/Taxable/Retirement override** control. This is where
  a misclassification gets fixed.
- **Dashboard** — a **taxable vs retirement** value split, and retirement is
  excluded from the "Est. tax if sold now" tile.
- **Harvest tab** — retirement lots simply never appear (excluded server-side).

## What shipped

Mostly additive, built on the Accounts page and the snapshots feature:

1. ✅ **`AccountKind::from_subtype`** pure helper in `crates/domain` + unit tests,
   plus `resolve(subtype, override)` for the manual override.
2. ✅ **API:** `kind` on the accounts/lots responses (derived), and
   `PATCH /api/accounts/{id}/kind` to override it (migration `0006`).
3. ✅ **Harvest:** retirement-account lots filtered out of the harvest handler.
4. ✅ **Snapshots:** recorded per **scope** (`total` / `taxable` / `retirement`)
   via migration `0004`, not per `account_id` — see below.
5. ✅ **Retirement tab** (`GET /api/retirement`): aggregate value, total return,
   IRR, TWR, and value history; dashboard taxable/retirement split.
6. **Deferred (v2):** contribution/withdrawal tracking to sharpen IRR/TWR;
   cross-account wash-sale logic.

## Decisions (as built)

The three open questions from the proposal, and how they were resolved:

1. **Scope for v1 — did we do IRR/TWR?** Yes. We shipped total return **and** IRR
   (money-weighted, from lot cash-flows) **and** TWR (time-weighted, from the daily
   retirement snapshots), rather than deferring them. No contribution data was
   needed for the approximations we use.
2. **Per-account value chart / snapshot shape?** We chose **scope-tagged** snapshots
   (`total` / `taxable` / `retirement`) over adding `account_id`. The Retirement tab
   charts the retirement scope as a whole; per-account charts weren't needed for v1.
3. **Where to show it — Accounts page or a dedicated tab?** A **dedicated Retirement
   tab** (the proposal had leaned toward the Accounts page). Classification and the
   override live on the Accounts page; the performance view is its own screen.
