import type { Dec } from "../api/types";

/** Parse a Decimal-as-string (or number) into a JS number for display/math. */
export const num = (d?: Dec | number | null): number =>
  d == null ? 0 : typeof d === "number" ? d : parseFloat(d);

/** USD, no cents by default. */
export const money = (d?: Dec | number | null, dp = 0): string =>
  num(d).toLocaleString("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: dp,
    maximumFractionDigits: dp,
  });

/** USD with an explicit +/− sign (− is a real minus sign, U+2212). */
export const signedMoney = (d?: Dec | number | null, dp = 0): string => {
  const n = num(d);
  const body = money(Math.abs(n), dp);
  return (n > 0 ? "+" : n < 0 ? "−" : "") + body;
};

export const qty = (d?: Dec | number | null): string =>
  num(d).toLocaleString("en-US", { maximumFractionDigits: 4 });

/** "Jun 21, 2026" from a NaiveDate ("YYYY-MM-DD") or RFC3339 datetime. */
export const fmtDate = (iso?: string | null): string => {
  if (!iso) return "—";
  const d = new Date(iso.length <= 10 ? `${iso}T00:00:00` : iso);
  return d.toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" });
};

/** Compact "2h ago" / "3d ago" from a datetime. */
export const relativeTime = (iso?: string | null): string => {
  if (!iso) return "";
  const then = new Date(iso).getTime();
  const mins = Math.round((Date.now() - then) / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.round(hrs / 24);
  return `${days}d ago`;
};

/** Sign-based CSS class for gain/loss coloring. */
export const gainClass = (n: number): string => (n > 0 ? "gain" : n < 0 ? "loss" : "");
