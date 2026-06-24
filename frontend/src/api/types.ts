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
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
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
export type AlertType = "approaching_long_term" | "harvestable_loss";
export interface Alert {
  id: Uuid;
  user_id: Uuid;
  type: string; // AlertType, but kept open for forward-compat
  security_id: Uuid | null;
  title: string;
  message: string;
  payload: Record<string, unknown>;
  created_at: IsoDateTime;
  read_at: IsoDateTime | null;
  emailed_at: IsoDateTime | null;
}

// POST /api/plaid/sandbox/connect (and /exchange) response
export interface ConnectResponse {
  item_id: string;
  summary: Record<string, unknown>;
}
