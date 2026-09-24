# Progress

Resume from this file plus `docs/plan-index.md`. PLAN.md is the spec.

## Current

- **Phase:** 7h — e2e journeys and scale tests (in progress)
- **Last green checkpoint:** Phase 7g — export
- **7h status:** journeys written in `e2e/tests/` (support.ts helpers:
  newUser via console-log code, createCascade, apiCascade, playLevel, idb,
  outboxCount): rules ✅, two-devices ✅, touch ✅, player ✅, pages ✅,
  shell ✅; being fixed: accounts, admin, controls, misc, plane, segments,
  typed, tabs, journeys. Configured passes (`@env` tags, own stacks via
  E2E_PROJECT/E2E_PORT/E2E_ENV in the Makefile): `@ttl` (PQ-016), `@purge`,
  `@limits` in `env.spec.ts` — not yet run. Not yet written: updating the app
  (second build, MIN_APP_VERSION), the 10,001-entry preview pause, the answer
  limit / ROW_STORAGE_BUDGET journeys (need lowered client constants), export
  with the backend stopped mid-download, CSP meta-policy checks. Scale tests
  (`scripts/scale.py`) still a placeholder.

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

### Phase 6 — Cascade rules and sync ✅
- Contract fixtures (independent of the code under test):
  `contract-fixtures/tools/reference.py` (Python model of the rules, shuffle,
  hash, leave text) → `gen_cascade.py` → `cascade/rules.json` (30 vectors,
  3,381 steps), `cascade/random.json` (40 seeded sequences), `shuffle.json`,
  `hash.json`, `leave_text.json`.
- Rules: `backend/src/cascade/{order,rules,sim,vectors}.rs` and
  `frontend/src/lib/cascade/{order,rules,sim,leave}.ts` pass every vector.
  Check order per PQ-013 (attempt, then duplicate, before depth).
- Sync: `backend/src/sync/` — `routes.rs` (`POST /api/sync`: request-level
  400s, user-row lock, cursor-above-seq resync, savepoints, transient → 503,
  `error` recording, acked_below pruning, 426, floor resync, paged pull,
  session renewal PQ-003; questions + grades endpoints), `ops.rs` (13 op
  kinds, conflicts, clamping, restart_clocks, tombstones,
  `purge_cascade_rows`), `pull.rs` (page token with ceiling + qrf, 50,000
  rows per page, `min_updated_seq`), `prefs.rs` (preferences/bindings
  validation). PQ-012 (sequence wire forms).
- Purge task: `backend/src/purge.rs` (advisory lock, per-user transactions,
  cap oldest-first, trashed cascades whole, sync record pruning raising
  `sync_floor_seq`, stale unconfirmed, export tokens, catalog status).
- Tests: `tests/sync_vectors.rs` (all rule vectors through the server, DB
  state + invariants), `tests/sync.rs` (19), `tests/conflicts.rs` (16, every
  Conflicts row), `tests/pull.rs` (9), `tests/purge.rs` (7),
  `tests/sync_faults.rs` (6: injected CHECK error, forced serialization
  failure, purge vs sync, limits, session renewal, 300k finish/reset).
  `tests/common/sync.rs` asserts every rejection reason is in the fixed list.

### Phase 7a — Per-user stores ✅
- `lib/local/db.ts` (schema v1: meta, preferences, distributions, base /
  overlay / staging row stores with cascade/quiz/position indexes, questions,
  cards, outbox with cascade_id and cursor_quiz indexes; versioned
  migrations; closes on `versionchange`), `rows.ts` (row types, wire
  converters), `meta.ts` (identity, device id + next device_seq, sync cursor,
  server values, opens, keep offline, opt-outs, budget-dropped; `recordOpen`),
  `view.ts` (overlay-over-base reads), `apply.ts` (`applyOp`, the
  store-backed twin of `cascade/sim.ts`; `applyLocally` minting device_seq,
  coalescing move_cursor, recording opens, all in one transaction),
  `preferences.ts` (defaults + pending ops laid over the base), `open.ts`
  (one connection per tab; "updated in another tab" notice).
- Session opens the DB on startup/login, stores `/api/auth/me` values in
  meta, closes it on logout/removal, calls `navigator.storage.persist()`.
  The temporary `lib/local/device.ts` is gone; the builder reads the device
  id from meta.
- Tests: `lib/local/apply.test.ts` (all 30 rule vectors through
  `applyLocally` with invariants, two writers, move_cursor coalescing after
  a finish_segment, rejection writes nothing, opens, seen_seq, a second
  tab's write during a rebase, fixture DB per earlier version, versionchange,
  device id, preferences view).

