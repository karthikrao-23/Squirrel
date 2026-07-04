import { useMemo, useState } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  Cell,
  Line,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useQueryClient } from "@tanstack/react-query";
import { keys, useAccounts, useHoldings, usePortfolioHistory, useSummary } from "../api/hooks";
import type { AccountKind, Holding } from "../api/types";
import { ConnectInstitutionButton } from "../components/ConnectInstitution";
import { Card, CardHead, Disclaimer, ErrorState, Money, Spinner, Stat } from "../components/ui";
import { fmtDate, money, num, qty } from "../lib/format";

const DONUT_COLORS = ["#1f6feb", "#1a8a55", "#b7791f", "#8a93a1", "#7c5cff", "#0ea5a3"];
// Level-1 (by kind) colors: taxable = brand blue, retirement = violet.
const KIND_COLORS: Record<AccountKind, string> = { taxable: "#1f6feb", retirement: "#7c5cff" };
const KIND_LABEL: Record<AccountKind, string> = { taxable: "Taxable", retirement: "Retirement" };
// Securities shown per drill tier before the rest fold into a clickable "Other".
const SLICES_PER_TIER = 5;

/** One allocation donut slice. `onClick` present ⇒ the slice drills further. */
interface AllocSlice {
  name: string;
  value: number;
  color: string;
  onClick?: () => void;
}

function holdingValue(h: Holding): number {
  return num(h.institution_value);
}
function holdingGain(h: Holding): number {
  return num(h.institution_value) - num(h.cost_basis);
}
function securityName(h: Holding): string {
  return h.ticker ?? h.security_name ?? "—";
}

interface KindTotals {
  value: number;
  basis: number;
  unrealized: number;
}
const ZERO: KindTotals = { value: 0, basis: 0, unrealized: 0 };

// ---- Holdings sorting -------------------------------------------------------
type SortKey = "security" | "account" | "qty" | "price" | "value" | "basis" | "unrealized";
interface SortState {
  key: SortKey;
  dir: "asc" | "desc";
}
const COLUMNS: { key: SortKey; label: string; right?: boolean }[] = [
  { key: "security", label: "Security" },
  { key: "account", label: "Account" },
  { key: "qty", label: "Qty", right: true },
  { key: "price", label: "Price", right: true },
  { key: "value", label: "Market value", right: true },
  { key: "basis", label: "Cost basis", right: true },
  { key: "unrealized", label: "Unrealized", right: true },
];

/** Sort key → comparable value. Text columns sort case-insensitively; the rest numerically. */
function sortValue(h: Holding, key: SortKey): number | string {
  switch (key) {
    case "security":
      return securityName(h).toLowerCase();
    case "account":
      return h.account_name.toLowerCase();
    case "qty":
      return num(h.quantity);
    case "price":
      return num(h.institution_price);
    case "value":
      return holdingValue(h);
    case "basis":
      return num(h.cost_basis);
    case "unrealized":
      return holdingGain(h);
  }
}

