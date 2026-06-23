import { useState } from "react";
import { useAlerts, useMarkRead } from "../api/hooks";
import type { Alert } from "../api/types";
import { ApiPill, Card, EmptyState, ErrorState, Spinner } from "../components/ui";
import { money, relativeTime } from "../lib/format";

function isLongTerm(a: Alert) {
  return a.type === "approaching_long_term";
}

/** Pull the estimated saving out of the alert payload, if present. */
function saving(a: Alert): number | null {
  const v = (a.payload as Record<string, unknown>)?.estimated_tax_saving;
  if (typeof v === "number") return v;
  if (typeof v === "string") return parseFloat(v);
  return null;
}

export function Alerts() {
  const [unreadOnly, setUnreadOnly] = useState(false);
  const alerts = useAlerts(unreadOnly);
  const markRead = useMarkRead();

  return (
    <div className="app" style={{ maxWidth: 820 }}>
      <div className="page-head">
        <h1>Alerts</h1>
        <p>
          Tax-aware sell &amp; harvest signals from the nightly scheduler. <ApiPill>GET /api/alerts</ApiPill>
        </p>
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
            return (
              <div className={`alert-item ${unread ? "unread" : ""}`} key={a.id}>
                {unread ? <span className="unread-dot" /> : <span style={{ width: 8, flex: "none" }} />}
                <div className={`alert-ic ${isLongTerm(a) ? "lt" : "loss"}`}>
                  {isLongTerm(a) ? "⏳" : "📉"}
                </div>
                <div className="alert-body">
                  <div className={`t ${unread ? "" : "muted"}`}>{a.title}</div>
                  <div className="m">{a.message}</div>
                  <div className="alert-meta">
                    <span className={`chip ${isLongTerm(a) ? "lt" : "loss"}`}>{a.type}</span>
                    {s != null && <span className="gain">+{money(s)} saving</span>}
                    <span>· {relativeTime(a.created_at)}</span>
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
