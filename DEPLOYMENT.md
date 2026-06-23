# Squirrel — Deployment Plan (localhost → production)

Companion to `PLAN.md`. That file covers *what we're building*; this one covers *how it goes
live*. It's intentionally forward-looking — we tackle the details when we reach M8
(productization). Today the app runs only via `docker compose up -d` + `cargo run -p api`.

> Guiding principle: **ship the simplest thing that's safe, then scale.** Don't reach for
> Kubernetes on day one. A single small VM or a PaaS app + managed Postgres handles a personal
> tracker (and early users) comfortably, and every step here has an obvious "grow into it" path.

---

## 0. Hard prerequisites (blockers — none of this is optional before real data)

These gate a real deployment regardless of host:

1. **Authentication exists** (PLAN §10; M8 productization). Today there is no auth and `BIND_ADDR` is
   `0.0.0.0:8080`. A public deploy without auth = anyone reads anyone's brokerage data. **This is
   the #1 blocker.**
2. **Plaid production access approved.** Plaid gates `production` behind an application review
   (company info, use case, security questionnaire, often a call). Lead time is **days to
   weeks** — start early. Sandbox → Development (limited live institutions, no review) →
   Production (review required).
3. **A real encryption key management story.** `TOKEN_ENCRYPTION_KEY` must come from a secrets
   manager, not a committed file, and we need a **key-rotation** plan (re-encrypt
   `access_token_encrypted` rows under a new key).
4. **A domain + TLS.** Plaid OAuth redirect URIs and webhooks require public **HTTPS** URLs.

---

## 1. Target architecture (recommended path)

```
            ┌─────────── HTTPS (TLS) ───────────┐
  Browser ──┤  Frontend (static React, CDN)     │
            └─────────── /api/* ────────────────┘
                          │
                   Reverse proxy / platform router
                          │
                 ┌────────▼────────┐      ┌──────────────┐
                 │  api (Rust)     │──────│ Managed       │
                 │  Axum container │ TLS  │ Postgres      │
                 │  + scheduler    │      │ (backups, PITR)│
                 └───┬─────────┬───┘      └──────────────┘
                     │         │
                Plaid API   SMTP/SES (email alerts)
                  (TLS)      (SPF/DKIM)
```

**Recommended host (phase 1):** a managed container PaaS — **Fly.io or Render** — plus that
provider's **managed Postgres**. Rationale: free/cheap TLS + domains, secrets management,
build-from-Dockerfile, health checks, and zero-ops Postgres with automated backups. Cheaper
human-time than a hand-rolled VM; trivial to migrate off later because everything is a container
+ standard Postgres.

