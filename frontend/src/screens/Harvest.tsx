import { useMemo, useState } from "react";
import { useHarvest, useSimulate } from "../api/hooks";
import {
  ApiPill,
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

export function Harvest() {
  const harvest = useHarvest();
  const simulate = useSimulate();
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const candidates = harvest.data ?? [];

  // Default-select all candidates once they load (mirrors the mock's "checked" rows).
  const allIds = useMemo(() => candidates.map((c) => c.lot_id), [candidates]);

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }
  function toggleAll() {
    setSelected((prev) => (prev.size === allIds.length ? new Set() : new Set(allIds)));
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
        <p>
          Lots trading below cost — selling realizes a loss that can offset gains.{" "}
          <ApiPill>GET /api/tax/harvest</ApiPill>
        </p>
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
            <table>
              <thead>
                <tr>
                  <th>
                    <input
                      type="checkbox"
                      checked={selected.size === allIds.length && allIds.length > 0}
                      onChange={toggleAll}
                      aria-label="Select all"
                    />
                  </th>
                  <th>Security</th>
                  <th>Opened</th>
                  <th>Term</th>
                  <th className="r">Qty</th>
                  <th className="r">Market value</th>
                  <th className="r">Unrealized loss</th>
                  <th className="r">Est. tax saving</th>
                </tr>
              </thead>
              <tbody>
                {candidates.map((c) => (
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
        </Card>

        {/* Simulator — POST /api/tax/simulate */}
        <Card>
          <CardHead title="Sell simulation" right={<ApiPill>POST /api/tax/simulate</ApiPill>} />
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
                <p className="hint mt8">Simulation only — TaxLossApp never places trades.</p>
              </div>
            )}
          </div>
        </Card>
      </div>

      <p className="foot">Federal + California · v1.</p>
    </div>
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
