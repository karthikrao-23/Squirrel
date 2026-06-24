import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "./App";
import { UnauthorizedError } from "./api/client";
import "./styles.css";

// When any query or mutation hits a 401 (session expired mid-session), nudge the
// `me` query to refetch. It will 401 too, flipping `<AuthGate>` to the login
// screen. We skip the `me` query's own errors so this can't loop.
function onUnauthorized(error: unknown, queryKey?: readonly unknown[]) {
  if (error instanceof UnauthorizedError && queryKey?.[0] !== "me") {
    queryClient.invalidateQueries({ queryKey: ["me"] });
  }
}

const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error, query) => onUnauthorized(error, query.queryKey),
  }),
  mutationCache: new MutationCache({
    onError: (error) => onUnauthorized(error),
  }),
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
      // Don't burn a retry on an auth failure — a 401 is a definitive answer.
      retry: (count, error) => !(error instanceof UnauthorizedError) && count < 1,
    },
  },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