export function Dashboard() {
  const qc = useQueryClient();
  const summary = useSummary();
  const holdings = useHoldings();
  const accounts = useAccounts();
  const history = usePortfolioHistory();

  // Allocation drill state: null = top level (by kind), else a kind we've drilled
  // into. `drillOffset` pages through the securities within that kind — clicking
  // the "Other" slice advances it to reveal the next tier.
  const [drill, setDrill] = useState<AccountKind | null>(null);
  const [drillOffset, setDrillOffset] = useState(0);
  const [sort, setSort] = useState<SortState>({ key: "value", dir: "desc" });

  const series = useMemo(
    () =>
      (history.data ?? []).map((s) => ({
        as_of: s.as_of,
        market_value: num(s.market_value),
        cost_basis: num(s.cost_basis),
      })),
    [history.data],
  );

  // Each holding's account kind (taxable/retirement), for the split + drill-down.
  const kindByAccount = useMemo(() => {
    const m = new Map<string, AccountKind>();
    for (const a of accounts.data ?? []) m.set(a.id, a.kind);
    return m;
  }, [accounts.data]);

  const holdingKind = (h: Holding): AccountKind => kindByAccount.get(h.account_id) ?? "taxable";

  // Value / cost-basis / unrealized split by account kind.
  const byKind = useMemo(() => {
    const acc: Record<AccountKind, KindTotals> = { taxable: { ...ZERO }, retirement: { ...ZERO } };
    for (const h of holdings.data ?? []) {
      const t = acc[holdingKind(h)];
      t.value += holdingValue(h);
      t.basis += num(h.cost_basis);
      t.unrealized += holdingGain(h);
    }
    return acc;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [holdings.data, kindByAccount]);

  // Level 1 of the allocation chart: one slice per kind that holds value.
  const allocByKind = useMemo(
    () =>
      (["taxable", "retirement"] as AccountKind[])
        .map((kind) => ({ kind, name: KIND_LABEL[kind], value: byKind[kind].value }))
        .filter((r) => r.value > 0),
    [byKind],
  );

  // Level 2: every security within the drilled-into kind, largest first, merging
  // the same security held across multiple accounts. Paged into tiers below.
  const allocSecurities = useMemo(() => {
    if (!drill) return [];
    const merged = new Map<string, number>();
    for (const h of holdings.data ?? []) {
      if (holdingKind(h) !== drill) continue;
      const v = holdingValue(h);
      if (v <= 0) continue;
      merged.set(securityName(h), (merged.get(securityName(h)) ?? 0) + v);
    }
    return [...merged].map(([name, value]) => ({ name, value })).sort((a, b) => b.value - a.value);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drill, holdings.data, kindByAccount]);

  const sortedHoldings = useMemo(() => {
    const arr = [...(holdings.data ?? [])];
    arr.sort((a, b) => {
      const va = sortValue(a, sort.key);
      const vb = sortValue(b, sort.key);
      const cmp =
        typeof va === "number" ? va - (vb as number) : String(va).localeCompare(String(vb));
      return sort.dir === "asc" ? cmp : -cmp;
    });
    return arr;
  }, [holdings.data, sort]);

  const toggleSort = (key: SortKey) =>
    setSort((s) =>
      s.key === key
        ? { key, dir: s.dir === "asc" ? "desc" : "asc" }
        : // Text columns default A→Z; numeric columns default high→low.
          { key, dir: key === "security" || key === "account" ? "asc" : "desc" },
    );

  // The slices to draw, each with its own click behavior. Top level = one per
  // kind (drills in). Drilled = a tier of securities; the trailing "Other" slice
  // pages to the next tier so the long tail is explorable, not a dead end.
  const slices: AllocSlice[] = useMemo(() => {
    if (!drill) {
      return allocByKind.map((r) => ({
        name: r.name,
        value: r.value,
        color: KIND_COLORS[r.kind],
        onClick: () => {
          setDrill(r.kind);
          setDrillOffset(0);
        },
      }));
    }
    const tier: AllocSlice[] = allocSecurities
      .slice(drillOffset, drillOffset + SLICES_PER_TIER)
      .map((r, i) => ({
        name: r.name,
        value: r.value,
        color: DONUT_COLORS[i % DONUT_COLORS.length],
      }));
    const rest = allocSecurities.slice(drillOffset + SLICES_PER_TIER);
    if (rest.length > 0) {
      tier.push({
        name: `Other (${rest.length})`,
        value: rest.reduce((s, r) => s + r.value, 0),
        color: DONUT_COLORS[SLICES_PER_TIER % DONUT_COLORS.length],
        onClick: () => setDrillOffset((o) => o + SLICES_PER_TIER),
      });
    }
    return tier;
  }, [drill, drillOffset, allocByKind, allocSecurities]);

  // Back steps through the "Other" tiers first, then up to the kind overview.
  const goBack = () => {
    if (drillOffset > 0) setDrillOffset((o) => Math.max(0, o - SLICES_PER_TIER));
    else setDrill(null);
  };

  const hasHoldings = (holdings.data ?? []).length > 0;
  const anyClickable = slices.some((s) => s.onClick);

  return (
    <div className="app">
      <div className="page-head flex between">
        <div>
          <h1>Dashboard</h1>
          <p>{summary.data ? `As of ${fmtDate(summary.data.as_of)}` : ""}</p>
        </div>
        <div className="flex" style={{ gap: 8 }}>
          <ConnectInstitutionButton />
          <button
            className="btn"
            onClick={() => {
              qc.invalidateQueries({ queryKey: keys.summary });
              qc.invalidateQueries({ queryKey: keys.holdings });
              qc.invalidateQueries({ queryKey: keys.accounts });
              qc.invalidateQueries({ queryKey: keys.portfolioHistory });
            }}
          >
            ↻ Refresh
          </button>
        </div>
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

      {/* Taxable vs retirement split — holdings grouped by account kind. Retirement
          accounts are tax-advantaged, so their line reads as return, not tax. */}
      {hasHoldings ? (
        <div className="grid cols-2 mt16">
          <Stat
            label="Taxable"
            value={money(byKind.taxable.value)}
            sub={<>unrealized <Money value={byKind.taxable.unrealized} /></>}
          />
          <Stat
            label="Retirement"
            value={money(byKind.retirement.value)}
            sub={
              byKind.retirement.value > 0 ? (
                <>return <Money value={byKind.retirement.unrealized} /></>
              ) : (
                "no retirement holdings"
              )
            }
          />
        </div>
      ) : null}

      <div className="grid dash mt16">
        {/* Value-over-time — GET /api/portfolio/history (daily snapshots). */}
        <Card>
          <CardHead title="Portfolio value" />
          <div className="card-body">
            {history.isLoading ? (
              <Spinner />
            ) : history.isError ? (
              <ErrorState error={history.error} onRetry={() => history.refetch()} />
            ) : series.length < 2 ? (
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
                <div className="faint">
                  Building history — a snapshot is recorded once per day.
                </div>
              </div>
            ) : (
              <div style={{ height: 200 }}>
                <ResponsiveContainer>
                  <AreaChart data={series} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
                    <defs>
                      <linearGradient id="mvFill" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="0%" stopColor="#1f6feb" stopOpacity={0.35} />
                        <stop offset="100%" stopColor="#1f6feb" stopOpacity={0} />
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
                      formatter={(v: number, name) => [
                        money(v, 2),
                        name === "market_value" ? "Market value" : "Cost basis",
                      ]}
                    />
                    <Area
                      type="monotone"
                      dataKey="market_value"
                      stroke="#1f6feb"
                      strokeWidth={2}
                      fill="url(#mvFill)"
                    />
                    <Line
                      type="monotone"
                      dataKey="cost_basis"
                      stroke="#8a93a1"
                      strokeWidth={1.5}
                      strokeDasharray="4 3"
                      dot={false}
                    />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            )}
          </div>
        </Card>

        {/* Allocation donut — GET /api/holdings. Drills by kind → securities, and
            the trailing "Other" slice pages through the remaining securities. */}
        <Card>
          <CardHead
            title={drill ? `Allocation · ${KIND_LABEL[drill]}` : "Allocation"}
            right={
              drill ? (
                <button className="btn sm" onClick={goBack}>
                  ← Back
                </button>
              ) : slices.length > 0 ? (
                <span className="faint" style={{ fontSize: 12 }}>
                  click a slice to drill in
                </span>
              ) : undefined
            }
          />
          <div className="card-body flex" style={{ gap: 20 }}>
            {holdings.isLoading ? (
              <Spinner />
            ) : slices.length === 0 ? (
              <div className="faint">No priced holdings.</div>
            ) : (
              <>
                {/* Fixed, non-shrinking box so the long legend can't squeeze the
                    chart; percentage radii so the donut scales to fill it. */}
                <div
                  style={{ flex: "0 0 130px", height: 130, cursor: anyClickable ? "pointer" : "default" }}
                >
                  <ResponsiveContainer>
                    <PieChart>
                      <Pie
                        data={slices}
                        dataKey="value"
                        innerRadius="55%"
                        outerRadius="98%"
                        paddingAngle={2}
                        onClick={(_, i) => slices[i]?.onClick?.()}
                      >
                        {slices.map((s, i) => (
                          <Cell key={i} fill={s.color} />
                        ))}
                      </Pie>
                      {/* pointerEvents:none so the hover tooltip can't sit over a
                          slice and swallow the click that would drill into it. */}
                      <Tooltip
                        formatter={(v: number) => money(v)}
                        wrapperStyle={{ pointerEvents: "none" }}
                      />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                <div className="legend" style={{ flex: 1, minWidth: 0 }}>
                  {slices.map((s) => {
                    const body = (
                      <>
                        <span className="sw" style={{ background: s.color, flexShrink: 0 }} />
                        <span style={{ minWidth: 0, overflowWrap: "anywhere", flex: 1 }}>
                          {s.name} · {money(s.value)}
                        </span>
                        {s.onClick ? (
                          <span className="drill-caret" aria-hidden>
                            ›
                          </span>
                        ) : null}
                      </>
                    );
                    // A clickable slice (a kind, or the "Other" tail) is a button —
                    // a reliable target no matter how thin its pie wedge is.
                    return s.onClick ? (
                      <button type="button" className="row clickable" key={s.name} onClick={s.onClick}>
                        {body}
                      </button>
                    ) : (
                      <div className="row" key={s.name}>
                        {body}
                      </div>
                    );
                  })}
                </div>
              </>
            )}
          </div>
        </Card>
      </div>

      {/* Holdings — GET /api/holdings. Click a column header to sort. */}
      <Card className="mt16">
        <CardHead title="Holdings" />
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
                {COLUMNS.map((c) => {
                  const active = sort.key === c.key;
                  return (
                    <th key={c.key} className={c.right ? "r" : ""} aria-sort={active ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
                      <button type="button" className="th-sort" onClick={() => toggleSort(c.key)}>
                        {c.label}
                        <span className="sort-caret">{active ? (sort.dir === "asc" ? "▲" : "▼") : "↕"}</span>
                      </button>
                    </th>
                  );
                })}
              </tr>
            </thead>
            <tbody>
              {sortedHoldings.map((h) => (
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
