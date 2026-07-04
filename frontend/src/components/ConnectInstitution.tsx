import { usePlaidConnect } from "../lib/plaidConnect";

/**
 * Button that launches Plaid Link to connect **another** institution — a new
 * bank/brokerage becomes a new Plaid item for the user (the backend allows many
 * per user; `exchange` stores + syncs each). Reuses the same link-token/exchange
 * flow as onboarding, just reachable from the dashboard.
 *
 * The link token is fetched **lazily** (only after the user clicks) so we don't
 * mint a Plaid token on every dashboard render; once ready, Link opens. OAuth
 * banks (E*Trade, …) round-trip through the bank and back — `usePlaidConnect`
 * persists the token and resumes Link on the redirect.
 */
export function ConnectInstitutionButton({
  label = "+ Connect institution",
  className = "btn",
}: {
  label?: string;
  className?: string;
}) {
  const { connect, preparing, isSyncing, error } = usePlaidConnect();

  return (
    <div className="flex" style={{ gap: 8 }}>
      {error && (
        <span className="loss" style={{ fontSize: 12 }}>
          {error.message}
        </span>
      )}
      <button
        className={className}
        disabled={preparing || isSyncing}
        onClick={connect}
      >
        {isSyncing ? "Syncing…" : preparing ? "Preparing…" : label}
      </button>
    </div>
  );
}
