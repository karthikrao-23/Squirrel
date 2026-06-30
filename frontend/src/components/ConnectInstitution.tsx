import { useEffect, useState } from "react";
import { usePlaidLink } from "react-plaid-link";
import { useExchange, useLinkToken } from "../api/hooks";

/**
 * Button that launches Plaid Link to connect **another** institution — a new
 * bank/brokerage becomes a new Plaid item for the user (the backend allows many
 * per user; `exchange` stores + syncs each). Reuses the same link-token/exchange
 * flow as onboarding, just reachable from the dashboard.
 *
 * The link token is fetched **lazily** (only after the user clicks, via the
 * `armed` flag) so we don't mint a Plaid token on every dashboard render; once
 * it's ready, Link opens automatically. On success the backend exchanges the
 * public token and runs the initial sync, then `useExchange` refreshes the
 * dashboard queries.
 */
export function ConnectInstitutionButton({
  label = "+ Connect institution",
  className = "btn",
}: {
  label?: string;
  className?: string;
}) {
  const [armed, setArmed] = useState(false);
  const linkToken = useLinkToken(armed);
  const exchange = useExchange();
  const token = linkToken.data?.link_token ?? null;

  const { open, ready } = usePlaidLink({
    token,
    onSuccess: (publicToken) => {
      setArmed(false); // single-use token consumed; a fresh one is fetched next time
      exchange.mutate(publicToken);
    },
    onExit: () => setArmed(false), // closed Link without finishing
  });

  // Open Link the moment the freshly-fetched token makes the handler ready. Keyed
  // on `ready` only, so it fires exactly once per arm cycle (not on every render).
  useEffect(() => {
    if (ready) open();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready]);

  const preparing = armed && !ready;
  const error = (linkToken.error as Error | null) || (exchange.error as Error | null);

  return (
    <div className="flex" style={{ gap: 8 }}>
      {error && (
        <span className="loss" style={{ fontSize: 12 }}>
          {error.message}
        </span>
      )}
      <button
        className={className}
        disabled={preparing || exchange.isPending}
        onClick={() => setArmed(true)}
      >
        {exchange.isPending ? "Syncing…" : preparing ? "Preparing…" : label}
      </button>
    </div>
  );
}
