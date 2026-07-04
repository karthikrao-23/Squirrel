// TanStack Query hooks — one per endpoint in PLAN.md §5. Query keys are simple
// and stable so mutations can invalidate precisely.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { del, get, patch, post } from "./client";
import type {
  Account,
  AccountKindOverride,
  AccountLotsResp,
  Alert,
  Connection,
  ConnectResponse,
  HarvestCandidate,
  Holding,
  LinkTokenResp,
  PortfolioSnapshot,
  RetirementSummary,
  SimulateReq,
  SimulateResp,
  TaxSummary,
  User,
} from "./types";

export const keys = {
  me: ["me"] as const,
  profile: ["profile"] as const,
  accounts: ["accounts"] as const,
  accountLots: ["accounts", "lots"] as const,
  connections: ["plaid", "connections"] as const,
  holdings: ["holdings"] as const,
  portfolioHistory: ["portfolio", "history"] as const,
  retirement: ["retirement"] as const,
  summary: ["tax", "summary"] as const,
  harvest: ["tax", "harvest"] as const,
  alerts: (unreadOnly: boolean) => ["alerts", { unreadOnly }] as const,
};

// ---- Auth ----
type Credentials = { email: string; password: string };

/** The current user, or an error (401 → `UnauthorizedError`) when not logged in.
 *  Drives `<AuthGate>`. We never retry it: a 401 is an answer, not a failure. */
export const useMe = () =>
  useQuery({ queryKey: keys.me, queryFn: () => get<User>("/api/auth/me"), retry: false });

export function useSignup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: Credentials) => post<User>("/api/auth/signup", body),
    onSuccess: (user) => onAuthed(qc, user),
  });
}

export function useLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: Credentials) => post<User>("/api/auth/login", body),
    onSuccess: (user) => onAuthed(qc, user),
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => post<void>("/api/auth/logout"),
    onSuccess: () => onLoggedOut(qc),
  });
}

export function useLogoutAll() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => post<void>("/api/auth/logout-all"),
    onSuccess: () => onLoggedOut(qc),
  });
}

/** After logout: flip `me` to logged-out and drop every other cached query.
 *  We *set* `me` to null (rather than `qc.clear()`) because clearing removes the
 *  query without pushing an update to its active observer, so `<AuthGate>` would
 *  keep rendering the stale logged-in user. setQueryData notifies the observer,
 *  which re-renders straight to the login screen — mirroring how `onAuthed`
 *  seeds `me` on login. */
function onLoggedOut(qc: ReturnType<typeof useQueryClient>) {
  qc.setQueryData(keys.me, null);
  qc.removeQueries({ predicate: (q) => q.queryKey[0] !== "me" });
}

/** After a successful login/signup: seed `me`, then drop every other cached
 *  query so nothing from a previous user (or a logged-out state) lingers. */
function onAuthed(qc: ReturnType<typeof useQueryClient>, user: User) {
  qc.setQueryData(keys.me, user);
  qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "me" });
}

// ---- Profile ----
export const useProfile = () =>
  useQuery({ queryKey: keys.profile, queryFn: () => get<User>("/api/profile") });

export function useUpdateProfile() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: { filing_status?: string; taxable_income?: string }) =>
      patch<User>("/api/profile", body),
    onSuccess: (user) => {
      qc.setQueryData(keys.profile, user);
      qc.invalidateQueries({ queryKey: keys.summary });
      qc.invalidateQueries({ queryKey: keys.harvest });
    },
  });
}

// ---- Portfolio ----
export const useAccounts = () =>
  useQuery({
    queryKey: keys.accounts,
    queryFn: () => get<{ accounts: Account[] }>("/api/accounts").then((r) => r.accounts),
  });

export const useAccountLots = () =>
  useQuery({
    queryKey: keys.accountLots,
    queryFn: () => get<AccountLotsResp>("/api/accounts/lots"),
  });

/** Override an account's tax classification (or clear it back to auto with
 *  `kind: null`). The kind gates harvest candidates, the retirement view, and
 *  the dashboard's taxable/retirement split, so refresh all of them. */
