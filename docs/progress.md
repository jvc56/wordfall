# Progress

Resume from this file plus `docs/plan-index.md`. PLAN.md is the spec.

## Current

- **Phase:** 3 — Catalog (next to start)
- **Last green checkpoint:** Phase 2 — Accounts

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

### Phase 2 — Accounts ✅
- Backend: `auth/` (tokens: PASETO v4.local session + HKDF export key;
  password: Argon2 + zxcvbn≥3, 8–128 chars; mail: console/recording mailers,
  in-memory 24h caps 3/IP/address and 20/address; session: `Session`
  (binding + CSRF), `UnboundSession` (/me), `Admin` (404 for non-admins),
  `renewal()` for the sliding TTL; routes: every Auth and account endpoint).
  `rate.rs`: governor keyed limiters for every named bucket + `FailureBuckets`
  (PQ-004). `net.rs` client IP by `TRUSTED_PROXY_HOPS`. `clock.rs` (test
  offset). `extract.rs` `ApiJson` (415 on wrong content type). `purge.rs`
  skeleton (advisory lock; deletes stale unconfirmed accounts only so far).
- Emails are queued after the response (spawned), so register/reset branches
  time alike. Registration takes sync seq 1 and stamps prefs; 6 default
  bindings.
- `backend/tests/auth.rs`: 25 integration tests (register branches, email
  caps, replacement under cap, timing, confirm, cookies/TTL, /me CSRF re-set,
  logout rules, 415, account binding, sign out everywhere, change password,
  delete, expiry, reset flow, login-failure limits before Argon2, per-IP auth
  bucket, purge of stale unconfirmed).
- Frontend: shadcn-svelte (vega style, zinc/neutral) + components in
  `src/lib/components/ui`; `lib/api.ts` (CSRF, X-Wordfall-User, withRetry);
  `lib/local/accounts.ts` = the **unscoped IndexedDB database** (accounts
  table + signed_in pointer, queued logout rules) with 14 Vitest cases;
  `lib/auth/session.svelte.ts` (startup reads pointer, /me comparison,
  BroadcastChannel, login/logout); pages `/`, `/login`, `/register`,
  `/register/check-email`, `/confirm-email`, `/reset-password`,
  `/reset-password/confirm`, minimal `/account` (logout + remove data,
  change password, sign out everywhere, delete), placeholder `/cascades`.
- `make test-e2e` is green end to end (seed registers `dev` through the real
  endpoints, code read from the console mail log).
- Deferred to later phases (tracked): sliding-TTL integration test (needs
  sync, Phase 6); account-binding tests for sync/cards/cascade creation/export
  token (their phases); admin-authorization test (Phase 3, first admin route).
- Terraform: service capped at two tasks.

## Next

Phase 3 — Catalog. Re-read: Admin (1031–1222), Tiles (1223–1286),
Probability (1489–1508), Catalog Indexes (1642–1700), Schema catalog
(3193–3268), API → Catalog and search (4101–4165) and Admin (4254–4270),
Testing → fixture catalog (4765–4781), backend unit tests (4888–4999),
integration tests admin parts. Build the fixture catalog FIRST
(`fixtures/catalog/` + `manifest.json` read by `stack.fixture_catalog()`:
entries `{kind: distribution|lexicon|leaves, name, file, parent}`).
Then: upload validators (line-numbered errors capped at 1,000 + total),
greedy tile parsing (no backtracking), LexiconIndex/LeaveSetIndex with
every derived attribute, LISTEN/NOTIFY reconcile, `catalog_instance_status`,
`/health` readiness from real loading, `GET /api/lexicons`,
`GET /api/letter-distributions/:name`, `/api/admin/*`, `/admin` pages. Check
`stack.seed`'s `/api/admin/catalog` key assumptions
(`letter_distributions[].name`, `lexicons[].name`, `leave_sets[].lexicon`).

## Open PQs

- PQ-001 (Tailwind v4 dark variant) — provisional, non-blocking.
- PQ-002 (auth wire names/shapes) — provisional, non-blocking.
- PQ-003 (sliding TTL on sync only) — provisional, non-blocking.
- PQ-004 (governor can't peek; custom login-failure buckets) — provisional.

## Known failing tests

- None.
