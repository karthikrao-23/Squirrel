import { describe, expect, it } from "vitest";
import type { HarvestCandidate } from "../api/types";
import { DEFAULT_VIEW, filterSortCandidates, type HarvestView } from "./harvest";

// Minimal candidate factory — only the fields the helper reads matter.
function cand(p: Partial<HarvestCandidate> & { lot_id: string }): HarvestCandidate {
  return {
    security_id: "sec",
    account_id: "acct",
    ticker: "AAPL",
    open_date: "2024-01-01",
    term: "short_term",
    quantity: "10",
    cost_basis: "1000.00",
    market_value: "800.00",
    unrealized_loss: "-200.00",
    estimated_tax_saving: "50.00",
    wash_sale_warning: false,
    ...p,
  };
}

const view = (over: Partial<HarvestView> = {}): HarvestView => ({ ...DEFAULT_VIEW, ...over });

const A = cand({
  lot_id: "a",
  ticker: "AAPL",
  term: "long_term",
  open_date: "2023-06-15",
  quantity: "10",
  market_value: "800.00",
  unrealized_loss: "-200.00",
  estimated_tax_saving: "50.00",
  wash_sale_warning: false,
});
const B = cand({
  lot_id: "b",
  ticker: "TSLA",
  term: "short_term",
  open_date: "2025-02-01",
  quantity: "5",
  market_value: "1500.00",
  unrealized_loss: "-900.00",
  estimated_tax_saving: "120.00",
  wash_sale_warning: true,
});
const C = cand({
  lot_id: "c",
  ticker: "MSFT",
  term: "short_term",
  open_date: "2024-11-20",
  quantity: "20",
  market_value: "300.00",
  unrealized_loss: "-50.00",
  estimated_tax_saving: "10.00",
  wash_sale_warning: false,
});

const ALL = [A, B, C];
const ids = (cs: HarvestCandidate[]) => cs.map((c) => c.lot_id);

describe("filterSortCandidates", () => {
  it("does not mutate the input array", () => {
    const input = [...ALL];
    filterSortCandidates(input, view({ sortKey: "quantity", sortDir: "asc" }));
    expect(input).toEqual(ALL);
  });

  describe("search", () => {
    it("matches ticker case-insensitively as a substring", () => {
      expect(ids(filterSortCandidates(ALL, view({ search: "sl" })))).toEqual(["b"]); // TSLA
      expect(ids(filterSortCandidates(ALL, view({ search: "msft" })))).toEqual(["c"]);
    });
    it("empty/whitespace search returns everything", () => {
      expect(filterSortCandidates(ALL, view({ search: "   " }))).toHaveLength(3);
    });
    it("null ticker never matches a non-empty search", () => {
      const N = cand({ lot_id: "n", ticker: null });
      expect(ids(filterSortCandidates([N], view({ search: "a" })))).toEqual([]);
      expect(ids(filterSortCandidates([N], view({ search: "" })))).toEqual(["n"]);
    });
  });

  describe("term filter", () => {
    it("all → no filtering", () => {
      expect(filterSortCandidates(ALL, view({ term: "all" }))).toHaveLength(3);
    });
    it("short_term keeps only short-term lots", () => {
      expect(ids(filterSortCandidates(ALL, view({ term: "short_term", sortKey: "quantity", sortDir: "asc" })))).toEqual(["b", "c"]);
    });
    it("long_term keeps only long-term lots", () => {
      expect(ids(filterSortCandidates(ALL, view({ term: "long_term" })))).toEqual(["a"]);
    });
  });

  describe("wash-sale toggle", () => {
    it("hideWash drops flagged candidates", () => {
      expect(ids(filterSortCandidates(ALL, view({ hideWash: true, sortKey: "quantity", sortDir: "asc" })))).toEqual(["a", "c"]);
    });
    it("hideWash off keeps flagged candidates", () => {
      expect(filterSortCandidates(ALL, view({ hideWash: false }))).toHaveLength(3);
    });
  });

  describe("sorting", () => {
    // unrealized_loss values: A=-200, B=-900, C=-50
    it("unrealized_loss asc (most negative first)", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "unrealized_loss", sortDir: "asc" })))).toEqual(["b", "a", "c"]);
    });
    it("unrealized_loss desc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "unrealized_loss", sortDir: "desc" })))).toEqual(["c", "a", "b"]);
    });

    // estimated_tax_saving: A=50, B=120, C=10
    it("estimated_tax_saving asc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "estimated_tax_saving", sortDir: "asc" })))).toEqual(["c", "a", "b"]);
    });
    it("estimated_tax_saving desc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "estimated_tax_saving", sortDir: "desc" })))).toEqual(["b", "a", "c"]);
    });

    // market_value: A=800, B=1500, C=300
    it("market_value asc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "market_value", sortDir: "asc" })))).toEqual(["c", "a", "b"]);
    });
    it("market_value desc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "market_value", sortDir: "desc" })))).toEqual(["b", "a", "c"]);
    });

    // open_date: A=2023-06-15, B=2025-02-01, C=2024-11-20
    it("open_date asc (oldest first)", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "open_date", sortDir: "asc" })))).toEqual(["a", "c", "b"]);
    });
    it("open_date desc (newest first)", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "open_date", sortDir: "desc" })))).toEqual(["b", "c", "a"]);
    });

    // quantity: A=10, B=5, C=20
    it("quantity asc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "quantity", sortDir: "asc" })))).toEqual(["b", "a", "c"]);
    });
    it("quantity desc", () => {
      expect(ids(filterSortCandidates(ALL, view({ sortKey: "quantity", sortDir: "desc" })))).toEqual(["c", "a", "b"]);
    });
  });

  it("combines search + term + hideWash + sort", () => {
    // short-term, not washed, sorted by qty desc → only C qualifies (B is washed)
    const out = filterSortCandidates(
      ALL,
      view({ term: "short_term", hideWash: true, sortKey: "quantity", sortDir: "desc" }),
    );
    expect(ids(out)).toEqual(["c"]);
  });
});
