import { Navigate, Outlet, Route, Routes } from "react-router-dom";
import { useAccounts } from "./api/hooks";
import { TopBar } from "./components/TopBar";
import { Spinner } from "./components/ui";
import { Alerts } from "./screens/Alerts";
import { Dashboard } from "./screens/Dashboard";
import { Harvest } from "./screens/Harvest";
import { Onboarding } from "./screens/Onboarding";

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
    <Routes>
      <Route path="/onboarding" element={<Onboarding />} />
      <Route element={<AppLayout />}>
        <Route path="/" element={<Dashboard />} />
        <Route path="/harvest" element={<Harvest />} />
        <Route path="/alerts" element={<Alerts />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
