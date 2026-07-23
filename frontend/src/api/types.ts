// TypeScript mirrors of the Rust API response shapes (crates/api routes + db models).
//
// IMPORTANT: money/quantity fields are `rust_decimal::Decimal`, and the workspace
// builds it with the `serde-with-str` feature — so they arrive as JSON **strings**
// (e.g. "83968.00"), not numbers. They're typed `Dec` here; use `num()`/`money()`
// from lib/format.ts to render them.

export type Dec = string;
export type Uuid = string;
export type IsoDate = string; // "YYYY-MM-DD" (chrono NaiveDate)
export type IsoDateTime = string; // RFC3339 (chrono DateTime<Utc>)

export type Term = "short_term" | "long_term";

// An account's classification. `debt` is a liability (loan/margin/credit line)
// excluded from portfolio value. `AccountKindOverride` is the user's manual
// setting: null = classify automatically from the Plaid subtype ("Auto").
export type AccountKind = "taxable" | "retirement" | "debt";
export type AccountKindOverride = AccountKind | null;

export type FilingStatus =
  | "single"
  | "married_filing_jointly"
  | "married_filing_separately"
  | "head_of_household";

export const FILING_STATUS_LABELS: Record<FilingStatus, string> = {
  single: "Single",
  married_filing_jointly: "Married filing jointly",
  married_filing_separately: "Married filing separately",
  head_of_household: "Head of household",
};

// GET/PATCH /api/profile and GET /api/auth/me. `email` is NOT NULL since auth.
export interface User {
  id: Uuid;
  email: string;
  filing_status: string;
  taxable_income: Dec;
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
}

// GET /api/accounts → { accounts: Account[] }
export interface Account {
  id: Uuid;
  user_id: Uuid;
  plaid_item_id: Uuid;
  plaid_account_id: string;
  name: string;
  official_name: string | null;
  type: string | null;
  subtype: string | null;
  current_balance: Dec | null;
  kind: AccountKind; // resolved (subtype + any override)
  kind_override: AccountKindOverride;
  // Last successful refresh from the brokerage; null until the first sync.
  // Written only by the sync path, so (unlike updated_at) it isn't moved by
  // manual edits like a tax-classification change.
  last_synced_at: IsoDateTime | null;
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
}

// GET /api/plaid/items — one Plaid connection (Link session) + the accounts it
// brought in. Removing a connection removes those accounts.
export interface ConnectionAccount {
  id: Uuid;
  name: string;
  subtype: string | null;
  kind: AccountKind;
  kind_override: AccountKindOverride;
}
export interface Connection {
  id: Uuid;
  institution_name: string | null;
  institution_id: string | null;
  status: string;
  created_at: IsoDateTime;
  accounts: ConnectionAccount[];
}

// GET /api/holdings → { holdings: Holding[] }
export interface Holding {
  account_id: Uuid;
  account_name: string;
  security_id: Uuid;
  ticker: string | null;
  security_name: string | null;
  quantity: Dec;
  institution_price: Dec | null;
  institution_value: Dec | null;
  cost_basis: Dec | null;
  currency: string | null;
}

// GET /api/accounts/lots → { lots: AccountLot[] }
export interface AccountLot {
  id: Uuid;
  account_id: Uuid;
  account_name: string;
  account_subtype: string | null;
  account_kind: AccountKind; // effective kind after any override
  account_kind_override: AccountKindOverride; // user's manual setting, or null
  security_id: Uuid;
  ticker: string | null;
  open_date: IsoDate;
  term: Term;
  remaining_quantity: Dec;
  cost_basis_per_share: Dec;
  close_price: Dec | null;
}

// An account with no lots, valued from Plaid's reported balance (holdings
// unavailable, e.g. Fidelity BrokerageLink).
export interface AccountBalanceOnly {
  account_id: Uuid;
  name: string;
  subtype: string | null;
  kind: AccountKind;
  kind_override: AccountKindOverride;
  current_balance: Dec;
}

// GET /api/accounts/lots
export interface AccountLotsResp {
  lots: AccountLot[];
  balance_only: AccountBalanceOnly[];
}

// Shared tax estimate breakdown
export interface TaxEstimate {
  federal: Dec;
  niit: Dec;
  state: Dec;
  total: Dec;
}

// GET /api/tax/summary
export interface TaxSummary {
  as_of: IsoDate;
  total_cost_basis: Dec;
  total_market_value: Dec;
  unrealized_short_term: Dec;
  unrealized_long_term: Dec;
  total_unrealized: Dec;
  estimated_tax_if_sold_now: TaxEstimate;
  lots_valued: number;
  lots_unpriced: number;
}

// GET /api/portfolio/history → { history: PortfolioSnapshot[] }
export interface PortfolioSnapshot {
  as_of: IsoDate;
  market_value: Dec;
  cost_basis: Dec;
}

// GET /api/retirement — performance view of tax-advantaged accounts, as a group.
export interface RetirementAccount {
  name: string;
  subtype: string | null;
  market_value: Dec;
  // Null for balance_only accounts (value from Plaid balance, no cost basis).
  cost_basis: Dec | null;
  unrealized: Dec | null;
  balance_only: boolean;
}
export interface RetirementTotals {
  market_value: Dec; // includes balance_only accounts
  cost_basis: Dec;
  unrealized: Dec;
  simple_return: number | null; // fraction, e.g. 0.12 = +12%
  irr: number | null; // money-weighted, annualized
  twr: number | null; // time-weighted (null until ≥2 daily snapshots)
  return_excludes: number; // # balance_only accounts excluded from the return
}
export interface RetirementSummary {
  accounts: RetirementAccount[];
  total: RetirementTotals;
  history: PortfolioSnapshot[];
}

// GET /api/tax/harvest → { candidates: HarvestCandidate[] }
export interface HarvestCandidate {
  lot_id: Uuid;
  security_id: Uuid;
  account_id: Uuid;
  ticker: string | null;
  open_date: IsoDate;
  term: Term;
  quantity: Dec;
  cost_basis: Dec;
  market_value: Dec;
  unrealized_loss: Dec;
  estimated_tax_saving: Dec;
  wash_sale_warning: boolean;
}

// POST /api/tax/simulate
export interface SaleRequest {
  lot_id: Uuid;
  quantity?: Dec; // omit to sell the full remaining quantity
}
export interface SimulateReq {
  sales: SaleRequest[];
}
export interface SaleResult {
  lot_id: Uuid;
  ticker: string | null;
  term: Term;
  quantity: Dec;
  cost_basis: Dec;
  proceeds: Dec;
  gain: Dec;
}
export interface SimulateResp {
  sales: SaleResult[];
  total_proceeds: Dec;
  total_cost_basis: Dec;
  short_term_gain: Dec;
  long_term_gain: Dec;
  total_gain: Dec;
  estimated_tax: TaxEstimate;
  after_tax_proceeds: Dec;
}

// GET /api/alerts → { alerts: Alert[] }
export type AlertType =
  | "approaching_long_term"
  | "harvestable_loss"
  | "missed_harvest";
export interface Alert {
  id: Uuid;
  user_id: Uuid;
  type: string; // AlertType, but kept open for forward-compat
  security_id: Uuid | null;
  title: string;
  message: string;
  payload: Record<string, unknown>;
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
  read_at: IsoDateTime | null;
  emailed_at: IsoDateTime | null;
}

// POST /api/plaid/link-token
export interface LinkTokenResp {
  link_token: string;
  expiration: string;
}

// POST /api/plaid/sandbox/connect (and /exchange) response
export interface ConnectResponse {
  item_id: string;
  summary: Record<string, unknown>;
}