**Alternatives (note, don't build yet):**
- **Single VM (Hetzner/DigitalOcean) + Docker Compose + Caddy** for auto-TLS — cheapest in $, most
  in ops (you own patching, backups, monitoring). Fine if you enjoy it; it's a Rust-learning
  project after all.
- **Kubernetes / ECS** — overkill until there are multiple services and real traffic. Explicitly
  deferred.

---

## 2. Build & packaging

- **Multi-stage Dockerfile for `api`.** Stage 1 builds with the Rust toolchain (cache `cargo`
  registry + a dependency-only layer so app edits don't rebuild the world); stage 2 copies the
  single static binary into a minimal base (`debian:bookworm-slim` or `gcr.io/distroless/cc`).
  We're already on `rustls` (no OpenSSL), so the runtime image stays tiny with no system TLS deps.
- **SQLx offline mode.** `.sqlx/` is committed (see `.gitignore`). Build with
  `SQLX_OFFLINE=true` so the image builds **without a live database** — important for CI and
  reproducible builds.
- **Bake `migrations/` into the image** (the binary already runs `sqlx::migrate!` at startup) —
  see §4 for the caveat on auto-migrating in multi-instance setups.
- **Frontend (M7):** `npm run build` → static assets served from the platform's CDN/static
  hosting or behind the same reverse proxy under `/`. API under `/api`. Keep them separable.
- **Pin a non-root user** in the container and set a read-only root filesystem where possible.

---

## 3. Configuration & secrets

- Everything already comes from env (`crates/api/src/config.rs`) — good, that's 12-factor.
  Production values live in the **platform secrets store** (Fly secrets / Render env groups /
  cloud secrets manager), never in the repo or image.
- **Required in prod:** `DATABASE_URL` (with `sslmode=require`), `TOKEN_ENCRYPTION_KEY`,
  `PLAID_ENV=production`, `PLAID_CLIENT_ID`, `PLAID_SECRET`, SMTP creds, `BIND_ADDR`
  (bind to the platform's expected port), `RUST_LOG` (drop to `info`, never `debug` for
  `api`/`sqlx` in prod — avoids leaking tokens/PII into logs).
- **Add a startup guard:** when `PLAID_ENV=production`, fail fast if any of the Plaid/encryption
  secrets are empty (today they're optional — fine for M1, unsafe for prod). Small change to
  `Config::from_env`.
- **Add a `PLAID_WEBHOOK_URL` / `APP_BASE_URL`** config so redirect + webhook URLs aren't
  hardcoded.

---

## 4. Database (the part with the most footguns)

- **Use managed Postgres** with automated daily backups + point-in-time recovery. Don't run
  Postgres in the same container as the app in prod (the docker-compose Postgres is dev-only).
- **TLS to the DB:** append `?sslmode=require` (or `verify-full` with the provider CA) to
  `DATABASE_URL`.
- **Migrations strategy — decide deliberately.** The app auto-runs migrations on startup
  (`db::run_migrations`). That's convenient but with **>1 instance** it races (two boots applying
  at once) and couples deploys to schema changes. Recommended evolution:
  - Phase 1 (single instance): auto-migrate on boot is fine.
  - Phase 2 (multi-instance / zero-downtime): run migrations as a **separate one-shot step** in
    the deploy pipeline (`sqlx migrate run`) *before* rolling the app, and make migrations
    **backward-compatible** (expand/contract: add columns nullable, backfill, then drop in a
    later release) so old and new instances coexist during a rolling deploy.
- **Connection pool sizing:** `max_connections(10)` per instance (`crates/db/src/lib.rs`) ×
  instances must stay under the managed DB's connection limit. Consider a pooler (PgBouncer /
  provider-built-in) before scaling out.
- **Have a tested restore drill**, not just backups. A backup you've never restored isn't a
  backup.

---

## 5. Plaid in production (external dependency, long lead time)

- **Apply for production access early** (see §0). Until approved, deploy against **Development**
  env to exercise the live flow with real-but-limited institutions.
- **Webhooks need a public HTTPS endpoint** and **signature verification** (PLAN §10; deferred to M8) —
  do not trust an unverified webhook body. Plaid signs with a JWT; verify it.
- **OAuth redirect URIs** for certain institutions must be **registered in the Plaid dashboard**
  and match `APP_BASE_URL` exactly.
- **Rotate/secure the Plaid secret** like any other credential; never log request bodies (they
  carry `access_token`).
- **Rate limits & retries:** wrap Plaid calls with timeouts + bounded retry/backoff; handle
  `ITEM_LOGIN_REQUIRED` (re-auth) gracefully.

---

## 6. Networking, TLS, and the reverse proxy

- **HTTPS everywhere**, HTTP→HTTPS redirect. On Fly/Render this is automatic; on a VM use **Caddy**
  (auto Let's Encrypt) or Traefik.
- **CORS** (PLAN §10; M8): lock `tower-http` CORS to the exact frontend origin — no wildcards
  once auth/cookies are involved.
- **Security headers:** HSTS, `X-Content-Type-Options`, a sensible CSP for the frontend, etc. Add
  via a tower-http layer or the proxy.
- **Request limits:** body-size limit + timeout layers on Axum; basic **rate limiting** (per-IP /
  per-user) in front of auth and Plaid-triggering endpoints.

---

## 7. Background jobs & the scheduler (subtle multi-instance bug)

- M5 introduces `tokio-cron-scheduler` for alert generation. With **multiple app instances, every
  instance runs the cron** → duplicate alerts/emails. Options, pick one before scaling out:
  - Run the scheduler in a **single dedicated worker instance** (app instances serve HTTP only).
  - Use a **Postgres advisory lock** / leader election so only one instance executes a given job.
  - Move to an external scheduler (cron job hitting an internal endpoint, or a queue).
- Make alert generation **idempotent** (the `alerts.emailed_at` column already supports
  "don't re-send") so an accidental double-run is harmless.

---

## 8. Email deliverability

- `lettre` + a real provider (**SES / Postmark / Mailgun**), not raw SMTP from the VM (will land
  in spam / get blocked on port 25).
- Set up **SPF, DKIM, DMARC** for the sending domain. Use a verified `SMTP_FROM`.
- Keep Mailtrap for staging; production uses the real provider with separate creds.

---

## 9. Observability

- **Structured logging:** keep `tracing`; emit JSON in prod and ship to the platform's log
  aggregator. **Scrub secrets/PII** — never log access tokens or full Plaid payloads.
- **Health checks:** `/health` exists (`crates/api/src/routes/health.rs`) — wire it to the
  platform liveness/readiness probe; add a readiness variant that checks DB connectivity.
- **Error tracking:** Sentry (or similar) for panics/5xx.
- **Metrics (later):** Prometheus endpoint or platform metrics — request latency, Plaid call
  failures, job run counts, DB pool saturation.
- **Uptime monitoring + alerting** on the health endpoint.

---

## 10. CI/CD

- **CI (GitHub Actions):** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
  (incl. `cargo test -p domain` fixtures from PLAN §7), build the Docker image with
  `SQLX_OFFLINE=true`. A SessionStart hook already standardizes local test/lint — mirror it in CI.
- **CD:** on merge to `main` (or a tagged release), build + push the image, run the migration
  one-shot, then roll the app. Keep a **manual approval** gate for production until confident.
- **Rollbacks:** images are immutable + tagged by SHA → roll back by redeploying the previous
  tag. Because of expand/contract migrations (§4), a code rollback never strands the schema.
- **Environments:** at least `staging` (Plaid sandbox/dev, Mailtrap) and `production` (Plaid prod,
  real email), with separate DBs and secrets.

---

## 11. Pre-launch security checklist (cross-ref PLAN §10)

- [ ] Auth + per-user row scoping enforced at the query layer (not just `user_id` columns)
- [ ] `TOKEN_ENCRYPTION_KEY` in a secrets manager; rotation procedure documented
- [ ] Plaid webhook signature verification on
- [ ] `RUST_LOG` at `info`; log scrubbing verified (no tokens/PII)
- [ ] TLS to DB (`sslmode`), HTTPS + HSTS on the app, CORS locked to frontend origin
- [ ] Rate limiting + request-size limits in place
- [ ] Backups taken **and a restore tested**
- [ ] Production-env startup guard rejects missing secrets
- [ ] Dependency audit (`cargo audit`) clean; base image patched
- [ ] "Not tax advice" disclaimer rendered in the UI (PLAN §4)

---

## 12. Phased rollout (suggested order)

1. **Containerize** the API (multi-stage Dockerfile, `SQLX_OFFLINE`), run it against the
   docker-compose Postgres locally to prove the image.
2. **Stand up staging** on the chosen PaaS + managed Postgres; deploy with Plaid **Development**
   env and Mailtrap. Exercise the full connect → holdings → alerts loop.
3. **Harden:** add the startup guard, CORS lockdown, rate limits, log scrubbing, health/readiness
   probes, error tracking.
4. **Apply for Plaid production** (do this in parallel from step 2 — it's the long pole).
5. **Move migrations to a one-shot deploy step** and adopt expand/contract once you want
   zero-downtime / >1 instance.
6. **Production cutover:** real domain + TLS, production secrets, real email provider with
   SPF/DKIM, run the §11 checklist, then go live behind auth.
7. **Scale when needed:** single instance is plenty initially. Before scaling out, resolve the
   scheduler single-runner question (§7) and DB connection pooling (§4).

---

## 13. Rough cost (phase 1, ballpark)

- PaaS app instance(s): ~$5–15/mo each
- Managed Postgres (small + backups): ~$15–25/mo
- Email provider: free tier early, then usage-based
- Domain: ~$10–15/yr
- Plaid: free in sandbox/dev; production is usage-priced — confirm current pricing at apply time

Total ≈ **$25–50/mo** for a small production footprint, scaling with usage.
