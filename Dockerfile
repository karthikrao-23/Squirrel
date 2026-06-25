# Multi-stage build for the Squirrel API + its SPA, producing a tiny distroless,
# non-root image. The single binary serves both `/api` and the built frontend
# (via STATIC_DIR), so there's one container to deploy.

# ---------------------------------------------------------------------------
# Stage 1 — build the SPA (Vite) into static assets.
# ---------------------------------------------------------------------------
FROM node:22-alpine AS web
WORKDIR /web
# Install deps from the lockfile first so this layer caches across source edits.
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm ci 2>/dev/null || npm install
COPY frontend/ ./
RUN npm run build   # → /web/dist

# ---------------------------------------------------------------------------
# Stage 2 — build the release binary.
#
# No database is needed at build time: every query is a runtime string (we use
# SQLx's runtime API, not the compile-time `query!` macro), so `SQLX_OFFLINE`
# and a committed `.sqlx/` are irrelevant here.
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /app

# 2a. Dependency-only layer: copy just the manifests + stub sources so `cargo
#     build` compiles the (slow, rarely-changing) dependency graph and caches it.
COPY Cargo.toml Cargo.lock ./
COPY crates/api/Cargo.toml crates/api/Cargo.toml
COPY crates/db/Cargo.toml crates/db/Cargo.toml
COPY crates/domain/Cargo.toml crates/domain/Cargo.toml
COPY crates/plaid/Cargo.toml crates/plaid/Cargo.toml
RUN mkdir -p crates/api/src crates/db/src crates/domain/src crates/plaid/src \
 && echo 'fn main() {}' > crates/api/src/main.rs \
 && : > crates/api/src/lib.rs \
 && : > crates/db/src/lib.rs \
 && : > crates/domain/src/lib.rs \
 && : > crates/plaid/src/lib.rs \
 && cargo build --release -p api \
 && rm -rf crates/*/src

# 2b. Real sources. Touching them invalidates only the workspace crates, not the
#     cached dependency layer above.
COPY crates/ crates/
COPY migrations/ migrations/
# Bump mtimes so cargo definitely rebuilds our crates over the stubs.
RUN find crates -name '*.rs' -exec touch {} + \
 && cargo build --release -p api

# ---------------------------------------------------------------------------
# Stage 3 — runtime: distroless, non-root, read-only-friendly.
#
# `cc-debian12` is enough (we're on rustls — no system OpenSSL). Pinned by
# digest for reproducibility; the `nonroot` variant runs as uid 65532. Update
# the digest deliberately (e.g. `docker buildx imagetools inspect
# gcr.io/distroless/cc-debian12:nonroot`).
# ---------------------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:b0ae8e989418b458e0f25489bc3be523718938a2b70864cc0f6a00af1ddbd985 AS runtime
WORKDIR /app
COPY --from=build /app/target/release/api /app/api
COPY --from=build /app/migrations /app/migrations
COPY --from=web /web/dist /app/dist

# The binary serves the SPA from here and runs migrations from /app/migrations.
ENV STATIC_DIR=/app/dist
# Cloud Run sets PORT; default for local runs.
ENV PORT=8080
EXPOSE 8080

USER nonroot
ENTRYPOINT ["/app/api"]
