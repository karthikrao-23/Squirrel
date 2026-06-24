// TanStack Query hooks — one per endpoint in PLAN.md §5. Query keys are simple
// and stable so mutations can invalidate precisely.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { get, patch, post } from "./client";
import type {
  Account,
  Alert,
  ConnectResponse,
  HarvestCandidate,
  Holding,
  SimulateReq,
  SimulateResp,
  TaxSummary,
  User,
} from "./types";

export const keys = {
  me: ["me"] as const,
  profile: ["profile"] as const,
  accounts: ["accounts"] as const,
  holdings: ["holdings"] as const,
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
    onSuccess: () => qc.clear(),
  });
}

export function useLogoutAll() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => post<void>("/api/auth/logout-all"),
    onSuccess: () => qc.clear(),
  });
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

export const useHoldings = () =>
  useQuery({
    queryKey: keys.holdings,
    queryFn: () => get<{ holdings: Holding[] }>("/api/holdings").then((r) => r.holdings),
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
// Uses the sandbox shortcut endpoint, which mints + exchanges + syncs in one
// call — no Plaid Link JS needed to exercise the full flow with sandbox creds.
export function useSandboxConnect() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => post<ConnectResponse>("/api/plaid/sandbox/connect"),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.accounts });
      qc.invalidateQueries({ queryKey: keys.holdings });
      qc.invalidateQueries({ queryKey: keys.summary });
    },
  });
}
