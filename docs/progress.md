# Progress

Resume from this file plus `docs/plan-index.md`. PLAN.md is the spec.

## Current

- **Phase:** 6 — Cascade rules and sync (next to start)
- **Last green checkpoint:** Phase 5 — Cascade builder

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

### Phase 4 — Search engine ✅
- `backend/src/search/`: `wire.rs` (QuizType, GroupOp, ConditionType with
  negatable/is_limit/applies_to/fields; strict JSON parse of the filter tree
  with extra-field refusal and `PathError {path, field, message}`;
  deterministic `tree_to_json` via serde_json `preserve_order`),
  `pattern.rs` (canonical token grammar, `tokenize` without a distribution,
  `parse` against one, TileSet bitset, anagram/subanagram `Bag` with
  backtracking slot assignment, in-order DP matcher), `validate.rs` (group
  rules: ≤100 rows, ≤100 groups incl. top, depth ≤4, non-empty; per-row
  applicability, negation, ranges vs floor/ceiling with "narrows nothing",
  ceilings from target: 15/6, 15×/6× max tile value, max_num_anagrams,
  max_order_rank, word/leave count; canonical tiles/patterns, 500-char
  limit, `?` → "use `.` for any single tile", In Word List 300,000 total on
  the crossing row, entries ≤300 chars, invalid entries ignored, leave order
  checked; save mode = no target), `engine.rs` (shortcuts: top AND Length
  buckets + literal anagram; AND/OR group semantics; limits per kind with
  strict/lax reduce, widening capped by strict, kinds intersected; deadline
  check every 4,096 candidates and before each ranking; questions), `routes.rs`
  (`POST /api/search/preview` → `{count, sample, over_cap}`; `prepare()` and
  `run()` reusable by cascade creation; semaphore wait ≤ SEARCH_TIMEOUT_MS →
  503 Retry-After; deadline → 422 `search_too_broad`).
- Tests: `src/search/tests.rs` (Zyzzyva examples, inner hooks, groups, both
  limit kinds, lax/strict, limits, leave value, leaves, In Lexicon negated,
  validation messages, word-list totals, canonical length limit, deadline,
  proptest: shortcuts==full scan for words and leaves, negation partitions,
  OR/AND/limit subsets, literal anagram == alphagram entry);
  `tests/search.rs` (preview, path errors, 503 semaphore, search bucket,
  account binding).
- Parity: `fixtures/parity/searches.json` (13 single-AND searches incl.
  two-tile Not Includes and Length 7–8 + Limit 1–100), `deviations.json`,
  `scripts/parity.py` (skips without licensed data, printing each search in
  Zyzzyva form; with data: stack up, seed CSW24, create Definition cascades,
  read keys, compare with `zyzzyva/<name>.txt`, print index sizes/times and
  search times). Uses Phase 5 endpoints.

### Phase 5 — Cascade builder ✅
- Contract: `contract-fixtures/tools/gen_filters.py` reads PLAN.md's parameter,
  reference and applicability tables → `contract-fixtures/filters/conditions.json`
  (types, fields, negatable, limit, applies_to, labels, one valid condition
  per type with exact text, one extra-field condition). Checked by
  `backend/src/search/contract.rs` and `frontend/src/lib/filters.test.ts`.
- Backend: `search/store.rs` (spec ↔ rows: groups DFS-numbered, typed
  columns, entries deduped/sorted C collation (PQ-009), In Lexicon id for
  cascades / name for saved searches, `copy_spec`), `search/saved.rs`
  (GET list without trees + entry counts, GET one (search bucket), POST with
  id idempotency, save-mode validation, name_taken/overwrite, limit under the
  user-row lock, DELETE with spec), `cascade/order.rs` (SplitMix64,
  Fisher–Yates, reset XOR, FNV-1a questions_hash, i64 storage),
  `cascade/rows.rs` (CascadeRow/QuizRow wire form: u64 as unsigned decimal
  text, seqs as decimal text), `cascade/routes.rs` (POST /api/cascades:
  idempotent by device ids, 409 invalid for other users' ids, cheap limit
  check → search outside any tx → tx locking user row, re-check, seq bump,
  spec, cascade, UNNEST questions, Source quiz shuffle; start-over copying
  spec/threshold/options; cards with keys=1, no-store, download bucket;
  anagram answer shape PQ-008; `clamp_at` for device times ≤ now+5min).
