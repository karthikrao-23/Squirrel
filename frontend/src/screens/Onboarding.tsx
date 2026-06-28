import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { usePlaidLink } from "react-plaid-link";
import {
  useAccounts,
  useExchange,
  useLinkToken,
  useProfile,
  useSandboxConnect,
  useUpdateProfile,
} from "../api/hooks";
import { FILING_STATUS_LABELS, type FilingStatus } from "../api/types";

const STATUSES = Object.keys(FILING_STATUS_LABELS) as FilingStatus[];

/** Step 1 connect: real Plaid Link (primary) plus a dev-only sandbox shortcut. */
function ConnectStep() {
  // Fetch a Link token up front so the button is ready to open Plaid Link.
  const linkToken = useLinkToken(true);
  const exchange = useExchange();
  const sandbox = useSandboxConnect();
  const token = linkToken.data?.link_token ?? null;

  const { open, ready } = usePlaidLink({
    token,
    // Plaid returns a short-lived public_token; the backend exchanges it, stores
    // the item, and runs the initial sync inline (so success = portfolio ready).
    onSuccess: (publicToken) => exchange.mutate(publicToken),
  });

  const syncing = exchange.isPending || sandbox.isPending;
  const error =
    (linkToken.error as Error | null) ||
    (exchange.error as Error | null) ||
    (sandbox.error as Error | null);

  return (
    <>
      <p className="muted" style={{ marginTop: 0 }}>
        Link a brokerage to import holdings and transactions. We never see your password — Plaid
        returns a token we exchange server-side.
      </p>
      <div className="flex">
        <button
          className="btn primary lg"
          disabled={!ready || !token || syncing}
          onClick={() => open()}
        >
          {exchange.isPending
            ? "Syncing your portfolio…"
            : linkToken.isLoading
              ? "Preparing…"
              : "Connect a brokerage"}
        </button>
        {/* Dev-only: the sandbox shortcut isn't in the production bundle, and the
            backend 403s it outside development anyway. */}
        {import.meta.env.DEV && (
          <button
            className="btn lg"
            style={{ marginLeft: 8 }}
            disabled={syncing}
            onClick={() => sandbox.mutate()}
          >
            {sandbox.isPending ? "Connecting…" : "Dev: sandbox shortcut"}
          </button>
        )}
      </div>
      {syncing && (
        <div className="faint mt16" style={{ fontSize: 13 }}>
          Importing holdings + transactions and reconstructing tax lots — this can take a few
          seconds.
        </div>
      )}
      {error && (
        <div className="loss mt16" style={{ fontSize: 13 }}>
          {error.message}
        </div>
      )}
    </>
  );
}

export function Onboarding() {
  const navigate = useNavigate();
  const accounts = useAccounts();
  const profile = useProfile();
  const updateProfile = useUpdateProfile();

  const connected = (accounts.data?.length ?? 0) > 0;

  const [status, setStatus] = useState<FilingStatus>("single");
  const [income, setIncome] = useState("");

  // Seed the form from the existing profile once it loads.
  useEffect(() => {
    if (profile.data) {
      setStatus((profile.data.filing_status as FilingStatus) ?? "single");
      if (profile.data.taxable_income) setIncome(String(parseFloat(profile.data.taxable_income)));
    }
  }, [profile.data]);

  function finish() {
    updateProfile.mutate(
      { filing_status: status, taxable_income: income === "" ? undefined : income },
      { onSuccess: () => navigate("/") },
    );
  }

  const step = !connected ? 1 : 2;

  return (
    <>
      <div className="topbar">
        <div className="brand">
          <span className="logo">🐿️</span> Squirrel
        </div>
        <div className="spacer" />
        <span className="muted">Setup</span>
      </div>

      <div className="app" style={{ maxWidth: 720 }}>
        <div className="page-head">
          <h1>Squirrel away more of your gains 🐿️</h1>
          <p>Connect a brokerage, then tell us your tax situation so Squirrel can time your sells and harvest losses.</p>
        </div>

        <div className="stepper">
          <div className={`step ${connected ? "done" : "active"}`}>
            <span className="n">{connected ? "✓" : "1"}</span> Connect
          </div>
          <div className="step-line" />
          <div className={`step ${step === 2 ? "active" : ""}`}>
            <span className="n">2</span> Tax profile
          </div>
        </div>

        {/* Step 1 — Connect */}
        <div className="card mt16">
          <div className="card-head">
            <h2>1 · Connect your brokerage</h2>
            <span className="api">Plaid Link · POST /api/plaid/exchange</span>
          </div>
          <div className="card-body">
            {connected ? (
              <div className="flex between">
                <div className="flex">
                  <div className="alert-ic lt">🔗</div>
                  <div>
                    <div style={{ fontWeight: 600 }}>
                      {accounts.data!.length} account(s) connected
                    </div>
                    <div className="faint">Holdings + transactions imported · tax lots reconstructed (FIFO)</div>
                  </div>
                </div>
                <span className="chip gain dot">Connected</span>
              </div>
            ) : (
              <ConnectStep />
            )}
          </div>
        </div>

        {/* Step 2 — Tax profile */}
        <div className="card mt16" style={{ opacity: connected ? 1 : 0.55 }}>
          <div className="card-head">
            <h2>2 · Your tax profile</h2>
            <span className="api">PATCH /api/profile</span>
          </div>
          <div className="card-body">
            <div className="field">
              <label>Filing status</label>
              <div className="radio-row">
                {STATUSES.map((s) => (
                  <label
                    key={s}
                    className={`radio-card ${status === s ? "sel" : ""}`}
                    onClick={() => setStatus(s)}
                  >
                    <div className="t">{FILING_STATUS_LABELS[s]}</div>
                    <div className="d">{s}</div>
                  </label>
                ))}
              </div>
            </div>
            <div className="field">
              <label>Taxable income (excluding investment gains)</label>
              <input
                className="input"
                inputMode="numeric"
                placeholder="120000"
                value={income}
                onChange={(e) => setIncome(e.target.value.replace(/[^0-9.]/g, ""))}
              />
              <div className="hint">
                Determines your federal LT bracket (0/15/20%), NIIT threshold, and CA ordinary rate.
              </div>
            </div>
            <button
              className="btn primary lg block"
              disabled={!connected || updateProfile.isPending}
              onClick={finish}
            >
              {updateProfile.isPending ? "Saving…" : "Finish setup →"}
            </button>
          </div>
        </div>

        <p className="foot">
          Estimates are decision-support, <strong>not tax advice</strong>. Federal + California only in v1.
        </p>
      </div>
    </>
  );
}
