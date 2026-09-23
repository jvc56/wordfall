# Progress

Resume from this file plus `docs/plan-index.md`. PLAN.md is the spec.

## Current

- **Phase:** 2 — Accounts (next to start)
- **Last green checkpoint:** Phase 1 — Skeleton

## Environment notes (this machine)

- Node 22 is installed user-locally at `~/.local/n/bin` (system node is 18).
  The Makefile prepends it automatically; in a shell use
  `export PATH=$HOME/.local/n/bin:$PATH`.
- Rust stable 1.98 is pinned in `backend/rust-toolchain.toml`.
- Compile-checked SQLx queries: `make sqlx-db` starts a Postgres container
  `wordfall-sqlx` on 127.0.0.1:55432 with the schema; `backend/.env`
  (gitignored) points `DATABASE_URL` at it. After adding or changing a query,
  run `make sqlx-prepare` and commit `backend/.sqlx/`. Builds and all test
  targets use `SQLX_OFFLINE=true`. After editing the migration, rerun
  `make sqlx-db`.
- Port 5432 on this host is taken by another project's Postgres; nothing here
  publishes Postgres except `wordfall-sqlx` (55432) and the throwaway test DB
  (random port).
- Playwright has no Ubuntu 20.04 build; the Makefile sets
  `PLAYWRIGHT_HOST_PLATFORM_OVERRIDE=ubuntu22.04-x64` on 20.04 hosts, and that
  Chromium runs here.
- Terraform runs in a container: `make infra-validate`.
- Host Python is 3.8; the scripts are written to run on 3.8+ (plan says 3.11).

## Done

### Phase 1 — Skeleton ✅
- `backend/`: Axum + SQLx crate `wordfall`; config from env with every
  variable of PLAN.md § Configuration (malformed values fail startup);
  migration run at startup via `sqlx::migrate!`; JSON tracing; `/health`
  (DB + catalog-ready flag, 503 until ready).
- `backend/migrations/0001_initial.sql` = PLAN.md lines 3008–3700 verbatim.
- `frontend/`: SvelteKit 2 / Svelte 5 static SPA (adapter-static, fallback
  `index.html`), Tailwind v4 (PQ-001), `kit.csp` hash mode;
  `npm run build` ends with `scripts/check-csp.js` over `build/index.html`.
  Asset inlining is off (data: URIs would violate `default-src 'self'`).
- `docker/`: backend Dockerfile, frontend (Nginx) Dockerfile,
  `nginx.conf.template` + `wordfall-headers.inc` (frame-ancestors 'self',
  nosniff, HSTS via env, no-cache on index.html/service-worker.js, immutable
  hashed assets, SPA fallback, real-IP so backend reads 1 hop).
- `docker-compose.yml`: postgres:16, backend, frontend on :5173 (port via
  `WORDFALL_PORT`), `hot-reload` profile with Vite on :5174. Backend env =
  `docker/backend.defaults.env` + `.stack/<project>.env` (written by stack.py
  per `up`: per-project signing key, PUBLIC_URL, `--env` overrides).
- `scripts/stack.py` (up/seed/reset/down + CLI), `scripts/dev.py` (all flags
  from PLAN.md § Development). `seed` is written against assumed auth/admin
  contracts (cookie `wordfall_csrf`, header `X-CSRF-Token`, confirm body
  `{code}`, console mail log line with `mail_to` and `confirmation_code`
  JSON fields, `/api/admin/catalog` keys) — **align these with PLAN.md
  § Authentication in Phase 2 and § Admin in Phase 3.**
- `Makefile`: test-unit, test-integration (throwaway postgres:16 on a random
  port, `#[sqlx::test]` per-test DBs), test-e2e (Playwright; globalSetup calls
  `scripts/stack.py up` + `seed`), test-scale and test-parity scaffolds
  (`scripts/scale.py`, `scripts/parity.py` — parity skips without licensed
  data), `infra-validate`, `sqlx-db`, `sqlx-prepare`.
- `e2e/`: Playwright config, global setup/teardown, `tests/shell.spec.ts`
  (shell boots under CSP, Nginx headers present).
- `infra/`: Terraform baseline (VPC, ALB+ACM, ECS 2 vCPU two containers,
  RDS PG16 30-day PITR + autoscaling, SES identity, SSM names, logs, ECR,
  IAM). `terraform validate` passes.
- Gate: stack comes up, migration applied, `/health` 200 via Nginx; unit and
  integration targets green; shell e2e spec passed against the running stack.
  `make test-e2e` end-to-end needs seeding (registration) — Phase 2.

## Next

Phase 2 — Accounts. Re-read: UX → Accounts/Preferences (430–462),
Authentication (3871–4056), Authentication while offline (2885–2995), API
preamble + Auth and account (4057–4100), Schema accounts (3103–3192),
Configuration, Integration tests (5299–5664) auth parts, Frontend auth routes.
Then fix `stack.seed` to the real contracts and make `make test-e2e` green.

## Open PQs

- PQ-001 (Tailwind v4 dark variant) — provisional, non-blocking.

## Known failing tests

- None. (`make test-e2e` cannot seed until Phase 2 registration exists.)
