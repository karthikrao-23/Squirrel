// Thin fetch wrapper. The Axum backend returns errors as `{ "error": "message" }`
// (see crates/api/src/error.rs); surface that text so TanStack Query can show it.
//
// Two auth concerns live here so every call inherits them:
//   1. Mutating requests carry the `X-Squirrel-CSRF` header. Cross-site JS can't
//      set a custom header without a CORS preflight the backend never grants, so
//      this is the CSRF token the backend's guard requires.
//   2. A 401 is turned into a typed `UnauthorizedError`, which the query client's
//      global handler uses to bounce the user back to the login screen.

export const CSRF_HEADER = "X-Squirrel-CSRF";

/** Thrown on any 401 so callers (and the global handler) can react to an
 *  expired/absent session without string-matching messages. */
export class UnauthorizedError extends Error {
  constructor() {
    super("unauthorized");
    this.name = "UnauthorizedError";
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const method = (init?.method ?? "GET").toUpperCase();
  const mutating = method !== "GET" && method !== "HEAD";
  const hasBody = init?.body != null;

  const res = await fetch(path, {
    ...init,
    headers: {
      // Only declare a JSON body when we actually send one. Sending
      // `Content-Type: application/json` with an empty body makes optional-JSON
      // extractors (e.g. POST /api/plaid/sandbox/connect) fail to parse "".
      ...(hasBody ? { "Content-Type": "application/json" } : {}),
      ...(mutating ? { [CSRF_HEADER]: "1" } : {}),
      ...(init?.headers ?? {}),
    },
  });

  if (res.status === 401) throw new UnauthorizedError();

  if (!res.ok) {
    let message = `Request failed (${res.status})`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body?.error) message = body.error;
    } catch {
      // non-JSON error body; keep the status-based message
    }
    throw new Error(message);
  }

  // 204 / empty bodies
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const get = <T>(path: string) => api<T>(path);

export const post = <T>(path: string, body?: unknown) =>
  api<T>(path, { method: "POST", body: body == null ? undefined : JSON.stringify(body) });

export const patch = <T>(path: string, body: unknown) =>
  api<T>(path, { method: "PATCH", body: JSON.stringify(body) });

export const del = <T>(path: string) => api<T>(path, { method: "DELETE" });
