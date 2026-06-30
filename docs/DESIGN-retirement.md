# Design: Retirement accounts — performance, not tax

> Status: **proposal for review** (no code yet). Feature #3 of the current batch.

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
**derived, not stored** — no migration; one unit-tested function. Surface it as a
`kind` field on the `/api/accounts` response and on each lot/holding row.

> **Caveat to document, not solve in v1:** wash sales technically *do* cross
> account boundaries (a replacement buy in an IRA within ±30 days can disallow a
> loss in a taxable account). That's an edge case; v1 simply excludes retirement
> accounts from harvest candidates and notes this for later.

## What changes per kind

**Taxable** — unchanged: lots, harvest candidates, ST/LT, tax-if-sold, wash-sale.

**Retirement** — suppress the tax framing, show performance:
- **Excluded from** `/api/tax/harvest` (no tax benefit) and from the "Est. tax if
  sold" figure. The harvest query gains a join on account kind and filters
  retirement lots out.
- **Performance surfaced instead:** current value, total cost basis, **total
  return ($ and %)**, and **value-over-time**.

## Performance metrics — what's feasible with our data

We have holdings (value + cost basis), reconciled tax lots, transactions, and —
once feature #4 lands — daily **value snapshots**.

| Metric | Definition | Feasible now? |
|---|---|---|
| Current value | Σ holding market value | ✅ |
| Total return ($) | value − cost basis | ✅ |
| Total return (%) | (value − cost basis) / cost basis | ✅ |
| Value over time | snapshot history (per account) | ✅ once #4 ships (needs per-account snapshots) |
| **Time-weighted return (TWR)** | return independent of contribution timing | ⚠️ needs cash-flow history |
| **Money-weighted return (IRR)** | return weighting contribution size/timing | ⚠️ needs cash-flow history |

**v1 headline = simple total return + value-over-time chart.** This is honest and
immediately useful. True TWR/IRR needs reliable **contribution/withdrawal**
classification (which Plaid transaction types only approximate), so it's deferred
to v2.

> Note: simple "total return %" reflects unrealized return on *current* holdings,
> not the account's lifetime return. The value-over-time chart conveys trajectory
> better; we'll label the % precisely ("unrealized return on current holdings").

## UI

Reuse the **Accounts page** (feature #1) and make each account card
**kind-aware** rather than building a separate screen:

```
Accounts
├─ TAXABLE
│   └─ [Brokerage card]   value · unrealized · ST/LT · lots table · (harvest-relevant)
│   └─ [Stock Plan card]  value · unrealized · lots table
└─ RETIREMENT
    └─ [Roth IRA card]    value · TOTAL RETURN $/%  · value-over-time sparkline · holdings
    └─ [401(k) card]      value · TOTAL RETURN $/%  · value-over-time sparkline · holdings
```

- Group accounts by kind with a section header; sort retirement after taxable.
- Retirement cards drop the ST/LT and "tax saving" columns and lead with **return
  %** + a small **value chart** (from per-account snapshots).
- **Dashboard:** add a one-line **taxable vs retirement** value split, and exclude
  retirement from the "Est. tax if sold now" tile.
- **Harvest tab:** unaffected for the user (retirement lots simply never appear).

## Implementation plan (when approved)

Small, mostly additive — leans on features #1 (accounts page) and #4 (snapshots):

1. **`AccountKind::from_subtype`** pure helper in `crates/domain` + unit tests.
2. **API:** add `kind` to the accounts/holdings/lots responses (derived).
3. **Harvest:** filter out retirement-account lots in the harvest query/handler.
4. **Snapshots (extends #4):** record snapshots **per account** (add `account_id`
   to `portfolio_snapshots`, nullable = whole-portfolio) so each retirement card
   gets its own value chart.
5. **Frontend:** kind-aware Accounts cards (performance vs tax), dashboard
   taxable/retirement split.
6. **Defer (v2):** contribution tracking → TWR/IRR; cross-account wash-sale logic.

## Open questions for you

1. **Scope for v1:** the minimal-but-useful set is classify + exclude-from-harvest
   + per-account performance card with total-return % and a value chart. Good, or
   do you want TWR/IRR (which needs contribution data) sooner?
2. **Per-account value chart** depends on per-account snapshots — OK to extend the
   feature-#4 snapshot table with `account_id`, or keep #4 portfolio-total only
   for now and add per-account later?
3. **Where to show it:** integrate into the Accounts page (recommended) vs a
   dedicated "Retirement" tab?
