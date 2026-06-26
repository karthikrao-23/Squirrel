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

- **Multi-stage `Dockerfile` (in repo).** Stage 1 (`node:22-alpine`) builds the SPA → `dist/`;
  stage 2 (`rust:1-bookworm`) builds the release binary with a dependency-only cache layer so app
  edits don't rebuild the world; stage 3 copies the binary + `migrations/` + `dist/` into
  `gcr.io/distroless/cc-debian12:nonroot` (pinned by digest). We're on `rustls` (no OpenSSL), so
  `cc` is enough and the runtime image stays tiny.
- **No SQLx offline mode needed.** Every query is a *runtime* string (we use SQLx's runtime API,
  not the compile-time `query!` macro), so the image builds without a live database and without a
  committed `.sqlx/`. (Earlier drafts of this doc called for `SQLX_OFFLINE=true`; that's obsolete.)
- **Migrations are baked into the image** (`/app/migrations`); the binary runs `sqlx::migrate!` at
  startup — see §4 for the caveat on auto-migrating in multi-instance setups.
- **One container serves both.** The binary serves the built SPA from `STATIC_DIR=/app/dist`
  (`ServeDir` fallback, mounted after `/api` and `/health`), so `/` is the app and `/api` is the
  API on the same origin — no separate static host, no CORS.
- **Non-root + read-only rootfs.** The image runs as the distroless `nonroot` user (uid 65532);
  the app writes nothing to disk (logs → stdout, data → Postgres), so set the Cloud Run root
  filesystem read-only.

---

## 3. Configuration & secrets

- Everything already comes from env (`crates/api/src/config.rs`) — good, that's 12-factor.
  Production values live in the **platform secrets store** (Fly secrets / Render env groups /
  cloud secrets manager), never in the repo or image.
- **Required in prod:** `APP_ENV=production`, `DATABASE_URL`, `TOKEN_ENCRYPTION_KEY`,
  `PLAID_CLIENT_ID`, `PLAID_SECRET`, `INTERNAL_API_TOKEN`, optional SMTP creds, and `RUST_LOG`
  (`info`, never `debug` for `api`/`sqlx` in prod — `db::connect` also clamps statement logging to
  `WARN` so bound params can't leak). On Cloud Run, `PORT` is injected; `STATIC_DIR=/app/dist` is
  baked into the image.
  - `DATABASE_URL` TLS mode is connection-dependent — see §4.
- **`APP_ENV` is the single source of truth for security posture** (`cookie_secure`, the
  sandbox-connect gate, the scheduler default). **Strict-parsed** (an unknown value is a fatal boot
  error), and `PLAID_ENV` parses strictly too so a typo can't downgrade the Plaid environment.
- **Startup guard — DONE** (`Config::validate_for_prod`): `APP_ENV=production` fails fast unless
  `TOKEN_ENCRYPTION_KEY`, `PLAID_CLIENT_ID/SECRET`, and `INTERNAL_API_TOKEN` are present and cookies
  are `Secure`.
- **`PLAID_WEBHOOK_URL` + `APP_ORIGIN`** are config (set by `deploy/deploy.sh` from `BASE_URL`), so
  the webhook URL and the CSRF Origin check aren't hardcoded.

---

## 4. Database (the part with the most footguns)

- **Use managed Postgres** with automated daily backups + point-in-time recovery. Don't run
  Postgres in the same container as the app in prod (the docker-compose Postgres is dev-only).
- **TLS to the DB depends on the transport:**
  - **Cloud SQL via the built-in unix socket** (what `deploy/` uses):
    `DATABASE_URL=postgres://USER:PW@/squirrel?host=/cloudsql/<INSTANCE_CONN>&sslmode=disable`.
    `sslmode=disable` is correct **here only** — the socket is loopback-local inside the Cloud Run
    sandbox (no network to encrypt), and the Cloud SQL connector handles transport security. The
    password lives in Secret Manager; the full URL is stored as one secret so it never hits a
    command line.
  - **Any TCP/IP connection** (a different host, a non-Cloud-SQL Postgres): use
    `?sslmode=require` (or `verify-full` with the provider CA). This is the §11 checklist item.
- **Migrations strategy — decide deliberately.** The app auto-runs migrations on startup
  (`db::run_migrations`). That's convenient but with **>1 instance** it races (two boots applying
  at once) and couples deploys to schema changes. Recommended evolution:
  - Phase 1 (low instance count): auto-migrate on boot is fine. `sqlx::migrate!` takes a Postgres
    **advisory lock**, so even with `max-instances>1` two cold-starting instances can't double-apply
    — one waits. (The `deploy/` config ships `max-instances=3`; this is safe for *correctness*.)
  - Phase 2 (zero-downtime / breaking changes): run migrations as a **separate one-shot step**
    (a Cloud Run **Job** running the same image with an entrypoint override, or `sqlx migrate run`)
    *before* rolling the app, and make migrations **backward-compatible** (expand/contract: add
    columns nullable, backfill, then drop in a later release) so old and new revisions coexist.
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

- [x] Auth + per-user row scoping enforced at the query layer — **done** (auth backend + composite
      Plaid uniques; cross-tenant isolation tests)
- [ ] `TOKEN_ENCRYPTION_KEY` in a secrets manager; rotation procedure documented (Secret Manager via
      `deploy/secrets.sh`; rotation = add a new secret version + redeploy)
- [x] Plaid webhook signature verification on — **done** (ES256 JWT, alg-pinned, body-hash + iat)
- [x] `RUST_LOG` at `info`; bound-param logging clamped to `WARN` in `db::connect` — **done**
- [ ] TLS to DB — `sslmode=disable` is correct for the Cloud SQL **unix socket** only (see §4); use
      `require`/`verify-full` for any TCP connection. **HTTPS + HSTS done** (HSTS emitted in the
      secure posture). **No CORS needed** — the SPA is served same-origin from the binary.
- [x] Rate limiting + request-size limits — **done** (governor on auth routes; 64 KiB body limit +
      15s timeout)
- [ ] Backups taken **and a restore tested**
- [x] Production-env startup guard rejects missing secrets — **done** (`Config::validate_for_prod`)
- [x] Dependency audit (`cargo audit`) — **gating** CI job, clean; the sole ignore is
      RUSTSEC-2023-0071 (`rsa`, no fix; pulled only by the unused `sqlx-mysql`, never compiled).
      Base image pinned by digest — patch by bumping the digest.
- [x] "Not tax advice" disclaimer rendered in the UI — **done** (M7)

---

## 12. Phased rollout (suggested order)

1. **Containerize** the API (multi-stage `Dockerfile`, no `SQLX_OFFLINE` needed), run it against the
   docker-compose Postgres locally to prove the image. **Done.**
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

---

## 14. GCP runbook (Cloud Run + Cloud SQL) — scripted

The chosen target is a **single public Cloud Run service** serving the SPA + `/api` same-origin,
backed by **Cloud SQL Postgres 16**. Idempotent `gcloud` scripts live in [`deploy/`](deploy/)
(`deploy/README.md` has the copy-paste quickstart). What they wire up, and why it matches the app:

- **Image** — `deploy/deploy.sh` builds the repo `Dockerfile` with Cloud Build, tags by **git SHA**,
  pushes to Artifact Registry, and deploys a new Cloud Run revision.
- **Database** — Cloud SQL via the built-in **unix socket**; `DATABASE_URL` is assembled in
  `secrets.sh` (`host=/cloudsql/<conn>&sslmode=disable`, see §4) and stored as a **single Secret
  Manager secret** so the password never touches a command line. The runtime service account gets
  `cloudsql.client` + `secretmanager.secretAccessor` only.
- **Config/secrets** — `APP_ENV=production`, `PLAID_ENV`, `RUST_LOG=info`, `STATIC_DIR=/app/dist`,
  `SCHEDULER_ENABLED=false`, `APP_ORIGIN`/`PLAID_WEBHOOK_URL` (from `BASE_URL`) as plain env;
  `TOKEN_ENCRYPTION_KEY`, Plaid creds, `INTERNAL_API_TOKEN`, SMTP as secret-backed env. The prod
  **startup guard** (§3) fails the boot if a required secret is missing.
- **Scaling / cost guard** — `min-instances=0`, a hard `max-instances` cap, `concurrency=20`, and
  `512Mi` memory sized for argon2 (~19 MiB/hash × concurrency). Rate limiting on the auth routes
  + the bounded concurrency keep the unauthenticated argon2 surface from amplifying the bill.
- **Scheduler** — `deploy/scheduler.sh` creates an **hourly** Cloud Scheduler job → `POST
  /api/internal/alerts/run`. Because the service is public (SPA + Plaid webhook), Cloud Run IAM
  can't gate `/api/internal/*` per-path, so it's authenticated at the **app layer** by
  `INTERNAL_API_TOKEN` (sent as a bearer header). The in-process `tokio-cron` scheduler is **off**
  in prod (`SCHEDULER_ENABLED=false`); this internal cycle is also what reaps expired sessions.
- **Health** — point a Cloud Run HTTP liveness/readiness probe at `/health` (stays public).
- **`X-Forwarded-For` dependency** — the auth-route rate limiter keys on the real client IP from
  `X-Forwarded-For`, which Google's front end always sets. Don't put another proxy in front that
  strips it, or `/api/auth/{login,signup}` will fail to key.
- **Domain + TLS** — map a custom domain (`gcloud run domain-mappings create`), then set `BASE_URL`
  to it in `config.env` and re-run `deploy.sh` (so `APP_ORIGIN`/`PLAID_WEBHOOK_URL` match) and
  `scheduler.sh`. Register the redirect/webhook URLs in the Plaid dashboard (§5).

**Not in these scripts (deliberate follow-ups):** CD via Workload Identity Federation (the existing
`ci.yml` stays; add a deploy workflow later), read-only root filesystem (Cloud Run doesn't expose a
first-class toggle — the app writes nothing to disk, so the ephemeral container FS is already
effectively unused), and splitting `/api/internal` into its own authenticated-only service if you
want platform-enforced OIDC instead of the app-layer bearer.
