# Progress

Resume from this file plus `docs/plan-index.md`. PLAN.md is the spec.

## Current

- **Phase:** 4 — Search engine (next to start)
- **Last green checkpoint:** Phase 3 — Catalog

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

### Phase 3 — Catalog ✅
- `fixtures/catalog/`: `english.csv`, `catalan.csv` (MAGPIE copies),
  `EN-FIX.tsv` (154 words: Zyzzyva help examples, SPORT/PORT/SPORTS, AEINRST
  ×9, Q/Z/V 7s, two 15s), `EN-FIX-OLD.tsv` (150; second lexicon on english for
  In Lexicon), `CA-FIX.tsv` (27 words with NY/QU/L·L/Ç incl. four made-up
  15-tile words), `EN-FIX-leaves.csv` (35), `CA-FIX-leaves.csv` (10),
  `manifest.json` (read by `stack.fixture_catalog()` and the test harness).
  Authored with a scratch script (not committed); edit the files directly.
- `fixtures/magpie-data/`: the 12 MAGPIE-DATA distributions at pinned commit
  2a9d656 + `fetch.sh`.
- `backend/src/catalog/`: `tiles.rs` (Distribution, strict MAGPIE parse,
  typed greedy parse, canonical leave, tile order = `[u8]` Ord),
  `probability.rs` (u128 combinations, two blanks; leave combos),
  `pos.rs`, `index.rs` (LexiconIndex/LeaveSetIndex with every derived
  attribute, ranks with min/max on ties, per-length/size buckets, alphagram
  map, canonical-leave lookup, maxima, build_ms, approx_bytes),
  `upload.rs` (validators; first 1,000 errors + total), `store.rs` (load +
  batched UNNEST inserts + `pg_notify`), `mod.rs` (Snapshot/Catalog,
  `startup`, `reconcile`, status rows, LISTEN on its own 1-connection pool,
  fallback reconcile, heartbeat), `routes.rs` (`/api/lexicons` gated on every
  live instance, `/api/letter-distributions/:name` from DB, `/api/admin/*`
  catalog/uploads/deletes; 120 s timeout; per-IP and admin-upload limits),
  `fixtures.rs` (cfg(test): loads fixture files into indexes for unit tests).
- `/health` 503 until the startup load completes (backend binds first).
- Purge task also prunes stale `catalog_instance_status`.
- Tests: 28 lib unit tests; `tests/catalog.rs` 20 integration tests.
- Frontend: `/admin` overview (delete actions disabled with reasons, loading
  badges, index sizes, instances), three upload pages via
  `components/admin/UploadForm.svelte` (XHR progress, error list),
  `components/NotFound.svelte` for non-admins.
- `stack.seed` waits until uploads are listed by `/api/lexicons`.
- Gate: `./scripts/dev.py` seeds the catalog through the real admin API,
  idempotently; restart reloads before `/health` is ready.
- Harness note: `#[sqlx::test]` pools share a 20-connection parent; long-held
  connections must not come from `state.db` (hence the listener's own pool).
  Integration suite takes ~2 min.

## Next

Phase 4 — Search engine. Re-read: Filters (1287–1566) incl. Groups, Pattern
syntax, Filter reference, lax rules, applicability; Search Engine
(1701–1816); Schema filter specs (3269–3433); API → Catalog and search
(4101–4165: preview, searches, filter JSON + parameter table); Configuration
(SEARCH_TIMEOUT_MS, SEARCH_CONCURRENCY, SEARCH_RATE_PER_MINUTE); Unit tests
backend search list (4888–4999); Zyzzyva parity (5992–6008); integration test
"Search concurrency". Build `backend/src/search/` (spec types, JSON wire
parse with extra-field refusal, validation keyed by path, candidate
shortcuts, evaluation, limits with lax/strict + intersection of kinds,
cooperative timeout every 4,096 candidates, questions), `POST
/api/search/preview`, semaphore + 503, property tests (add `proptest`
dev-dep), parity scaffold in `scripts/parity.py`.

## Open PQs

- PQ-001 (Tailwind v4 dark variant) — provisional, non-blocking.
- PQ-002 (auth wire names/shapes) — provisional, non-blocking.
- PQ-003 (sliding TTL on sync only) — provisional, non-blocking.
- PQ-004 (governor can't peek; custom login-failure buckets) — provisional.
- PQ-005 (/api/lexicons "startup load" test wording) — provisional.
- PQ-006 (≤256 tiles per distribution for u8 indexes) — provisional.
- PQ-007 (upload form errors use `line: null`) — provisional.

## Known failing tests

- None.
