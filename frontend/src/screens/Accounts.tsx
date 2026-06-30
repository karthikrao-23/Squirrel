import { useMemo } from "react";
import { useAccountLots } from "../api/hooks";
import type { AccountLot } from "../api/types";
import {
  Card,
  CardHead,
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

interface AccountGroup {
  account_id: string;
  account_name: string;
  account_subtype: string | null;
  lots: AccountLot[];
  totalMarketValue: number;
  totalCostBasis: number;
  totalUnrealized: number;
}

/** Group lots by account, preserving the backend's account-name ordering, and
 *  accumulate per-account totals in the same pass. */
function groupByAccount(lots: AccountLot[]): AccountGroup[] {
  const byId = new Map<string, AccountGroup>();
  for (const lot of lots) {
    let g = byId.get(lot.account_id);
    if (!g) {
      g = {
        account_id: lot.account_id,
        account_name: lot.account_name,
        account_subtype: lot.account_subtype,
        lots: [],
        totalMarketValue: 0,
        totalCostBasis: 0,
        totalUnrealized: 0,
      };
      byId.set(lot.account_id, g);
    }
    g.lots.push(lot);
    g.totalMarketValue += marketValue(lot);
    g.totalCostBasis += costBasis(lot);
    g.totalUnrealized += unrealized(lot);
  }
  return [...byId.values()];
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
        groups.map((g) => (
          <Card className="mt16" key={g.account_id}>
            <CardHead
              title={g.account_name}
              right={
                g.account_subtype ? <span className="chip">{g.account_subtype}</span> : undefined
              }
            />
            <div className="grid cols-3">
              <Stat label="Market value" value={money(g.totalMarketValue)} />
              <Stat label="Cost basis" value={money(g.totalCostBasis)} />
              <Stat label="Unrealized" value={<Money value={g.totalUnrealized} />} />
            </div>
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
                {g.lots.map((l) => (
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
          </Card>
        ))
      )}

      <p className="foot">Showing open lots grouped by account · v1.</p>
    </div>
  );
}
