import { useState, type FormEvent } from "react";
import { useLogin, useSignup } from "../api/hooks";

type Mode = "login" | "signup";

/** Login / signup screen shown by `<AuthGate>` when there's no session. Posts to
 *  `/api/auth/*` same-origin, so the session cookie is set automatically. On
 *  success the `me` query is seeded and the gate renders the app. */
export function Auth() {
  const [mode, setMode] = useState<Mode>("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  const login = useLogin();
  const signup = useSignup();
  const active = mode === "login" ? login : signup;

  function submit(e: FormEvent) {
    e.preventDefault();
    active.mutate({ email, password });
  }

  function switchMode(next: Mode) {
    setMode(next);
    login.reset();
    signup.reset();
  }

  const error = active.error as Error | null;

  return (
    <>
      <div className="topbar">
        <div className="brand">
          <span className="logo">🐿️</span> Squirrel
        </div>
        <div className="spacer" />
        <span className="muted">{mode === "login" ? "Sign in" : "Create account"}</span>
      </div>

      <div className="app" style={{ maxWidth: 440 }}>
        <div className="page-head">
          <h1>{mode === "login" ? "Welcome back" : "Squirrel away more of your gains 🐿️"}</h1>
          <p>
            {mode === "login"
              ? "Sign in to see your portfolio and harvest opportunities."
              : "Create an account to connect a brokerage and track your tax lots."}
          </p>
        </div>

        <div className="card mt16">
          <div className="card-body">
            <form onSubmit={submit}>
              <div className="field">
                <label htmlFor="email">Email</label>
                <input
                  id="email"
                  className="input"
                  type="email"
                  autoComplete="email"
                  required
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor="password">Password</label>
                <input
                  id="password"
                  className="input"
                  type="password"
                  autoComplete={mode === "login" ? "current-password" : "new-password"}
                  required
                  minLength={mode === "signup" ? 12 : undefined}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                />
                {mode === "signup" && (
                  <div className="hint">At least 12 characters.</div>
                )}
              </div>

              {error && (
                <div className="loss mt8" style={{ fontSize: 13 }}>
                  {mode === "login" && error.name === "UnauthorizedError"
                    ? "Incorrect email or password."
                    : error.message}
                </div>
              )}

              <button className="btn primary lg block mt16" type="submit" disabled={active.isPending}>
                {active.isPending
                  ? mode === "login"
                    ? "Signing in…"
                    : "Creating account…"
                  : mode === "login"
                    ? "Sign in"
                    : "Create account"}
              </button>
            </form>

            <div className="muted mt16" style={{ textAlign: "center", fontSize: 13 }}>
              {mode === "login" ? (
                <>
                  No account?{" "}
                  <button className="linklike" onClick={() => switchMode("signup")}>
                    Create one
                  </button>
                </>
              ) : (
                <>
                  Already have an account?{" "}
                  <button className="linklike" onClick={() => switchMode("login")}>
                    Sign in
                  </button>
                </>
              )}
            </div>
          </div>
        </div>

        <p className="foot">
          Estimates are decision-support, <strong>not tax advice</strong>. Federal + California only in v1.
        </p>
      </div>
    </>
  );
}