### Phase 7b — Sync engine ✅
- `lib/sync/protocol.ts` (wire types, BATCH_OPS 500), `pull.ts` (pages:
  question rows straight to the base for a new quiz or an unchanged attempt,
  nowhere for a pending quiz, else staging; tombstones/preferences in memory;
  per-cascade touched and foreign-sequence sets), `rebase.ts` (steps 1–4 per
  cascade in one transaction with the outbox; promotion by attempt/seed and
  hash; rebuild/materialisation/pending; fast path by row sequences; full
  pull deletion by `seq ≤ S`, sparing cascades with pending ops or a later
  creation; cursor advances only at the end), `notices.ts` (sentences by
  reason, owners and dependents, PQ-014), `policy.ts` (question_rows_for:
  window, keep offline, auto keep, rows held), `engine.ts` (batches,
  back-to-back drain, 429/503 Retry-After, 401 needs_login, 426 reload flag
  in meta, resync → full pull, Web Locks leader, triggers),
  `runtime.svelte.ts` (leader tab runs it; BroadcastChannel status/kick),
  `lib/local/created.ts` (a created cascade into the base at once).
- `__APP_BUILD__`/`__APP_COMMIT__` via Vite define from WORDFALL_BUILD /
  WORDFALL_COMMIT (Docker build args APP_BUILD/APP_COMMIT).
- Tests: `lib/sync/engine.test.ts` (33) against `testing/fake-server.ts`
  (rules via SimCascade, per-row sequences, paging); e2e builder journey now
  asserts the new cascade's 18 rows are in IndexedDB and the cursor advanced
  against the real server.

### Phase 7c — Download manager ✅
- `lib/sync/downloads.ts`: after every pull (and on first open / Keep
  offline / restore via `ensureCascade`): drop pass (window, then
  ROW_STORAGE_BUDGET in two tiers with the budget-dropped mark; skips pending
  ops, open players via Web Locks `wordfall-player-<id>` or this tab's own
  open cascade), distributions into the `distributions` store, index lists
  deepest level first, pending quizzes' grades (sync first; attempt/seed
  mismatch → discard and sync again; completeness vs counters; positions;
  hash), keys (`keys=1`) for every wanted cascade before any answers, cards
  with the extras the preferences ask for (refetch when lacking), eviction
  oldest-open first sparing user-kept and pending cascades, quota path (drop
  pass, eviction, one retry, then `noRoom`), 429 → wait and refetch the same
  page, player priority (`forPlayer`), totals to the accounts row. Sizes of
  keys/answers measured as written (`meta.sizes`), rows at ROW_BYTES each.
- `lib/sync/keep.ts` (Keep offline on = open + fetch now; off = opt-out).
- Runtime runs the manager after each pull without blocking the cycle.
- Tests: `lib/sync/downloads.test.ts` (16).

