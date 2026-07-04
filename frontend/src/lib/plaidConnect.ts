import { useEffect, useMemo, useState } from "react";
import { usePlaidLink } from "react-plaid-link";
import { useExchange, useLinkToken } from "../api/hooks";

// Where we stash the link token across an OAuth redirect. Selecting an OAuth
// bank (E*Trade, Schwab, Chase, …) sends the browser to the bank's site and back
// to our registered redirect URI — a full page load — so the token that started
// the session has to survive in storage to resume it.
const OAUTH_TOKEN_KEY = "plaid_oauth_link_token";

/** True when this page load is Plaid returning from an OAuth bank: it appends
 *  `?oauth_state_id=...` to our redirect URI. */
function isOAuthRedirect(): boolean {
  return typeof window !== "undefined" && /[?&]oauth_state_id=/.test(window.location.search);
}

/**
 * Drives Plaid Link for both entry points (onboarding + "connect another"),
 * including the **OAuth redirect round-trip** that banks like E*Trade require.
 *
 * - `eager` (onboarding): fetch the token up front and open only when the user
 *   clicks `open()`. Non-eager (dashboard button): `connect()` fetches a token
 *   and auto-opens once ready.
 * - On any flow, the token is persisted before Link opens, and if the page is an
 *   OAuth return we reopen Link with `receivedRedirectUri` to finish the handshake.
 */
export function usePlaidConnect({ eager = false }: { eager?: boolean } = {}) {
  const exchange = useExchange();

  // Evaluated once per mount: was this page reached via an OAuth redirect?
  const oauthReturn = useMemo(isOAuthRedirect, []);
  const storedToken = oauthReturn ? localStorage.getItem(OAUTH_TOKEN_KEY) : null;

  const [armed, setArmed] = useState(eager);
  // An OAuth return reuses the stored token, so no fetch is needed.
  const linkTokenQ = useLinkToken(armed && !oauthReturn);
  const token = oauthReturn ? storedToken : (linkTokenQ.data?.link_token ?? null);

  const finishOAuth = () => {
    localStorage.removeItem(OAUTH_TOKEN_KEY);
    setArmed(false);
    // Strip `?oauth_state_id` so a refresh doesn't try to resume a spent flow.
    if (oauthReturn) window.history.replaceState({}, "", window.location.pathname);
  };

  const { open, ready } = usePlaidLink({
    token,
    receivedRedirectUri: oauthReturn ? window.location.href : undefined,
    onSuccess: (publicToken) => {
      finishOAuth();
      exchange.mutate(publicToken);
    },
    onExit: finishOAuth,
  });

  // Persist the token before the user can pick an OAuth bank, so it survives the
  // round-trip to the bank and back.
  useEffect(() => {
    if (token && !oauthReturn) localStorage.setItem(OAUTH_TOKEN_KEY, token);
  }, [token, oauthReturn]);

  // Auto-open when resuming OAuth, or when a non-eager caller has armed a connect.
  // Eager (onboarding) opens only via an explicit `open()` click.
  const autoOpen = oauthReturn || (armed && !eager);
  useEffect(() => {
    if (ready && autoOpen) open();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, autoOpen]);

  const preparing = (armed || oauthReturn) && !ready && !exchange.isPending;
  const error = (linkTokenQ.error as Error | null) || (exchange.error as Error | null);

  return {
    open,
    ready: ready && !!token,
    connect: () => setArmed(true),
    preparing,
    isSyncing: exchange.isPending,
    error,
  };
}