- Tests: `tests/schema.rs` (6), `tests/saved_searches.rs` (7),
  `tests/cascades.rs` (13).
- Frontend: `lib/tiles.ts` (+tests), `lib/filters.ts` (table, ceilings,
  rangeError, wire parse/serialise, summary name, cutName) (+tests),
  `lib/options.ts`, `lib/sync/config.ts` (client constants, PQ-010),
  `lib/builder/preview.ts` (debounce/interval/reserve/busy/one retry)
  (+tests), `lib/builder/create.ts` (`sendWithRetry` with busy state)
  (+test), `lib/builder/form.ts` (row/group state, flags on type/lexicon
  change and load, toWire with client errors, saveBlocker, entries)
  (+tests), `lib/builder/tree-ops.ts`, `lib/catalog.ts`, components
  `TileText`, `builder/TilePalette`, `builder/WordListEditor`,
  `builder/FilterRow`, `builder/FilterGroup` (recursive, drag-and-drop),
  `QuizOptionsForm`; page `/cascades/new` (type, lexicon, tree, load/save
  dialogs, preview, threshold, options, auto name, create with retry);
  placeholder `/cascades/[id]`.
- TEMPORARY until Phase 7a: `lib/local/device.ts` keeps the device id in
  localStorage (must move into the per-user `meta` store); builder defaults
  use documented defaults instead of the preferences store; the cascade count
  for the limit warning is not shown yet (needs local cascades store).
- e2e: `tests/builder.spec.ts` (log in, preview 18, name, create).

## Next

Phase 6 — Cascade rules and sync. Re-read in full: Cascade Rules (78–427),
Cascades (1817–1964), Offline and Sync → Operations (2365–2475), The sync
cycle (2476–2797), Conflicts (2798–2884), Schema cascades/quizzes/sync
(3434–3826) + Purge task (3827–3870), API → Cascades and sync (4166–4253:
sync body/response, questions and grades endpoints), Testing → Contract
fixtures (4782–4883), frontend unit list (5000+ for lib/cascade), integration
sync tests (5334–5565). Order: (1) contract fixtures FIRST with independent
reference scripts under `contract-fixtures/tools/` (shuffle vectors, question
hashes incl. >2^63, leave value text, rule vectors); (2) Rust
`backend/src/cascade/rules.rs` + TS `frontend/src/lib/cascade/` passing them;
(3) `/api/sync` (push with savepoints, transient vs `error`, acked_below,
paged pull with ceiling in page token, tombstones, resync_required, 426),
questions + grades endpoints, sliding session renewal on sync (PQ-003); (4)
purge task (per-user cap, trashed cascades whole, sync record pruning).

## Open PQs

- PQ-001 (Tailwind v4 dark variant) — provisional, non-blocking.
- PQ-002 (auth wire names/shapes) — provisional, non-blocking.
- PQ-003 (sliding TTL on sync only) — provisional, non-blocking.
- PQ-004 (governor can't peek; custom login-failure buckets) — provisional.
- PQ-005 (/api/lexicons "startup load" test wording) — provisional.
- PQ-006 (≤256 tiles per distribution for u8 indexes) — provisional.
- PQ-007 (upload form errors use `line: null`) — provisional.
- PQ-008 (anagram card answer shape; creation sync_seq as text) — provisional.
- PQ-009 (In Word List entries are a set, returned in byte order) — provisional.
- PQ-010 (client SEARCH_RATE constant; structural invalid-entry count) — provisional.

## Known failing tests

- None.
