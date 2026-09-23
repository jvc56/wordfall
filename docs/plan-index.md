# PLAN.md index

Every heading in `PLAN.md` with its line range (inclusive; a range runs to the
line before the next heading of the same or higher level). Regenerate the table
with `grep -n '^#' PLAN.md` if PLAN.md changes.

| Heading | Lines |
|---|---|
| # Wordfall — Project Plan | 1–6061 |
|   ## Overview | 3–58 |
|   ## Glossary | 59–77 |
|   ## Cascade Rules | 78–427 |
|     ### The Source quiz | 131–177 |
|     ### Quiz options | 178–237 |
|     ### Progression: Ladder or Drill | 238–277 |
|     ### Segments | 278–359 |
|     ### Trash, restore and purge | 360–404 |
|     ### Cascade limit | 405–427 |
|   ## User Experience | 428–1030 |
|     ### Accounts | 430–437 |
|     ### Preferences | 438–462 |
|     ### Creating a cascade | 463–550 |
|     ### Cascades page | 551–577 |
|     ### Trash page | 578–603 |
|     ### Taking a quiz | 604–879 |
|       #### Desktop layout | 616–649 |
|       #### The three actions | 650–673 |
|       #### Answers | 674–703 |
|       #### Typed mode (Anagram quizzes only) | 704–761 |
|       #### Controls | 762–807 |
|       #### Touch zones | 808–873 |
|       #### Saving progress | 874–879 |
|     ### Finishing a quiz | 880–907 |
|     ### Exporting words | 908–1030 |
|   ## Admin | 1031–1222 |
|     ### Uploads | 1039–1052 |
|     ### File formats | 1053–1166 |
|       #### Letter distribution file | 1077–1124 |
|       #### Lexicon file | 1125–1145 |
|       #### Leave values file | 1146–1166 |
|     ### Upload limits | 1167–1173 |
|     ### Immutability and deletion | 1174–1202 |
|     ### Loading changes into running servers | 1203–1222 |
|   ## Tiles | 1223–1286 |
|   ## Filters | 1287–1566 |
|     ### Groups | 1306–1361 |
|     ### Pattern syntax | 1362–1387 |
|     ### Filter reference | 1388–1488 |
|     ### Probability and probability order | 1489–1508 |
|     ### Filter applicability by quiz type | 1509–1566 |
|   ## Architecture | 1567–1641 |
|     ### Tech Stack | 1594–1621 |
|     ### Repository layout | 1622–1641 |
|   ## Catalog Indexes | 1642–1700 |
|     ### Derived attributes | 1644–1700 |
|   ## Search Engine | 1701–1816 |
|   ## Cascades | 1817–1964 |
|     ### Questions are stored once per cascade | 1819–1829 |
|     ### Deterministic shuffles | 1830–1867 |
|     ### Rule implementation | 1868–1923 |
|     ### Working at 300,000 questions | 1924–1964 |
|   ## Offline and Sync | 1965–2995 |
|     ### Principles | 1967–1978 |
|     ### What needs a connection | 1979–1990 |
|     ### On the device | 1991–2364 |
|     ### Operations | 2365–2475 |
|     ### The sync cycle | 2476–2797 |
|     ### Conflicts | 2798–2884 |
|     ### Authentication while offline | 2885–2995 |
|   ## Schema | 2996–3870 |
|     ### Purge task | 3827–3870 |
|   ## Authentication | 3871–4056 |
|   ## API | 4057–4270 |
|     ### Auth and account | 4084–4100 |
|     ### Catalog and search | 4101–4165 |
|     ### Cascades and sync | 4166–4253 |
|     ### Admin | 4254–4270 |
|   ## Frontend | 4271–4415 |
|   ## Configuration | 4416–4464 |
|   ## Development | 4465–4563 |
|     ### One command | 4467–4524 |
|     ### How it is put together | 4525–4543 |
|     ### Day to day | 4544–4563 |
|   ## Deployment and Operations | 4564–4667 |
|   ## Backups | 4668–4748 |
|     ### What is backed up | 4674–4689 |
|     ### Alarms | 4690–4697 |
|     ### Restoring | 4698–4726 |
|     ### The drill | 4727–4739 |
|     ### Locally | 4740–4748 |
|   ## Testing | 4749–6026 |
|     ### The fixture catalog | 4765–4781 |
|     ### Contract fixtures | 4782–4883 |
|     ### Unit tests | 4884–5298 |
|     ### Integration tests | 5299–5664 |
|     ### End-to-end tests | 5665–5914 |
|       #### What the tests reuse | 5672–5703 |
|       #### The journeys | 5704–5914 |
|     ### Scale tests | 5915–5991 |
|     ### Zyzzyva parity | 5992–6008 |
|     ### Running the tests | 6009–6026 |
|   ## Delivery Phases | 6027–6061 |
Notable anchors inside long sections:

