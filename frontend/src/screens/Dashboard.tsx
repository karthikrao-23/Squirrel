import { useMemo } from "react";
import { Cell, Pie, PieChart, ResponsiveContainer, Tooltip } from "recharts";
import { useQueryClient } from "@tanstack/react-query";
import { keys, useHoldings, useSummary } from "../api/hooks";
import type { Holding } from "../api/types";
import { ApiPill, Card, CardHead, Disclaimer, ErrorState, Money, Spinner, Stat } from "../components/ui";
import { fmtDate, money, num, qty } from "../lib/format";

const DONUT_COLORS = ["#1f6feb", "#1a8a55", "#b7791f", "#8a93a1", "#7c5cff", "#0ea5a3"];

function holdingValue(h: Holding): number {
  return num(h.institution_value);
}
function holdingGain(h: Holding): number {
  return num(h.institution_value) - num(h.cost_basis);
}

export function Dashboard() {
  const qc = useQueryClient();
  const summary = useSummary();
  const holdings = useHoldings();

  const allocation = useMemo(() => {
    const rows = (holdings.data ?? [])
      .map((h) => ({ name: h.ticker ?? h.security_name ?? "—", value: holdingValue(h) }))
      .filter((r) => r.value > 0)
      .sort((a, b) => b.value - a.value);
    const top = rows.slice(0, 5);
    const rest = rows.slice(5).reduce((s, r) => s + r.value, 0);
    return rest > 0 ? [...top, { name: "Other", value: rest }] : top;
  }, [holdings.data]);

  return (
    <div className="app">
      <div className="page-head flex between">
        <div>
          <h1>Dashboard</h1>
          <p>
            {summary.data ? `As of ${fmtDate(summary.data.as_of)} · ` : ""}
            <ApiPill>GET /api/tax/summary · GET /api/holdings</ApiPill>
          </p>
        </div>
        <button
          className="btn"
          onClick={() => {
            qc.invalidateQueries({ queryKey: keys.summary });
            qc.invalidateQueries({ queryKey: keys.holdings });
          }}
        >
          ↻ Refresh
        </button>
      </div>

      {/* Stat tiles — GET /api/tax/summary */}
      {summary.isLoading ? (
        <Card><Spinner /></Card>
      ) : summary.isError ? (
        <Card><ErrorState error={summary.error} onRetry={() => summary.refetch()} /></Card>
      ) : summary.data ? (
        <div className="grid cols-4">
          <Stat
            label="Market value"
            value={money(summary.data.total_market_value)}
            sub={`${summary.data.lots_valued} lots valued${
              summary.data.lots_unpriced ? ` · ${summary.data.lots_unpriced} unpriced` : ""
            }`}
          />
          <Stat
            label="Total unrealized"
            value={<Money value={summary.data.total_unrealized} />}
            sub={`cost basis ${money(summary.data.total_cost_basis)}`}
          />
          <Stat
            label="ST / LT split"
            value={
              <>
                <Money value={summary.data.unrealized_short_term} /> <span className="faint">/</span>{" "}
                <Money value={summary.data.unrealized_long_term} />
              </>
            }
            sub="short-term · long-term"
          />
          <Stat
            label="Est. tax if sold now"
            value={money(summary.data.estimated_tax_if_sold_now.total)}
            sub={
              `fed ${money(summary.data.estimated_tax_if_sold_now.federal)} · ` +
              `NIIT ${money(summary.data.estimated_tax_if_sold_now.niit)} · ` +
              `CA ${money(summary.data.estimated_tax_if_sold_now.state)}`
            }
          />
        </div>
      ) : null}

      <div className="grid dash mt16">
        {/* Value-over-time: no backend endpoint yet (point-in-time only). */}
        <Card>
          <CardHead title="Portfolio value" />
          <div className="card-body">
            <div
              style={{
                height: 200,
                display: "grid",
                placeItems: "center",
                border: "1px dashed var(--border-strong)",
                borderRadius: "var(--radius-sm)",
                color: "var(--text-faint)",
                textAlign: "center",
                padding: 16,
              }}
            >
              <div>
                <div style={{ fontWeight: 600 }}>Performance history coming soon</div>
                <div className="faint mt8">
                  Needs a value-snapshot endpoint — <code>tax/summary</code> is point-in-time. Tracked for a
                  later milestone.
                </div>
              </div>
            </div>
          </div>
        </Card>

        {/* Allocation donut — GET /api/holdings */}
        <Card>
          <CardHead title="Allocation" />
          <div className="card-body flex" style={{ gap: 20 }}>
            {holdings.isLoading ? (
              <Spinner />
            ) : allocation.length === 0 ? (
              <div className="faint">No priced holdings.</div>
            ) : (
              <>
                {/* Fixed, non-shrinking box so the long legend can't squeeze the
                    chart; percentage radii so the donut scales to fill it (a
                    fixed pixel radius overflows + clips when the box shrinks). */}
                <div style={{ flex: "0 0 130px", height: 130 }}>
                  <ResponsiveContainer>
                    <PieChart>
                      <Pie data={allocation} dataKey="value" innerRadius="55%" outerRadius="98%" paddingAngle={2}>
                        {allocation.map((_, i) => (
                          <Cell key={i} fill={DONUT_COLORS[i % DONUT_COLORS.length]} />
                        ))}
                      </Pie>
                      <Tooltip formatter={(v: number) => money(v)} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                <div className="legend" style={{ flex: 1, minWidth: 0 }}>
                  {allocation.map((r, i) => (
                    <div className="row" key={r.name}>
                      <span
                        className="sw"
                        style={{ background: DONUT_COLORS[i % DONUT_COLORS.length], flexShrink: 0 }}
                      />
                      <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>
                        {r.name} · {money(r.value)}
                      </span>
                    </div>
                  ))}
                </div>
              </>
            )}
          </div>
        </Card>
      </div>

      {/* Holdings — GET /api/holdings */}
      <Card className="mt16">
        <CardHead title="Holdings" right={<ApiPill>GET /api/holdings</ApiPill>} />
        {holdings.isLoading ? (
          <Spinner />
        ) : holdings.isError ? (
          <ErrorState error={holdings.error} onRetry={() => holdings.refetch()} />
        ) : (holdings.data ?? []).length === 0 ? (
          <div className="faint" style={{ padding: 16 }}>No holdings yet.</div>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Security</th>
                <th>Account</th>
                <th className="r">Qty</th>
                <th className="r">Price</th>
                <th className="r">Market value</th>
                <th className="r">Cost basis</th>
                <th className="r">Unrealized</th>
              </tr>
            </thead>
            <tbody>
              {holdings.data!.map((h) => (
                <tr key={`${h.account_id}-${h.security_id}`}>
                  <td className="tick">
                    {h.ticker ?? "—"}
                    <span className="name">{h.security_name ?? ""}</span>
                  </td>
                  <td className="muted">{h.account_name}</td>
                  <td className="r num">{qty(h.quantity)}</td>
                  <td className="r num">{h.institution_price ? money(h.institution_price, 2) : "—"}</td>
                  <td className="r num">{money(h.institution_value, 0)}</td>
                  <td className="r num">{money(h.cost_basis, 0)}</td>
                  <td className="r">
                    <Money value={holdingGain(h)} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      <Disclaimer />
      <p className="foot">Showing unrealized positions (open lots). Federal + California · v1.</p>
    </div>
  );
}
