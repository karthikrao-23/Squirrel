# Squirrel — M6 UI/UX Design

The **design gate** before the React build (M7). Four core screens are mocked as
viewable HTML in [`design/`](./design/) and mapped here to the §5 API endpoints
they consume. Reviewing this before coding the frontend avoids rework.

- **View locally:** open `design/index.html` in a browser (no build step).
- **View as cards:** the same files are synced to a Claude Design project (claude.ai/design).
- **Stack target (M7):** React + TypeScript + Vite + TanStack Query + Recharts. The
  CSS variables in `design/styles.css` are the source of truth for the M7 theme.

> Everything here is **decision-support, not tax advice** — every screen carries a
> disclaimer. Tax scope is **federal + California** in v1.

---

## Screen → endpoint map

### 1. Onboarding — `design/onboarding.html`
The land → connect → sync → profile flow (§5 "Onboarding flow").

| UI element | Endpoint | Notes |
|---|---|---|
| "Connect your brokerage" button | `POST /api/plaid/link-token` | returns a Link token; frontend opens Plaid Link |
| Link returns `public_token` | `POST /api/plaid/exchange` | stores encrypted item, triggers initial holdings + transactions sync |
| "Importing your data" progress | (server-side after exchange) | lots reconstructed FIFO; webhook `POST /api/plaid/webhook` keeps them fresh |
| Filing status + taxable income form | `PATCH /api/profile` | 4 statuses: `single`, `married_filing_jointly`, `married_filing_separately`, `head_of_household`; `taxable_income` ≥ 0 |

### 2. Dashboard — `design/dashboard.html`
| UI element | Endpoint | Fields used |
|---|---|---|
| Market-value tile | `GET /api/tax/summary` | `total_market_value`, `lots_valued` |
| Unrealized tile | `GET /api/tax/summary` | `total_unrealized`, `total_cost_basis` |
| ST/LT split tile | `GET /api/tax/summary` | `unrealized_short_term`, `unrealized_long_term` |
| Est. tax tile | `GET /api/tax/summary` | `estimated_tax_if_sold_now` (`federal`, `niit`, `state`, `total`) |
| Holdings table | `GET /api/holdings` | `ticker`, `security_name`, `account_name`, `quantity`, `institution_price`, `institution_value`, `cost_basis` (unrealized = value − basis, computed client-side) |
| Allocation donut | `GET /api/holdings` | grouped by security type (client-side) |
| **Portfolio-value chart** | ⚠️ **no endpoint yet** | needs a value-history endpoint; flagged as M7+ backend work, mocked for now |

### 3. Harvest — `design/harvest.html`
| UI element | Endpoint | Fields used |
|---|---|---|
| Loss-candidates table | `GET /api/tax/harvest` | `ticker`, `open_date`, `term`, `quantity`, `cost_basis`, `market_value`, `unrealized_loss`, `estimated_tax_saving`, `wash_sale_warning` |
| Lot selection → sim panel | `POST /api/tax/simulate` | request: `{ sales: [{ lot_id, quantity? }] }` |
| Simulation results | `POST /api/tax/simulate` | `total_proceeds`, `total_cost_basis`, `short_term_gain`, `long_term_gain`, `estimated_tax`, `after_tax_proceeds` |

### 4. Alerts — `design/alerts.html`
| UI element | Endpoint | Fields used |
|---|---|---|
| Nav bell badge | `GET /api/alerts?unread_only=true` | unread count |
| Alert list | `GET /api/alerts` | `type` (`approaching_long_term` / `harvestable_loss`), `title`, `message`, `created_at`, `read_at`, `emailed_at`, `payload` (saving, wash-sale) |
| All / Unread toggle | `GET /api/alerts?unread_only=` | |
| "Mark read" | `POST /api/alerts/{id}/read` | |

---

## Component inventory (→ M7 React components)
Top nav + bell badge · stat tile · data table (sortable) · gain/loss number ·
term chip (ST/LT) · wash-sale chip · area chart (Recharts) · allocation donut ·
stepper · filing-status radio cards · simulate summary panel · alert item ·
disclaimer banner. Tokens live in `design/styles.css` (`:root` variables).

## States to handle (each screen)
- **Loading** — skeleton rows / spinner while TanStack Query is fetching.
- **Empty** — onboarding not done (no accounts → route to onboarding); no harvest
  candidates ("No losses to harvest — nice."); no alerts ("You're all caught up").
- **Error** — endpoint 4xx/5xx → inline retry; `AppError::BadRequest` messages surfaced.
- **Unpriced lots** — `tax/summary` reports `lots_unpriced`; show a "N lots not priced" footnote.
- **Stale data** — show `as_of` date; offer Re-sync (`POST /api/lots/rebuild` / Plaid resync).

## Open questions for review
1. Portfolio-value history chart needs a new backend endpoint (snapshots over time) — defer to M7 or add to backlog?
2. Should the sell simulator allow partial-lot quantities in the UI, or whole-lot only for v1? (API already supports `quantity?`.)
3. Realized gains are deferred (M3 stores only open lots) — confirm dashboard shows **unrealized only** for v1.

## Verification (PLAN.md §7, M6)
- [x] Mocks exist for all four core screens (onboarding, dashboard, harvest, alerts).
- [x] Each screen mapped to its §5 endpoints (tables above).
- [x] "Not tax advice" disclaimer on every screen.
- [ ] Reviewed & approved → proceed to M7.
