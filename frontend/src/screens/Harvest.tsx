import { useMemo, useState } from "react";
import { useHarvest, useSimulate } from "../api/hooks";
import {
  Card,
  CardHead,
  Disclaimer,
  EmptyState,
  ErrorState,
  Money,
  Spinner,
  TermChip,
  WashSaleChip,
} from "../components/ui";
import { fmtDate, money, qty } from "../lib/format";
import {
  DEFAULT_VIEW,
  filterSortCandidates,
  type SortKey,
  type TermFilter,
} from "../lib/harvest";

export function Harvest() {
  const harvest = useHarvest();
  const simulate = useSimulate();
  const [selected, setSelected] = useState<Set<string>>(new Set());

  // Search / sort / filter state for the candidate table.
  const [search, setSearch] = useState("");
  const [term, setTerm] = useState<TermFilter>(DEFAULT_VIEW.term);
  const [hideWash, setHideWash] = useState(DEFAULT_VIEW.hideWash);
  const [sortKey, setSortKey] = useState<SortKey>(DEFAULT_VIEW.sortKey);
  const [sortDir, setSortDir] = useState(DEFAULT_VIEW.sortDir);

  const candidates = harvest.data ?? [];

  // The filtered + sorted rows currently visible in the table.
  const visible = useMemo(
    () => filterSortCandidates(candidates, { search, term, hideWash, sortKey, sortDir }),
    [candidates, search, term, hideWash, sortKey, sortDir],
  );

  // "Select all" operates over the *visible* set; selection is keyed by lot_id
  // and survives filtering (ids stay selected even when hidden).
  const visibleIds = useMemo(() => visible.map((c) => c.lot_id), [visible]);
  const allVisibleSelected =
    visibleIds.length > 0 && visibleIds.every((id) => selected.has(id));

  // Click a header: toggle direction if already sorting by it, else select it.
  function sortBy(key: SortKey) {
    if (key === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("desc");
    }
  }
  const sortArrow = (key: SortKey) => (sortKey === key ? (sortDir === "asc" ? " ▲" : " ▼") : "");

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }
  function toggleAll() {
    setSelected((prev) => {
      const next = new Set(prev);
      if (allVisibleSelected) {
        for (const id of visibleIds) next.delete(id);
      } else {
        for (const id of visibleIds) next.add(id);
      }
      return next;
    });
  }

  function runSimulation() {
    const sales = [...selected].map((lot_id) => ({ lot_id })); // full remaining quantity
    if (sales.length > 0) simulate.mutate({ sales });
  }

  const result = simulate.data;

  return (
    <div className="app">
      <div className="page-head">
        <h1>Tax-loss harvesting</h1>
        <p>Lots trading below cost — selling realizes a loss that can offset gains.</p>
      </div>

      <Disclaimer>
        Estimates for decision-support, <strong>not tax advice</strong>. Wash-sale flags are heuristic (same
        security bought within ±30 days). Confirm with a professional before trading.
      </Disclaimer>

      <div className="grid dash mt16">
        {/* Candidates — GET /api/tax/harvest */}
        <Card>
          <CardHead title="Loss candidates" right={<span className="faint">select lots to simulate</span>} />
          {harvest.isLoading ? (
            <Spinner />
          ) : harvest.isError ? (
            <ErrorState error={harvest.error} onRetry={() => harvest.refetch()} />
          ) : candidates.length === 0 ? (
            <EmptyState title="No losses to harvest" hint="None of your open lots are below cost right now." />
          ) : (
            <>
              <div className="toolbar" style={{ flexWrap: "wrap" }}>
                <input
                  className="input"
                  type="search"
                  placeholder="Search ticker…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  aria-label="Search by ticker"
                  style={{ flex: "1 1 160px", width: "auto" }}
                />
                <div className="toggle" role="group" aria-label="Filter by term">
                  {(
                    [
                      ["all", "All"],
                      ["short_term", "Short"],
                      ["long_term", "Long"],
                    ] as [TermFilter, string][]
                  ).map(([value, label]) => (
                    <button
                      key={value}
                      type="button"
                      className={term === value ? "on" : ""}
                      onClick={() => setTerm(value)}
                      aria-pressed={term === value}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <label className="flex" style={{ gap: 6, fontSize: 13, whiteSpace: "nowrap" }}>
                  <input
                    type="checkbox"
                    checked={hideWash}
                    onChange={(e) => setHideWash(e.target.checked)}
                  />
                  Hide wash-sale
                </label>
              </div>

              {visible.length === 0 ? (
                <EmptyState title="No matches" hint="No candidates match your search or filters." />
              ) : (
                <table>
                  <thead>
                    <tr>
                      <th>
                        <input
                          type="checkbox"
                          checked={allVisibleSelected}
                          onChange={toggleAll}
                          aria-label="Select all visible"
                        />
                      </th>
                      <th>Security</th>
                      <SortHeader label="Opened" col="open_date" sortBy={sortBy} arrow={sortArrow} />
                      <th>Term</th>
                      <SortHeader className="r" label="Qty" col="quantity" sortBy={sortBy} arrow={sortArrow} />
                      <SortHeader className="r" label="Market value" col="market_value" sortBy={sortBy} arrow={sortArrow} />
                      <SortHeader className="r" label="Unrealized loss" col="unrealized_loss" sortBy={sortBy} arrow={sortArrow} />
                      <SortHeader className="r" label="Est. tax saving" col="estimated_tax_saving" sortBy={sortBy} arrow={sortArrow} />
                    </tr>
                  </thead>
                  <tbody>
                    {visible.map((c) => (
                  <tr key={c.lot_id}>
                    <td>
                      <input
                        type="checkbox"
                        checked={selected.has(c.lot_id)}
                        onChange={() => toggle(c.lot_id)}
                        aria-label={`Select ${c.ticker ?? "lot"}`}
                      />
                    </td>
                    <td className="tick">
                      {c.ticker ?? "—"} {c.wash_sale_warning && <WashSaleChip />}
                    </td>
                    <td className="num muted">{fmtDate(c.open_date)}</td>
                    <td>
                      <TermChip term={c.term} />
                    </td>
                    <td className="r num">{qty(c.quantity)}</td>
                    <td className="r num">{money(c.market_value)}</td>
                    <td className="r">
                      <Money value={c.unrealized_loss} />
                    </td>
                    <td className="r num gain">{money(c.estimated_tax_saving)}</td>
                  </tr>
                ))}
                  </tbody>
                </table>
              )}
            </>
          )}
        </Card>

        {/* Simulator — POST /api/tax/simulate */}
        <Card>
          <CardHead title="Sell simulation" />
          <div className="card-body">
            <button
              className="btn primary block"
              disabled={selected.size === 0 || simulate.isPending}
              onClick={runSimulation}
            >
              {simulate.isPending ? "Simulating…" : `Simulate sale of ${selected.size} lot(s)`}
            </button>

            {simulate.isError && (
              <div className="loss mt16" style={{ fontSize: 13 }}>
                {(simulate.error as Error).message}
              </div>
            )}

            {result && (
              <div className="mt16">
                <Row label="Proceeds" value={money(result.total_proceeds)} />
                <Row label="Cost basis" value={money(result.total_cost_basis)} />
                <Row label="Short-term gain" value={<Money value={result.short_term_gain} />} />
                <Row label="Long-term gain" value={<Money value={result.long_term_gain} />} />
                <hr className="sep" />
                <Row label="Total realized" value={<Money value={result.total_gain} />} />
                <Row
                  label="Est. tax effect"
                  value={<span className="num">{money(result.estimated_tax.total)}</span>}
                />
                <div className="faint" style={{ fontSize: 12, marginTop: 2 }}>
                  fed {money(result.estimated_tax.federal)} · NIIT {money(result.estimated_tax.niit)} · CA{" "}
                  {money(result.estimated_tax.state)}
                </div>
                <hr className="sep" />
                <div className="flex between" style={{ fontSize: 16, fontWeight: 700 }}>
                  <span>After-tax proceeds</span>
                  <span className="num">{money(result.after_tax_proceeds)}</span>
                </div>
                <p className="hint mt8">Simulation only — Squirrel never places trades.</p>
              </div>
            )}
          </div>
        </Card>
      </div>

      <p className="foot">Federal + California · v1.</p>
    </div>
  );
}

/** A clickable column header that sorts by `col`, showing a direction arrow
 *  when active. Styled as a plain header but cursor-pointer + selectable. */
function SortHeader({
  label,
  col,
  sortBy,
  arrow,
  className = "",
}: {
  label: string;
  col: SortKey;
  sortBy: (k: SortKey) => void;
  arrow: (k: SortKey) => string;
  className?: string;
}) {
  return (
    <th
      className={className}
      onClick={() => sortBy(col)}
      style={{ cursor: "pointer", userSelect: "none", whiteSpace: "nowrap" }}
      aria-label={`Sort by ${label}`}
    >
      {label}
      {arrow(col)}
    </th>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex between mt8">
      <span className="muted">{label}</span>
      <span className="num">{value}</span>
    </div>
  );
}