### Phase 7d — Service worker ✅
- `src/service-worker.ts` (precache build + files + /index.html; every app
  navigation → cached shell; /api and unrecognised requests untouched; no
  skipWaiting on install, only on SKIP_WAITING; old build caches deleted
  only when no page reports that build, re-checked on CLOSING),
  `lib/sw/logic.ts` (pure routing and cache decisions, tested),
  `lib/sw/client.svelte.ts` ("a new version is ready" offer; only the tab
  that asked reloads on controllerchange; others show "updated in another
  tab"; HELLO/CLOSING with the build version). Layout banners.
- e2e `shell.spec.ts`: shell loads offline once the worker controls the page.

### Phase 7e — The player ✅
- `lib/player/controller.ts` (state machine over the local view: reveal /
  toggle / save-and-advance, move_cursor, finish_segment at a run's last
  card and finish at the attempt's, Previous never below run_start with the
  "already finished this part" line, missing key → "needs a connection",
  moved past with nothing emitted, run end waits with a count; missing
  answer → flashcard fallback; quota → storage message and retry; on-demand
  card page via `DownloadManager.fetchForPlayer`; `refreshCard` on download
  progress), `typed.ts`, `bindings.ts` (strokes, input protection, bind /
  unbind rules, debounce), `banner.ts`, `ladder.ts`.
- Components: `CascadeLadder.svelte` (collapse above 12; compact form),
  `player/AnswerView.svelte`, `SyncStatus.svelte`, `Notices.svelte`
  (layout-wide). Route `/cascades/[id]`: desktop rails / touch zones,
  mouse/wheel/key bindings, typed input with the three-button row, settings
  (preferences + quiz options), completion screen (Keep studying, Start over,
  Move to Trash), trashed → Restore, Web Lock `wordfall-player-<id>`.
- Runtime: `changed` signal (step 5 refresh), `progress`, `fetchCard`,
  `ensureCascade` in any tab. Vitest: jsdom + @testing-library/svelte
  (`resolve.conditions: ['browser']` under VITEST).
- Tests: `lib/player/player.test.ts` (14), `CascadeLadder.test.ts` (2);
  e2e `player.spec.ts` (study with keys, offline, reload offline, drain).

### Phase 7f — Cascades, Trash and Account pages ✅
- `lib/cascades/summary.ts` (options summary, ladder text, offline badge
  states, Trash groups with provisional purge dates, CASCADE_LIMIT 100 /
  warning 90 — PQ-015). `/cascades` (limit, sync status, compact ladder,
  badges, Quiz options dialog, Export…, Start over, Keep offline with the
  automatic-keep hint, Move to Trash), `/trash` (groups collapsed, pages of
  100 behind Show more, single-entry groups open, Restore / Export… /
  Delete forever, "downloads when online"), `/account` (+ SyncStatus,
  `PreferencesCard`, `ControlsEditor` with capture box, `StorageCard` with
  budget line, kept cascades, accounts and removal). SyncStatus shows "N
  changes waiting to sync". Builder: defaults from the preferences view
  (clamped to the cap), cascade-count warning and limit.
- Tests: `lib/cascades/summary.test.ts` (4); e2e `pages.spec.ts`.
- Export… buttons link to `/cascades/:id/export` (7g).

### Phase 7g — Export ✅
- Contract: `contract-fixtures/tools/gen_export.py` (independent reference
  from the plan's text) → `contract-fixtures/export/cases.json` (329 cases:
  three types, four selections, both formats, both orders, empty files,
  MAGPIE tiles, CSV quoting, ` / ` and ` | ` in definitions, four front
  hooks, leave values at 0–3 decimals, filenames with `·`/`–`, a non-BMP
  character and >100 chars, cascade-wide union, a quiz in the Trash).
- Rust `backend/src/export/format.rs` (chunked `format_each`) and TS
  `frontend/src/lib/export/format.ts` (generator) both pass every case.
- Server `backend/src/export/mod.rs`: `POST …/export-token` (session,
  binding, 404s, EXPORT_RATE_PER_MINUTE; PASETO under the HKDF export key
  with a SHA-256 of the canonical choices, 60 s, jti) and
  `GET …/export` (204 for expired/reused/mismatched; `export_tokens_spent`
  records the one use; streams through a channel from a blocking task;
  Content-Disposition filename). Added `tokio-stream`.
- Device: `lib/export/local.ts` (input from local stores or what is
  missing), `worker.ts` (chunks → Blob), route `/cascades/[id]/export`
  (dialog, live count, materialise first, server fallback through a hidden
  iframe, offline → "questions alone"), player ladder Export….
  `DownloadManager.materialiseQuiz(…, force)` fetches a cleared quiz's rows.
- Tests: Rust fixture test; `tests/export.rs` (7); Vitest fixtures (329) +
  `local.test.ts` (5); e2e downloads (local and server paths).

## Next

Phase 7 — Local-first player, split into sub-milestones, each committed at
green: (done: 7a–7d) **7a** IndexedDB per-user stores (`meta` incl. device id — replace the
temporary `lib/local/device.ts`), base/overlay/staging/outbox and
`applyLocally` using `lib/cascade/sim.ts` rules; **7b** sync engine (push,
paged pull, rebase, notices, resync); **7c** download manager (window,
budget, keep offline, card/key pages, questions endpoint); **7d** service
worker; **7e** player; **7f** cascades, trash and account pages (builder
defaults from preferences, cascade-count warning); **7g** export; **7h** e2e
journeys and scale tests. Re-read before 7a: User Experience (428–1030),
Offline and Sync (1965–2995, esp. On the device 1991–2364), Cascades
(1817–1964), Frontend (4271–4415), API → Cascades and sync (4166–4253),
frontend unit tests (5000–5298).

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
- PQ-011 (40 questions at S=5: 7 chains + 1 descent) — provisional.
- PQ-012 (sequence wire forms: sync_seq text, cursor number or text) — provisional.
- PQ-013 (finish/finish_segment check order) — provisional.
- PQ-014 (singular "1 answer … wasn't kept") — provisional.
- PQ-015 (cascade limit as a client constant) — provisional.

## Known failing tests

- None.