- Schema SQL begins at 3008; accounts 3103, catalog 3193, filter specs 3269,
  cascades and quizzes 3434; Purge task 3827.
- Unit tests: backend list from 4888, frontend list from 5000.

## Sections each delivery phase depends on

Always re-read the phase's own bullet in Delivery Phases (6027–6061) and the
relevant part of Testing (4749–6026) before starting.

### Phase 1 — Skeleton
- Architecture, Tech Stack, Repository layout (1567–1641)
- Schema (2996–3870) → `backend/migrations/0001_initial.sql`
- API preamble (4057–4083) for `/health` and error shape
- Frontend (4271–4415): shell, CSP, nginx, build-time CSP check
- Configuration (4416–4464)
- Development (4465–4563): `scripts/stack.py`, `scripts/dev.py`, compose
- Deployment and Operations (4564–4667): Terraform baseline
- Testing: Running the tests (6009–6026)

### Phase 2 — Accounts
- UX → Accounts (430–437), Preferences (438–462)
- Authentication (3871–4056) incl. security bullets, rate limits, CSRF
- Offline and Sync → Authentication while offline (2885–2995): `X-Wordfall-User`
- API preamble (4057–4083), API → Auth and account (4084–4100)
- Schema accounts tables (3103–3192)
- Configuration (4416–4464)
- Testing → Integration tests (5299–5664), auth parts

### Phase 3 — Catalog
- Admin (1031–1222): uploads, file formats, limits, immutability, NOTIFY reload
- Tiles (1223–1286)
- Filters → Probability and probability order (1489–1508)
- Catalog Indexes (1642–1700)
- Schema catalog tables (3193–3268), `catalog_instance_status`
- API → Catalog and search (4101–4165), API → Admin (4254–4270)
- Testing → fixture catalog (4765–4781), backend unit tests (4888–4999),
  integration tests (5299–5664) admin parts

### Phase 4 — Search engine
- Filters (1287–1566): Groups, Pattern syntax, Filter reference, applicability
- Search Engine (1701–1816)
- Catalog Indexes (1642–1700)
- Schema filter specs (3269–3433)
- API → Catalog and search (4101–4165)
- Configuration (`SEARCH_CONCURRENCY`, timeouts)
- Testing → backend unit tests (4888–4999), Zyzzyva parity (5992–6008)

### Phase 5 — Cascade builder
- UX → Creating a cascade (463–550); Quiz options (178–237); Cascade limit (405–427)
- Filters → applicability (1509–1566)
- Cascades → Questions stored once (1819–1829), Working at 300,000 (1924–1964)
- API → Catalog and search (4101–4165), Cascades and sync (4166–4253)
- Frontend (4271–4415)
- Schema filter specs + cascades (3269–3500+)
- Testing → Contract fixtures (4782–4883, the filter contract test)

### Phase 6 — Cascade rules and sync
- Cascade Rules (78–427), all subsections
- Cascades (1817–1964): shuffles, rule implementation
- Offline and Sync → Operations (2365–2475), The sync cycle (2476–2797),
  Conflicts (2798–2884)
- Schema cascades, quizzes, sync (3434–3826), Purge task (3827–3870)
- API → Cascades and sync (4166–4253)
- Testing → Contract fixtures (4782–4883), unit (4884–5298), integration (5299–5664)

### Phase 7 — Local-first player
- User Experience (428–1030) in full
- Offline and Sync (1965–2995) in full, especially On the device (1991–2364)
- Cascades (1817–1964)
- Frontend (4271–4415)
- API → Cascades and sync (4166–4253)
- Testing → frontend unit (5000–5298), E2E (5665–5914), Scale (5915–5991)

### Phase 8 — Production
- Deployment and Operations (4564–4667)
- Backups (4668–4748)
- Configuration (4416–4464), Development (4465–4563)
- Authentication (email/SES parts)
