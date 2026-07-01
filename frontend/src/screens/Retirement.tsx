import { useMemo } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useRetirement } from "../api/hooks";
import { Card, CardHead, Disclaimer, ErrorState, Money, Spinner, Stat } from "../components/ui";
import { fmtDate, money, num } from "../lib/format";

/** Format a fractional return (0.123 → "+12.3%"); null → em dash. */
function pct(r: number | null): string {
  if (r == null || !Number.isFinite(r)) return "—";
  return `${r >= 0 ? "+" : ""}${(r * 100).toFixed(1)}%`;
}

export function Retirement() {
  const ret = useRetirement();

  const series = useMemo(
    () =>
      (ret.data?.history ?? []).map((s) => ({
        as_of: s.as_of,
        market_value: num(s.market_value),
      })),
    [ret.data],
  );

  if (ret.isLoading) {
    return (
      <div className="app">
        <Spinner label="Loading retirement…" />
      </div>
    );
  }
  if (ret.isError) {
    return (
      <div className="app">
        <Card>
          <ErrorState error={ret.error} onRetry={() => ret.refetch()} />
        </Card>
      </div>
    );
  }

  const data = ret.data!;
  const hasAccounts = data.accounts.length > 0;

  return (
    <div className="app">
      <div className="page-head">
        <h1>Retirement</h1>
        <p>
          Performance of your tax-advantaged accounts (IRA / Roth / 401k) as a group. No tax
          harvesting here — retirement gains aren't taxed on sale.
        </p>
      </div>

      {!hasAccounts ? (
        <Card>
          <div className="faint" style={{ padding: 24, textAlign: "center" }}>
            No retirement accounts connected yet. Connect an IRA / Roth / 401(k) and its holdings
            will show up here.
          </div>
        </Card>
      ) : (
        <>
          {/* Headline metrics */}
          <div className="grid cols-4">
            <Stat
              label="Value"
              value={money(data.total.market_value)}
              sub={`cost basis ${money(data.total.cost_basis)}`}
            />
            <Stat
              label="Total return"
              value={<Money value={data.total.unrealized} />}
              sub={pct(data.total.simple_return)}
            />
            <Stat
              label="IRR (money-weighted)"
              value={pct(data.total.irr)}
              sub="annualized, from your lots"
            />
            <Stat
              label="TWR (time-weighted)"
              value={data.total.twr == null ? "Building…" : pct(data.total.twr)}
              sub={data.total.twr == null ? "needs ≥2 daily snapshots" : "from daily snapshots"}
            />
          </div>

          {/* Value over time (retirement as a whole) */}
          <Card className="mt16">
            <CardHead title="Retirement value over time" />
            <div className="card-body">
              {series.length < 2 ? (
                <div
                  style={{
                    height: 220,
                    display: "grid",
                    placeItems: "center",
                    border: "1px dashed var(--border-strong)",
                    borderRadius: "var(--radius-sm)",
                    color: "var(--text-faint)",
                    textAlign: "center",
                    padding: 16,
                  }}
                >
                  <div className="faint">Building history — a snapshot is recorded once per day.</div>
                </div>
              ) : (
                <div style={{ height: 220 }}>
                  <ResponsiveContainer>
                    <AreaChart data={series} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
                      <defs>
                        <linearGradient id="retFill" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="0%" stopColor="#1a8a55" stopOpacity={0.35} />
                          <stop offset="100%" stopColor="#1a8a55" stopOpacity={0} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                      <XAxis
                        dataKey="as_of"
                        tickFormatter={(d: string) => fmtDate(d)}
                        tick={{ fontSize: 11, fill: "var(--text-faint)" }}
                        minTickGap={24}
                      />
                      <YAxis
                        width={64}
                        tickFormatter={(v: number) => money(v)}
                        tick={{ fontSize: 11, fill: "var(--text-faint)" }}
                      />
                      <Tooltip
                        labelFormatter={(d) => fmtDate(d as string)}
                        formatter={(v: number) => [money(v, 2), "Value"]}
                      />
                      <Area
                        type="monotone"
                        dataKey="market_value"
                        stroke="#1a8a55"
                        strokeWidth={2}
                        fill="url(#retFill)"
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              )}
            </div>
          </Card>

          {/* Per-account breakdown */}
          <Card className="mt16">
            <CardHead title="Accounts" />
            <table>
              <thead>
                <tr>
                  <th>Account</th>
                  <th>Type</th>
                  <th className="r">Value</th>
                  <th className="r">Cost basis</th>
                  <th className="r">Unrealized</th>
                </tr>
              </thead>
              <tbody>
                {data.accounts.map((a) => (
                  <tr key={a.name}>
                    <td>{a.name}</td>
                    <td className="muted">{a.subtype ?? "—"}</td>
                    <td className="r num">{money(a.market_value)}</td>
                    <td className="r num">{money(a.cost_basis)}</td>
                    <td className="r">
                      <Money value={a.unrealized} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>
        </>
      )}

      <Disclaimer />
      <p className="foot">
        IRR weights each lot by when it was acquired; TWR is built from daily value snapshots and
        sharpens over time. Decision-support, not tax advice.
      </p>
    </div>
  );
}
