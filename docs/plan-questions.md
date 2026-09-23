# Plan questions

Ambiguities, contradictions or impossibilities found in PLAN.md, with the
provisional choice made. Code affected is marked `// PQ-nnn`.

## PQ-001 — Tailwind `darkMode: 'class'` under Tailwind v4 (open)

- **Plan:** Tech Stack (PLAN.md 1604): "shadcn-svelte, dark mode only (Tailwind
  `darkMode: 'class'` with `dark` always on the root)".
- **Issue:** `darkMode: 'class'` is Tailwind v3 config syntax. Current
  shadcn-svelte (1.x) requires Tailwind v4, which has no `darkMode` key; the v4
  equivalent is `@custom-variant dark (&:where(.dark, .dark *));` in CSS.
- **Options:** (a) Tailwind v4 + current shadcn-svelte with the class-based
  custom variant; (b) Tailwind v3 + the legacy shadcn-svelte 0.x line.
- **Provisional choice:** (a). Behaviour is identical (class-based dark mode,
  `class="dark"` always on `<html>`). Marked in `frontend/src/app.css`.

## PQ-002 — Auth wire details the plan leaves unnamed (open)

- **Plan:** Authentication (3871–4056), API → Auth and account (4084–4100).
- **Issue:** The plan names the session cookie (`wordfall_session`) and the
  endpoints but not: the CSRF cookie and header names; the shape of the
  register `400` ("every field error at once"); the success statuses and
  bodies of register, confirm, reset and the account endpoints; the request
  field names of confirm, reset-confirm, change password and delete; the
  password length bounds ("strength with zxcvbn (score ≥ 3) and length").
