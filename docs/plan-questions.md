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
