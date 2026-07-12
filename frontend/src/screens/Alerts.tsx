import { useState } from "react";
import { useAlerts, useEvaluateAlerts, useMarkRead } from "../api/hooks";
import type { Alert } from "../api/types";
import { Card, EmptyState, ErrorState, Spinner } from "../components/ui";
import { money, relativeTime } from "../lib/format";

/** Icon, colour class, and chip label for each alert type. */
function look(a: Alert): { icon: string; ic: string; chip: string; label: string } {
  switch (a.type) {
    case "approaching_long_term":
      return { icon: "⏳", ic: "lt", chip: "lt", label: "approaching long-term" };
    case "missed_harvest":
      return { icon: "⚠️", ic: "missed", chip: "warn", label: "missed harvest" };
    case "harvestable_loss":
    default:
      return { icon: "📉", ic: "loss", chip: "loss", label: "harvestable loss" };
  }
}

/** Pull the estimated saving out of the alert payload, if present. */
function saving(a: Alert): number | null {
  const v = (a.payload as Record<string, unknown>)?.estimated_tax_saving;
  if (typeof v === "number") return v;
  if (typeof v === "string") return parseFloat(v);
  return null;
}

/** A string field from the alert payload (e.g. a date), if present. */
function payloadStr(a: Alert, key: string): string | null {
  const v = (a.payload as Record<string, unknown>)?.[key];
  return typeof v === "string" ? v : null;
}

export function Alerts() {
  const [unreadOnly, setUnreadOnly] = useState(false);
  const alerts = useAlerts(unreadOnly);
  const markRead = useMarkRead();
  const evaluate = useEvaluateAlerts();

  return (
    <div className="app" style={{ maxWidth: 820 }}>
      <div className="page-head">
        <h1>Alerts</h1>
        <p>Tax-aware sell &amp; harvest signals, refreshed automatically as your data updates.</p>
      </div>

      <div className="toolbar">
        <div className="toggle">
          <button className={!unreadOnly ? "on" : ""} onClick={() => setUnreadOnly(false)}>
            All
          </button>
          <button className={unreadOnly ? "on" : ""} onClick={() => setUnreadOnly(true)}>
            Unread
          </button>
        </div>
        <span className="faint">{unreadOnly ? "?unread_only=true" : ""}</span>
        <button
          className="btn"
          style={{ marginLeft: "auto" }}
          disabled={evaluate.isPending}
          onClick={() => evaluate.mutate()}
        >
          {evaluate.isPending ? "Refreshing…" : "Refresh alerts"}
        </button>
      </div>

      <Card>
        {alerts.isLoading ? (
          <Spinner />
        ) : alerts.isError ? (
          <ErrorState error={alerts.error} onRetry={() => alerts.refetch()} />
        ) : (alerts.data ?? []).length === 0 ? (
          <EmptyState
            title={unreadOnly ? "No unread alerts" : "You're all caught up"}
            hint="New tax-timing and harvest signals will appear here."
          />
        ) : (
          alerts.data!.map((a) => {
            const unread = a.read_at == null;
            const s = saving(a);
            const l = look(a);
            const missed = a.type === "missed_harvest";
            const missedOn = payloadStr(a, "missed_on");
            return (
              <div className={`alert-item ${unread ? "unread" : ""}`} key={a.id}>
                {unread ? <span className="unread-dot" /> : <span style={{ width: 8, flex: "none" }} />}
                <div className={`alert-ic ${l.ic}`}>{l.icon}</div>
                <div className="alert-body">
                  <div className={`t ${unread ? "" : "muted"}`}>{a.title}</div>
                  <div className="m">{a.message}</div>
                  <div className="alert-meta">
                    <span className={`chip ${l.chip}`}>{l.label}</span>
                    {s != null &&
                      (missed ? (
                        <span className="faint">~{money(s)} missed</span>
                      ) : (
                        <span className="gain">+{money(s)} saving</span>
                      ))}
                    {missed && missedOn && <span className="faint">· missed {missedOn}</span>}
                    <span>· {relativeTime(a.updated_at)}</span>
                    {a.emailed_at ? <span>· ✉ emailed</span> : <span className="faint">· not emailed</span>}
                  </div>
                </div>
                {unread && (
                  <button className="btn" disabled={markRead.isPending} onClick={() => markRead.mutate(a.id)}>
                    Mark read
                  </button>
                )}
              </div>
            );
          })
        )}
      </Card>

      <p className="foot">Also delivered by email (lettre). Estimates are <strong>not tax advice</strong>.</p>
    </div>
  );
}