export function useSetAccountKind() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, kind }: { id: string; kind: AccountKindOverride }) =>
      patch<{ account_id: string; kind: string; kind_override: AccountKindOverride }>(
        `/api/accounts/${id}/kind`,
        { kind },
      ),
    onSuccess: () => {
      for (const key of [
        keys.accountLots,
        keys.accounts,
        keys.connections,
        keys.summary,
        keys.harvest,
        keys.retirement,
      ]) {
        qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

export const useHoldings = () =>
  useQuery({
    queryKey: keys.holdings,
    queryFn: () => get<{ holdings: Holding[] }>("/api/holdings").then((r) => r.holdings),
  });

export const useConnections = () =>
  useQuery({
    queryKey: keys.connections,
    queryFn: () =>
      get<{ connections: Connection[] }>("/api/plaid/items").then((r) => r.connections),
  });

/** Remove a Plaid connection; its accounts/holdings/transactions/lots go with
 *  it, so refresh everything the portfolio derives from them. */
export function useRemoveConnection() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => del<{ removed: boolean }>(`/api/plaid/items/${id}`),
    onSuccess: () => {
      for (const key of [
        keys.connections,
        keys.accounts,
        keys.accountLots,
        keys.holdings,
        keys.summary,
        keys.harvest,
        keys.retirement,
        keys.portfolioHistory,
      ]) {
        qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

export const usePortfolioHistory = () =>
  useQuery({
    queryKey: keys.portfolioHistory,
    queryFn: () =>
      get<{ history: PortfolioSnapshot[] }>("/api/portfolio/history").then((r) => r.history),
  });

export const useRetirement = () =>
  useQuery({
    queryKey: keys.retirement,
    queryFn: () => get<RetirementSummary>("/api/retirement"),
  });

// ---- Tax ----
export const useSummary = () =>
  useQuery({ queryKey: keys.summary, queryFn: () => get<TaxSummary>("/api/tax/summary") });

export const useHarvest = () =>
  useQuery({
    queryKey: keys.harvest,
    queryFn: () =>
      get<{ candidates: HarvestCandidate[] }>("/api/tax/harvest").then((r) => r.candidates),
  });

export const useSimulate = () =>
  useMutation({
    mutationFn: (body: SimulateReq) => post<SimulateResp>("/api/tax/simulate", body),
  });

// ---- Alerts ----
export const useAlerts = (unreadOnly = false) =>
  useQuery({
    queryKey: keys.alerts(unreadOnly),
    queryFn: () =>
      get<{ alerts: Alert[] }>(
        `/api/alerts${unreadOnly ? "?unread_only=true" : ""}`,
      ).then((r) => r.alerts),
  });

export function useMarkRead() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => post<{ ok: boolean }>(`/api/alerts/${id}/read`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["alerts"] }),
  });
}

// ---- Onboarding (Plaid) ----

/** Invalidate everything the dashboard derives from a fresh sync. */
function invalidateAfterSync(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: keys.accounts });
  qc.invalidateQueries({ queryKey: keys.holdings });
  qc.invalidateQueries({ queryKey: keys.summary });
  qc.invalidateQueries({ queryKey: keys.harvest });
}

/** Fetch a Plaid Link token (only when `enabled`). Link tokens are short-lived
 *  and single-use, so this isn't cached. */
export const useLinkToken = (enabled: boolean) =>
  useQuery({
    queryKey: ["plaid", "link-token"],
    queryFn: () => post<LinkTokenResp>("/api/plaid/link-token"),
    enabled,
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });

/** Exchange a Plaid public token; the backend stores the item and runs the
 *  initial sync inline, so success means the portfolio is ready. */
export function useExchange() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (publicToken: string) =>
      post<ConnectResponse>("/api/plaid/exchange", { public_token: publicToken }),
    onSuccess: () => invalidateAfterSync(qc),
  });
}

// Dev-only shortcut: mints + exchanges + syncs a sandbox item in one call, so
// the flow can be exercised locally without opening Plaid Link.
export function useSandboxConnect() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => post<ConnectResponse>("/api/plaid/sandbox/connect"),
    onSuccess: () => invalidateAfterSync(qc),
  });
}