- **Provisional choices:** CSRF cookie `wordfall_csrf`, header
  `X-CSRF-Token`; field errors `400 { errors: [{ field, message }] }` (the
  search endpoints' `{ path, field, message }` minus `path`); register and
  reset-password `202 {}`; confirm, reset-confirm, sign-out-everywhere,
  password and delete `204`; login `200` with the `/api/auth/me` body;
  bodies `{ code }`, `{ email }`, `{ token, password }`,
  `{ current_password, new_password }`, `{ password }`; passwords 8–128
  characters. Wrong credentials `401 { error: "unauthorized" }`, unconfirmed
  `403 { error: "email_unconfirmed" }`, bad code/token
  `400 { error: "invalid_code" | "invalid_token" }`.
- Marked in `backend/src/auth/session.rs`, `backend/src/auth/password.rs`,
  `backend/src/error.rs`, `frontend/src/lib/api.ts`.

## PQ-003 — Which responses slide the session TTL (open)

- **Plan:** Authentication (3946–3949): "a sync made in the last seven days of
  a cookie's life is answered with a fresh cookie of the full TTL".
- **Issue:** Only sync is named; `GET /api/auth/me` "re-sets" the CSRF cookie
  but is not said to renew the session.
- **Options:** (a) renew on sync only; (b) renew on every authenticated request.
- **Provisional choice:** (a), literal. `session::renewal` is applied by the
  sync handler (Phase 6); its integration test lands with the sync endpoint.

## PQ-004 — `governor` cannot express the login-failure buckets (open)

- **Plan:** Tech Stack names `governor`; Authentication → Login requires a
  request refused "before any Argon2 verify runs when either bucket is empty",
  with "a token taken from both only when the verify fails" and "a successful
  login spends nothing".
- **Issue:** `governor` only offers check-and-take; it cannot peek at a bucket
  without spending it, nor refund.
- **Provisional choice:** every other limit uses `governor`; the two
  login-failure limits use a small in-house bucket with the same quota
  semantics (`n` per minute, burst `n`) that can be inspected before it is
  spent (`backend/src/rate.rs`, `FailureBuckets`).

## PQ-005 — "/api/lexicons omits an item while a second instance is still completing its startup load" (open)

- **Plan:** Integration tests (5390–5393) vs. Admin → Loading changes
  (1214–1216): "An instance writes **no** rows until its startup load is
  complete ... so a task still booting during a deploy never makes an item
  look unloaded".
- **Issue:** A booting instance has no rows, so it is not "live" and cannot
  hold an item back; the test sentence reads as if it should.
- **Provisional choice:** follow the Admin section (the mechanism). The test
  is written as: an item is omitted while a **live** second instance (one with
  rows) has not yet loaded it, and listed once that instance writes its row;
  a separate assertion checks that a booting instance hides nothing
  (`backend/tests/catalog.rs`).

## PQ-006 — A distribution's tile count is capped at 256 (open)

- **Plan:** Search Engine (1737–1739) has the engine compare "small tile
  indexes (`u8`)"; File formats sets no cap on the number of tiles.
- **Provisional choice:** the upload validator refuses a distribution with
  more than 256 tiles (every MAGPIE-DATA file has under 40), so a tile index
  always fits a `u8` (`backend/src/catalog/tiles.rs`, `upload.rs`).

## PQ-007 — Upload errors that belong to no line (open)

- **Plan:** API → Admin: "`400` with `{ errors: [{ line, message }], total_errors }`".
- **Issue:** Form-level problems (name taken or malformed, unknown
  distribution or lexicon, lexicon already has leave values, missing file,
  empty file, body too large) have no line.
- **Provisional choice:** the same `400` shape with `line: null`.

## PQ-008 — The shape of an Anagram card's `answer` (open)

- **Plan:** API → Cascades and sync (card pages): cards are
  `[{ idx, key, answer }]`; a Leave Value answer is a JSON number; Answers
  (674–703) describes what is shown, not the JSON.
- **Provisional choice:** Definition → the definition string; Leave Value →
  the number (shortest round-trip text); Anagram → an array, in alphabetical
  order, of `{ word, front_hooks?, back_hooks?, definition? }`, words and hook
  lists in MAGPIE notation (hooks already in tile order, `""` when none),
  `front_hooks`/`back_hooks` present only with `hooks=1` and `definition` only
  with `definitions=1`. Creation's `sync_seq` is decimal text like every other
  sequence on the wire. (`backend/src/cascade/routes.rs`)

## PQ-009 — In Word List entry order (open)

- **Plan:** Integration tests (5582–5585): a saved tree with a 10,000-entry
  In Word List read back "equal to what was sent, child order included";
  Schema: `search_condition_words` has `PRIMARY KEY (spec_id, position,
  entry)` and no ordering column.
- **Issue:** Entry order and duplicates cannot survive the schema.
- **Provisional choice:** entries are a set: stored deduplicated and read back
  sorted by byte order (`COLLATE "C"`). The builder's WordListEditor sends
  them sorted and deduplicated, so a round trip is exact.
  (`backend/src/search/store.rs`)

## PQ-010 — What the builder knows client-side (open)

- **Plan:** Creating a cascade (step 8) has the preview keep a reserve of "all
  but two of `SEARCH_RATE_PER_MINUTE`"; Frontend → WordListEditor shows "how
  many entries are not valid in the lexicon".
- **Issue:** `SEARCH_RATE_PER_MINUTE` is server configuration the client is
  never told (`/api/auth/me` reports only the retention period and the question
  cap), and lexicon membership needs the server's word list.
- **Provisional choices:** the reserve is measured against the default, 30, a
  constant in `lib/sync/config.ts`; the editor's "not valid" count is
  structural — entries that do not parse in the distribution for the quiz
  type, have the wrong tile count, exceed the bag (leaves) or are not in
  canonical order — which is also what the plan's type-change recount test
  describes. Words that parse but are not in the lexicon are ignored by the
  server's search as the plan says.

## PQ-011 — "exactly 8 chain quizzes" from 40 questions at S = 5 (open)

- **Plan:** Contract fixtures (4822–4824): "an attempt of 40 questions with
  S = 5 and every run missing something creating exactly 8 chain quizzes, the
  `ceil(question_count / S)` bound"; Segments (292–297): only a run that is
  **not the last run** descends as a chain quiz, and "the last run always ends
  at the last question, where finishing the attempt applies the rules above".
- **Issue:** 40 / 5 has 7 boundaries strictly inside the quiz, so the rules
  create 7 segment (chain) quizzes; the 8th run's misses go to the `finish`,
  which creates one descent (or replacement) quiz, not a chain quiz. The bound
  `ceil(q / S)` is met by the attempt's created quizzes, 7 + 1 = 8.
- **Provisional choice:** the rules as written. The vector
  (`contract-fixtures/cascade/rules.json`, "the ceil(question_count / S)
  bound") and both modules' tests assert 8 quizzes created by the attempt, of
  which 7 are segment chains.

## PQ-012 — The wire form of `cursor`, `seen_seq` and `sync_seq` (open)

- **Plan:** API (4177) gives the sync body `{ device_id, app_version, cursor, … }`
  and the response's `sync_seq` without a type; § API (4186–4196) has every
  pulled row's `updated_seq`, tombstone `seq` and `min_updated_seq` "as decimal
  text".
- **Issue:** Sequences are `BIGINT`s; the plan says nothing about whether the
  request's `cursor` and each operation's `seen_seq` are JSON numbers or text,
  or how `sync_seq` is sent.
- **Provisional choice:** `sync_seq` is sent as decimal text, like the other
  sequences on the wire; `cursor`, `seen_seq` and `device_seq` are accepted
  as a JSON integer or as decimal digits (`backend/src/sync/routes.rs`,
  `// PQ-012`). The device sends numbers (sequences stay far below 2^53).

## PQ-013 — Check order for `finish` and `finish_segment` (open)

- **Plan:** Operations table (2371–2372) lists, for both, "the quiz is active,
  its cascade is not trashed, it is at the deepest level, the attempt and its
  seed match, …, no quiz already exists for this quiz, attempt and segment
  end". Conflicts (2805, 2810, 2811, 2816) and the Sync integration tests
  (5423–5428, 5480–5484): a second `finish` after one that **reset** the quiz
  is `stale_attempt`; the loser of a race on one boundary, and a device whose
  run another device already passed, get `duplicate_segment`; "a
  `finish_segment` after a `finish` is rejected because its attempt is out of
  date".
- **Issue:** Read as an order, the table's list makes those outcomes
  unreachable: a finish that descended, and a `finish_segment` that drilled,
  both add a level below the quiz, so the depth check would answer
  `not_deepest` first.
- **Provisional choice:** the table's conditions all hold, checked in the order
  the Conflicts table and the tests require: live (active, not trashed), then
  the attempt and seed, then — for `finish_segment` — the segment end's
  validity and the duplicate, then the depth, then the cursor test and the
  grading. Applied identically in `contract-fixtures/tools/reference.py`,
  `backend/src/cascade/sim.rs`, `backend/src/sync/ops.rs` and
  `frontend/src/lib/cascade/sim.ts` (each marked `PQ-013`), with a new rule
  vector ("the attempt and a duplicate run come before the depth") that pins
  it. The "Two different quizzes restored" row's `not_deepest` is unaffected,
  since that finish carries a current attempt.

## PQ-014 — "K answers from this device weren't kept." when K is 1 (open)

- **Plan:** Conflicts (after the table): 'The second sentence, "K answers
  from this device weren't kept.", appears only when K is greater than 0.'
- **Issue:** Read literally, K = 1 gives "1 answers … weren't kept."
- **Provisional choice:** "1 answer from this device wasn't kept." for one,
  the literal sentence otherwise (`frontend/src/lib/sync/notices.ts`,
  `keptSentence`).

## PQ-015 — Where the device learns the cascade limit (open)

- **Plan:** Cascade limit (405–427): "The Cascades page shows `87 of 100
  cascades`, and the cascade builder shows a warning from 90";
  API (4094): `/api/auth/me` returns `{ user_id, username, is_admin,
  trash_retention_days, max_quiz_questions }` — no limit.
- **Issue:** `MAX_CASCADES_PER_USER` is configurable on the server, but no
  endpoint reports it before a `409 { error: "cascade_limit", limit }`.
- **Provisional choice:** the device uses the documented default, 100, as a
  client constant (`frontend/src/lib/cascades/summary.ts`, `CASCADE_LIMIT`),
  with the warning from 90, as PQ-010 does for the search rate. The server's
  `409` still refuses creation at its own configured limit.
