import { useMemo, useState } from "react";
import {
  useAccountLots,
  useAccounts,
  useConnections,
  useRemoveConnection,
  useSetAccountKind,
} from "../api/hooks";
import type {
  AccountBalanceOnly,
  AccountKind,
  AccountKindOverride,
  AccountLot,
} from "../api/types";
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
import { fmtDate, money, num, qty, relativeTime } from "../lib/format";

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
  account_kind: AccountKind;
  account_kind_override: AccountKindOverride;
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
        account_kind: lot.account_kind,
        account_kind_override: lot.account_kind_override,
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

const KIND_LABELS: Record<AccountKind, string> = {
  taxable: "Taxable",
  retirement: "Retirement",
  debt: "Debt",
};

const KIND_OPTIONS: { value: AccountKindOverride; label: string }[] = [
  { value: null, label: "Auto" },
  { value: "taxable", label: "Taxable" },
  { value: "retirement", label: "Retirement" },
  { value: "debt", label: "Debt" },
];

/** Small header badge showing an account's effective classification. */
function KindChip({ kind }: { kind: AccountKind }) {
  return <span className={`chip ${kind}`}>{KIND_LABELS[kind]}</span>;
}

/** Faint "Synced 3h ago" label from an account's last successful refresh.
 *  `null`/absent means the account hasn't completed a sync yet. */
function SyncBadge({ iso }: { iso?: string | null }) {
  return (
    <span className="faint" style={{ fontSize: 12 }} title={iso ? fmtDate(iso) : undefined}>
      {iso ? `Synced ${relativeTime(iso)}` : "Not synced yet"}
    </span>
  );
}

/** Segmented Auto / Taxable / Retirement / Debt control that overrides an
 *  account's classification. "Auto" (override = null) derives the kind from
 *  Plaid's subtype; the others pin it. Debt marks a liability that's excluded
 *  from portfolio value. */
function KindControl({
  accountId,
  override,
  resolvedKind,
}: {
  accountId: string;
  override: AccountKindOverride;
  resolvedKind: AccountKind;
}) {
  const setKind = useSetAccountKind();
  const busy = setKind.isPending && setKind.variables?.id === accountId;

  return (
    <div className="kind-control">
      <span className="faint" style={{ fontSize: 12 }}>
        Account type
      </span>
      <div className="segmented" role="group" aria-label="Account type">
        {KIND_OPTIONS.map((opt) => {
          const active = override === opt.value;
          return (
            <button
              key={opt.label}
              type="button"
              className={`seg ${active ? "active" : ""}`}
              aria-pressed={active}
              disabled={busy || active}
              onClick={() => setKind.mutate({ id: accountId, kind: opt.value })}
            >
              {opt.label}
            </button>
          );
        })}
      </div>
      <span className="faint" style={{ fontSize: 12 }}>
        {override === null
          ? `Auto → ${KIND_LABELS[resolvedKind]}`
          : resolvedKind === "debt"
            ? "Manually set · excluded from portfolio value"
            : "Manually set"}
      </span>
      {setKind.isError && (
        <span className="loss" style={{ fontSize: 12 }}>
          {(setKind.error as Error).message}
        </span>
      )}
    </div>
  );
}

