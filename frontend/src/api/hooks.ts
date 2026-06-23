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
  profile: ["profile"] as const,
  accounts: ["accounts"] as const,
  holdings: ["holdings"] as const,
  summary: ["tax", "summary"] as const,
  harvest: ["tax", "harvest"] as const,
  alerts: (unreadOnly: boolean) => ["alerts", { unreadOnly }] as const,
};

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
