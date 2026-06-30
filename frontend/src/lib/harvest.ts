// Pure filter + sort logic for the tax-loss harvest candidate table.
// Kept out of the React component so it can be unit-tested in isolation
// (see harvest.test.ts). Sorting parses Dec-as-string fields with `num()`.

import type { HarvestCandidate, Term } from "../api/types";
import { num } from "./format";

export type SortKey =
  | "unrealized_loss"
  | "estimated_tax_saving"
  | "market_value"
  | "open_date"
  | "quantity";

export type SortDir = "asc" | "desc";

export type TermFilter = "all" | Term;

export interface HarvestView {
  search: string;
  term: TermFilter;
  hideWash: boolean;
  sortKey: SortKey;
  sortDir: SortDir;
}

export const DEFAULT_VIEW: HarvestView = {
  search: "",
  term: "all",
  hideWash: false,
  sortKey: "estimated_tax_saving",
  sortDir: "desc",
};

/** Comparable scalar for a candidate under a given sort key.
 *  Numeric Dec strings → number; open_date → epoch ms. */
function sortValue(c: HarvestCandidate, key: SortKey): number {
  if (key === "open_date") {
    // NaiveDate "YYYY-MM-DD"; pin to midnight so it parses consistently.
    return new Date(`${c.open_date}T00:00:00`).getTime();
  }
  return num(c[key]);
}

/**
 * Filter then sort harvest candidates for display. Pure — never mutates the
 * input array.
 *
 * - `search`: case-insensitive substring match on ticker (null ticker never matches a non-empty search).
 * - `term`: "all", or restrict to one of "short_term" / "long_term".
 * - `hideWash`: drop candidates flagged with a wash-sale warning.
 * - `sortKey` / `sortDir`: numeric/date sort, ascending or descending.
 */
export function filterSortCandidates(
  candidates: HarvestCandidate[],
  view: HarvestView,
): HarvestCandidate[] {
  const { search, term, hideWash, sortKey, sortDir } = view;
  const q = search.trim().toLowerCase();

  const filtered = candidates.filter((c) => {
    if (q && !(c.ticker ?? "").toLowerCase().includes(q)) return false;
    if (term !== "all" && c.term !== term) return false;
    if (hideWash && c.wash_sale_warning) return false;
    return true;
  });

  const dir = sortDir === "asc" ? 1 : -1;
  // Copy before sorting so we don't mutate the caller's array.
  return [...filtered].sort((a, b) => {
    const diff = sortValue(a, sortKey) - sortValue(b, sortKey);
    return diff * dir;
  });
}
