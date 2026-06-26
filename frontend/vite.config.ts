import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The frontend talks only to `/api`; in dev we proxy that to the Axum backend
// (BIND_ADDR=0.0.0.0:8080) so the browser stays same-origin (no CORS needed).
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      // `xfwd` adds X-Forwarded-For so the backend's auth-route rate limiter
      // (keys on the real client IP) can extract a key in dev, just like Cloud
      // Run's front end does in prod.
      "/api": { target: "http://localhost:8080", xfwd: true },
    },
  },
});
