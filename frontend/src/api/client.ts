// Thin fetch wrapper. The Axum backend returns errors as `{ "error": "message" }`
// (see crates/api/src/error.rs); surface that text so TanStack Query can show it.

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    ...init,
  });

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
