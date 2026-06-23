import { NavLink } from "react-router-dom";
import { useAlerts } from "../api/hooks";

export function TopBar() {
  // Unread badge: derive from the alerts list (no read_at => unread).
  const { data: alerts } = useAlerts();
  const unread = alerts?.filter((a) => a.read_at == null).length ?? 0;

  return (
    <div className="topbar">
      <div className="brand">
        <span className="logo">🐿️</span> Squirrel
      </div>
      <nav className="nav">
        <NavLink to="/" end className={({ isActive }) => (isActive ? "active" : "")}>
          Dashboard
        </NavLink>
        <NavLink to="/harvest" className={({ isActive }) => (isActive ? "active" : "")}>
          Harvest
        </NavLink>
        <NavLink to="/alerts" className={({ isActive }) => (isActive ? "active" : "")}>
          Alerts
        </NavLink>
      </nav>
      <div className="spacer" />
      <NavLink to="/alerts" className="bell" aria-label={`${unread} unread alerts`}>
        🔔{unread > 0 && <span className="badge">{unread}</span>}
      </NavLink>
    </div>
  );
}