function AccountCard({
  group,
  lastSyncedAt,
}: {
  group: AccountGroup;
  lastSyncedAt?: string | null;
}) {
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
          <KindChip kind={group.account_kind} />
          <SyncBadge iso={lastSyncedAt} />
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
          <div className="mt16" style={{ marginBottom: 8 }}>
            <KindControl
              accountId={group.account_id}
              override={group.account_kind_override}
              resolvedKind={group.account_kind}
            />
          </div>
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

/** Manage Plaid connections. A duplicate connection (same institution linked
 *  twice) shows up as two rows here; removing one deletes its accounts + lots. */
function Connections() {
  const connections = useConnections();
  const remove = useRemoveConnection();
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const list = connections.data ?? [];
  if (list.length === 0) return null; // nothing to manage (or still loading)

  // Flag connections that share an institution — likely the accidental re-link.
  const instCounts = new Map<string, number>();
  for (const c of list) {
    const key = c.institution_id ?? c.institution_name ?? c.id;
    instCounts.set(key, (instCounts.get(key) ?? 0) + 1);
  }

  return (
    <Card className="mt16">
      <CardHead title="Connections" />
      <div className="card-body">
        <p className="faint" style={{ marginTop: 0, fontSize: 13 }}>
          Each row is one Plaid link. Removing a connection also removes its accounts and their
          tax lots, and disconnects it from Plaid.
        </p>
        {list.map((c) => {
          const key = c.institution_id ?? c.institution_name ?? c.id;
          const dup = (instCounts.get(key) ?? 0) > 1;
          const confirming = confirmId === c.id;
          const busy = remove.isPending && remove.variables === c.id;
          const n = c.accounts.length;
          return (
            <div key={c.id} className="conn-row">
              <div className="conn-info">
                <div className="conn-title">
                  <strong>{c.institution_name ?? "Connected institution"}</strong>
                  {dup && <span className="chip warn">possible duplicate</span>}
                </div>
                <div className="faint" style={{ fontSize: 12 }}>
                  Linked {fmtDate(c.created_at)} ·{" "}
                  {n === 0 ? "no accounts" : c.accounts.map((a) => a.name).join(", ")}
                </div>
              </div>
              {confirming ? (
                <div className="flex">
                  <span className="faint" style={{ fontSize: 12, alignSelf: "center" }}>
                    Remove {n} account{n === 1 ? "" : "s"}?
                  </span>
                  <button
                    className="btn danger sm"
                    disabled={busy}
                    onClick={() => remove.mutate(c.id, { onSuccess: () => setConfirmId(null) })}
                  >
                    {busy ? "Removing…" : "Remove"}
                  </button>
                  <button className="btn sm" disabled={busy} onClick={() => setConfirmId(null)}>
                    Cancel
                  </button>
                </div>
              ) : (
                <button className="btn sm" onClick={() => setConfirmId(c.id)}>
                  Disconnect
                </button>
              )}
            </div>
          );
        })}
        {remove.isError && (
          <div className="loss mt16" style={{ fontSize: 13 }}>
            {(remove.error as Error).message}
          </div>
        )}
      </div>
    </Card>
  );
}

/** An account we can't break into lots — Plaid won't share its holdings — so we
 *  show its Plaid balance as the value. */
function BalanceOnlyCard({
  account,
  lastSyncedAt,
}: {
  account: AccountBalanceOnly;
  lastSyncedAt?: string | null;
}) {
  return (
    <Card className="mt16">
      <div className="collapse-head" style={{ cursor: "default" }}>
        <div className="collapse-title">
          <h2>{account.name}</h2>
          {account.subtype && <span className="chip">{account.subtype}</span>}
          <KindChip kind={account.kind} />
          <SyncBadge iso={lastSyncedAt} />
        </div>
        <div className="collapse-summary">
          <span className="num">{money(account.current_balance)}</span>
        </div>
      </div>
      <div className="card-body">
        <p className="faint" style={{ margin: "0 0 12px", fontSize: 13 }}>
          Value from Plaid's account balance. This institution doesn't share per-position holdings,
          so lot-level detail (cost basis, gains, harvesting) isn't available for this account.
        </p>
        <KindControl
          accountId={account.account_id}
          override={account.kind_override}
          resolvedKind={account.kind}
        />
      </div>
    </Card>
  );
}

export function Accounts() {
  const accountLots = useAccountLots();
  const accounts = useAccounts();
  const lots = accountLots.data?.lots ?? [];
  const balanceOnly = accountLots.data?.balance_only ?? [];
  const groups = useMemo(() => groupByAccount(lots), [accountLots.data]);

  // Last-synced time lives on the account record (GET /api/accounts), but the
  // cards above are built from lots / balances — so join by account id.
  const syncedById = useMemo(
    () => new Map((accounts.data ?? []).map((a) => [a.id, a.last_synced_at])),
    [accounts.data],
  );

  return (
    <div className="app">
      <div className="page-head">
        <h1>Accounts</h1>
        <p>Open tax lots held in each connected account, with per-account totals.</p>
      </div>

      <Connections />

      {accountLots.isLoading ? (
        <Card>
          <Spinner />
        </Card>
      ) : accountLots.isError ? (
        <Card>
          <ErrorState error={accountLots.error} onRetry={() => accountLots.refetch()} />
        </Card>
      ) : groups.length === 0 && balanceOnly.length === 0 ? (
        <Card>
          <EmptyState
            title="No open lots"
            hint="Connect a brokerage or rebuild your lots to see holdings by account."
          />
        </Card>
      ) : (
        <>
          {groups.map((g) => (
            <AccountCard key={g.account_id} group={g} lastSyncedAt={syncedById.get(g.account_id)} />
          ))}
          {balanceOnly.map((a) => (
            <BalanceOnlyCard
              key={a.account_id}
              account={a}
              lastSyncedAt={syncedById.get(a.account_id)}
            />
          ))}
        </>
      )}

      <p className="foot">Showing open lots grouped by account · v1.</p>
    </div>
  );
}
