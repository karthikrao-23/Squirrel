// Small presentational primitives mapping 1:1 to the M6 design-system classes
// in styles.css. Keeping them here means screens read declaratively.

import type { ReactNode } from "react";
import type { Term } from "../api/types";
import { num, signedMoney } from "../lib/format";

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`card ${className}`}>{children}</div>;
}

export function CardHead({ title, right }: { title: string; right?: ReactNode }) {
  return (
    <div className="card-head">
      <h2>{title}</h2>
      {right}
    </div>
  );
}

/** A faint monospace pill noting which endpoint backs a block (matches the mocks). */
export function ApiPill({ children }: { children: ReactNode }) {
  return <span className="api">{children}</span>;
}

export function Stat({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="card stat">
      <div className="label">{label}</div>
      <div className="value num">{value}</div>
      {sub != null && <div className="sub">{sub}</div>}
    </div>
  );
}

/** Signed money, colored by sign. */
export function Money({ value, dp = 0 }: { value: string | number | null | undefined; dp?: number }) {
  const n = num(value);
  return <span className={`num ${n > 0 ? "gain" : n < 0 ? "loss" : ""}`}>{signedMoney(value, dp)}</span>;
}

export function TermChip({ term }: { term: Term }) {
  return term === "long_term" ? (
    <span className="chip lt">Long-term</span>
  ) : (
    <span className="chip st">Short-term</span>
  );
}

export function WashSaleChip() {
  return <span className="chip warn dot">wash-sale risk</span>;
}

export function Disclaimer({ children }: { children?: ReactNode }) {
  return (
    <div className="disclaimer mt8">
      <span aria-hidden>⚠️</span>
      <div>
        {children ?? (
          <>
            Estimates for decision-support, <strong>not tax advice</strong>. Federal + California only (v1).
          </>
        )}
      </div>
    </div>
  );
}

export function Spinner({ label = "Loading…" }: { label?: string }) {
  return <div className="muted" style={{ padding: "32px 16px" }}>{label}</div>;
}

export function EmptyState({ title, hint }: { title: string; hint?: string }) {
  return (
    <div style={{ padding: "40px 16px", textAlign: "center" }}>
      <div style={{ fontWeight: 600 }}>{title}</div>
      {hint && <div className="faint mt8">{hint}</div>}
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const msg = error instanceof Error ? error.message : "Something went wrong";
  return (
    <div style={{ padding: "32px 16px", textAlign: "center" }}>
      <div className="loss" style={{ fontWeight: 600 }}>{msg}</div>
      {onRetry && (
        <button className="btn mt16" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}
