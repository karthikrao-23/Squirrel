import { useMemo, useState } from "react";
import { useAccountLots } from "../api/hooks";
import type { AccountLot } from "../api/types";
import {
  Card,
  EmptyState,
  ErrorState,
  Money,
  Spinner,
  Stat,
  TermChip,
} from "../components/ui";
import { fmtDate, money, num, qty } from "../lib/format";

/** Market value of a lot = remaining shares × current price. */
function marketValue(l: AccountLot): number {
  return num(l.remaining_quantity) * num(l.close_price);
}
/** Cost basis of a lot = remaining shares × per-share basis. */
function costBasis(l: AccountLot): number {
  return num(l.remaining_quantity) * num(l.cost_basis_per_share);
}
/** Unrealized gain/loss = market value − cost basis. */
function unrealized(l: AccountLot): number {
  return marketValue(l) - costBasis(l);
}
/** Year of a lot's open date ("YYYY-MM-DD" → "YYYY"). */
function lotYear(l: AccountLot): string {
  return l.open_date.slice(0, 4);
}

interface YearGroup {
  year: string;
  lots: AccountLot[];
  totalMarketValue: number;
}

interface AccountGroup {
  account_id: string;
  account_name: string;
  account_subtype: string | null;
  years: YearGroup[];
  totalMarketValue: number;
  totalCostBasis: number;
  totalUnrealized: number;
}

/** Group lots by account, then by open-date year within each account, and
 *  accumulate per-account totals in the same pass. Accounts are sorted by
 *  total market value (descending) and years newest-first. */
function groupByAccount(lots: AccountLot[]): AccountGroup[] {
  const byId = new Map<string, AccountGroup>();
  const yearsById = new Map<string, Map<string, YearGroup>>();

  for (const lot of lots) {
    let g = byId.get(lot.account_id);
    if (!g) {
      g = {
        account_id: lot.account_id,
        account_name: lot.account_name,
        account_subtype: lot.account_subtype,
        years: [],
        totalMarketValue: 0,
        totalCostBasis: 0,
        totalUnrealized: 0,
      };
      byId.set(lot.account_id, g);
      yearsById.set(lot.account_id, new Map());
    }
    g.totalMarketValue += marketValue(lot);
    g.totalCostBasis += costBasis(lot);
    g.totalUnrealized += unrealized(lot);

    const yearMap = yearsById.get(lot.account_id)!;
    const year = lotYear(lot);
    let yg = yearMap.get(year);
    if (!yg) {
      yg = { year, lots: [], totalMarketValue: 0 };
      yearMap.set(year, yg);
    }
    yg.lots.push(lot);
    yg.totalMarketValue += marketValue(lot);
  }

  for (const [accountId, yearMap] of yearsById) {
    byId.get(accountId)!.years = [...yearMap.values()].sort((a, b) =>
      b.year.localeCompare(a.year),
    );
  }

  return [...byId.values()].sort((a, b) => b.totalMarketValue - a.totalMarketValue);
}

/** Lots table for a single year. */
function LotsTable({ lots }: { lots: AccountLot[] }) {
  return (
    <table>
      <thead>
        <tr>
          <th>Security</th>
          <th>Opened</th>
          <th>Term</th>
          <th className="r">Qty</th>
          <th className="r">Price</th>
          <th className="r">Market value</th>
          <th className="r">Cost basis</th>
          <th className="r">Unrealized</th>
        </tr>
      </thead>
      <tbody>
        {lots.map((l) => (
          <tr key={l.id}>
            <td className="tick">{l.ticker ?? "—"}</td>
            <td className="num muted">{fmtDate(l.open_date)}</td>
            <td>
              <TermChip term={l.term} />
            </td>
            <td className="r num">{qty(l.remaining_quantity)}</td>
            <td className="r num">{l.close_price ? money(l.close_price, 2) : "—"}</td>
            <td className="r num">{money(marketValue(l), 0)}</td>
            <td className="r num">{money(costBasis(l), 0)}</td>
            <td className="r">
              <Money value={unrealized(l)} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <span className="chevron" aria-hidden>
      {open ? "▾" : "▸"}
    </span>
  );
}

function AccountCard({ group }: { group: AccountGroup }) {
  const [open, setOpen] = useState(false);
  const [openYears, setOpenYears] = useState<Set<string>>(new Set());

  const toggleYear = (year: string) =>
    setOpenYears((prev) => {
      const next = new Set(prev);
      next.has(year) ? next.delete(year) : next.add(year);
      return next;
    });

  return (
    <Card className="mt16">
      <button
        type="button"
        className="collapse-head"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <div className="collapse-title">
          <Chevron open={open} />
          <h2>{group.account_name}</h2>
          {group.account_subtype && <span className="chip">{group.account_subtype}</span>}
        </div>
        <div className="collapse-summary">
          <span className="muted">{money(group.totalMarketValue)}</span>
          <span className="muted">·</span>
          <span className="muted">{money(group.totalCostBasis)} basis</span>
          <span className="muted">·</span>
          <Money value={group.totalUnrealized} />
        </div>
      </button>

      {open && (
        <div className="collapse-body">
          <div className="grid cols-3">
            <Stat label="Market value" value={money(group.totalMarketValue)} />
            <Stat label="Cost basis" value={money(group.totalCostBasis)} />
            <Stat label="Unrealized" value={<Money value={group.totalUnrealized} />} />
          </div>

          {group.years.map((yg) => {
            const yearOpen = openYears.has(yg.year);
            return (
              <div className="year-group mt16" key={yg.year}>
                <button
                  type="button"
                  className="collapse-head sub"
                  aria-expanded={yearOpen}
                  onClick={() => toggleYear(yg.year)}
                >
                  <div className="collapse-title">
                    <Chevron open={yearOpen} />
                    <h3>{yg.year}</h3>
                    <span className="faint">
                      {yg.lots.length} {yg.lots.length === 1 ? "lot" : "lots"}
                    </span>
                  </div>
                  <span className="num muted">{money(yg.totalMarketValue)}</span>
                </button>
                {yearOpen && <LotsTable lots={yg.lots} />}
              </div>
            );
          })}
        </div>
      )}
    </Card>
  );
}

export function Accounts() {
  const accountLots = useAccountLots();
  const groups = useMemo(() => groupByAccount(accountLots.data ?? []), [accountLots.data]);

  return (
    <div className="app">
      <div className="page-head">
        <h1>Accounts</h1>
        <p>Open tax lots held in each connected account, with per-account totals.</p>
      </div>

      {accountLots.isLoading ? (
        <Card>
          <Spinner />
        </Card>
      ) : accountLots.isError ? (
        <Card>
          <ErrorState error={accountLots.error} onRetry={() => accountLots.refetch()} />
        </Card>
      ) : groups.length === 0 ? (
        <Card>
          <EmptyState
            title="No open lots"
            hint="Connect a brokerage or rebuild your lots to see holdings by account."
          />
        </Card>
      ) : (
        groups.map((g) => <AccountCard key={g.account_id} group={g} />)
      )}

      <p className="foot">Showing open lots grouped by account · v1.</p>
    </div>
  );
}
