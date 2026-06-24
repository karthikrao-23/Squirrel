import type { ReactNode } from "react";
import { Navigate, Outlet, Route, Routes } from "react-router-dom";
import { useAccounts, useMe } from "./api/hooks";
import { TopBar } from "./components/TopBar";
import { Spinner } from "./components/ui";
import { Alerts } from "./screens/Alerts";
import { Auth } from "./screens/Auth";
import { Dashboard } from "./screens/Dashboard";
import { Harvest } from "./screens/Harvest";
import { Onboarding } from "./screens/Onboarding";

/** Auth boundary: gates the whole app on a valid session. While `me` resolves we
 *  show a spinner; a 401 (or any error) drops to the login screen; success
 *  renders the app (the onboarding gate still applies behind it). */
function AuthGate({ children }: { children: ReactNode }) {
  const me = useMe();

  if (me.isLoading) {
    return (
      <div className="app">
        <Spinner label="Loading…" />
      </div>
    );
  }
  if (me.isError || !me.data) {
    return <Auth />;
  }
  return <>{children}</>;
}

/** Shell for the main app. If no brokerage is connected yet, route to onboarding. */
function AppLayout() {
  const accounts = useAccounts();

  if (accounts.isLoading) {
    return (
      <>
        <TopBar />
        <div className="app">
          <Spinner label="Loading your portfolio…" />
        </div>
      </>
    );
  }
  if (!accounts.isError && (accounts.data?.length ?? 0) === 0) {
    return <Navigate to="/onboarding" replace />;
  }

  return (
    <>
      <TopBar />
      <Outlet />
    </>
  );
}

export function App() {
  return (
    <AuthGate>
      <Routes>
        <Route path="/onboarding" element={<Onboarding />} />
        <Route element={<AppLayout />}>
          <Route path="/" element={<Dashboard />} />
          <Route path="/harvest" element={<Harvest />} />
          <Route path="/alerts" element={<Alerts />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AuthGate>
  );
}
