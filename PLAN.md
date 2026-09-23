# Wordfall — Project Plan

## Overview

Wordfall is a word study website for crossword game players. A logged-in user
describes the words they want with filters, and Wordfall turns the matches into
a **cascade** of flashcard quizzes. Getting a quiz at least X% right **clears**
it and removes it from the cascade. Missing too many sends the missed questions
one **level** down. The **Source quiz** at Level 1 is the exception: it is never
cleared and stays until the user trashes the cascade. The user works down the
cascade and back up until the Source quiz is finished with no misses, which
makes the cascade **complete**.

There are three quiz types:

| Quiz type | Question | Answer |
|---|---|---|
| **Anagram** | An alphagram (the tiles of a word in alphabetical order), e.g. `AEINRST` | Every valid word in the lexicon made from exactly those tiles, e.g. `ANESTRI, ANTSIER, NASTIER, RATINES, RETAINS, RETINAS, RETSINA, STAINER, STEARIN` |
| **Definition** | A word, e.g. `QAT` | That word's definition, e.g. `an evergreen shrub [n -S]` |
| **Leave Value** | An alphabetized set of 1–6 tiles, e.g. `?EIRS` | The leave's value, e.g. `+34.1` |

Twenty of the filters match search conditions in Zyzzyva
(<https://github.com/scrabblewords/collins-zyzzyva>), so experienced players can
describe a word list the way they already know how. Three are Wordfall-only:
**Front Inner Hook** and **Back Inner Hook**, which Zyzzyva shows in its word
display but has no search condition for, and **Leave Value**, for Leave Value
quizzes. Filters combine with AND and OR in nested groups whose precedence the
user chooses. [Filters](#filters) below explains what each filter means and how
Zyzzyva implements it.

Word study data comes in three kinds, all uploaded by admins:

- **Letter distributions** (English, Catalan, Polish, …) define the tiles: how
  each is written, how many of each are in the bag, their point values, and which
  are vowels. They use MAGPIE's letter distribution format.
- **Lexicons** (CSW24, NWL23, …) are word lists. Every word has a definition and
  a playability value. Each lexicon refers to one letter distribution.
- **Leave values** are sets of leave → value pairs. Each set belongs to one
  lexicon and uses that lexicon's letter distribution. A lexicon has at most one
  set of leave values, and may have none.

Each cascade also carries a few **quiz options**: how long a study run is
(**segment size**), what happens to a quiz that isn't cleared (**progression**),
and whether typed answers have to be given in alphabetical order. Every quiz the
cascade creates starts with a copy of them. See [Quiz options](#quiz-options).

Any cascade or quiz can be downloaded as a word list of everything in it, or of
just what was answered correctly or missed. See
[Exporting words](#exporting-words).

A cascade's Source quiz can hold up to **300,000 questions**.

Once a cascade has been downloaded, studying works **offline**: a user can log
in and start a cascade, board a plane, keep studying, and have everything sync
when they land. See [Offline and Sync](#offline-and-sync).

---

## Glossary

| Term | Meaning |
|---|---|
| **Cascade** | Everything built from one set of filters: the Source quiz and every level quiz that comes from it. |
| **Source quiz** | The quiz created from the filter search. It is always the active quiz at Level 1. A finish never clears, replaces or removes it; only trashing the cascade does. |
| **Level** | A position in the cascade, numbered from 1. Each level has at most one active quiz. The **deepest level** is the only one that can be played. |
| **Attempt** | One pass through a quiz's questions, identified by its number and the shuffle seed that produced its order. A quiz that is reset or restored starts a new attempt with a new shuffle. |
| **Segment** (or **run**) | A fixed-size run of questions inside an attempt, the length of one sitting. Finishing a run drills its misses before the attempt goes on. Off by default. |
| **Finish** | Move on from the last question of an attempt. Finishing always either sends the quiz to the Trash, sends the user down a level, or resets the quiz in place. |
| **Clear threshold** | The score (X%) needed to clear a quiz. It is set per cascade when the cascade is created. |
| **Quiz options** | Segment size, progression and alphabetical order. They are held by the cascade, copied to each new quiz, and editable on either. |
| **Progression** | What finishing a quiz below the clear threshold does: **Ladder** (keep the quiz and go down a level) or **Drill** (replace the quiz with its misses). |
| **Clear** | Finish an attempt with a score of at least the clear threshold. The quiz goes to the Trash. The Source quiz is never cleared; see **Complete**. |
| **Complete** | A cascade whose Source quiz has been finished with no misses. It is a milestone, not an end state: the Source quiz resets like any other finish, nothing is created because nothing was missed, and the cascade stays active and playable until the user trashes it. |
| **Trash** | Cleared quizzes and trashed cascades. They can be restored until they are **purged** (deleted permanently) after the trash retention period (Y days, a server setting that defaults to 30). |

---

## Cascade Rules

A cascade is a **stack of levels**, and the user always plays the **deepest
level**. Level 1 is always the **Source quiz**, which has its own rules (see
[The Source quiz](#the-source-quiz)); the table below is for quizzes at Level 2
and deeper. The rules in this first table are the defaults: **Ladder**
progression and no segments. [Progression](#progression-ladder-or-drill) and
[Segments](#segments) below give the two ways the cascade's
[quiz options](#quiz-options) change them.

When the user finishes an attempt at the deepest level N ≥ 2, with score
= correct ÷ questions:

| Result | What happens | Where the user goes next |
|---|---|---|
| **Score ≥ threshold, some misses** | Level N's quiz is **cleared** (to the Trash). A new quiz of the missed questions takes its place at Level N. | The new Level N quiz |
| **Score ≥ threshold, no misses** | Level N's quiz is **cleared** (to the Trash). Level N is removed. | Back up to Level N − 1's quiz. |
| **Score < threshold, some correct** | Level N's quiz stays, **reset** to a new attempt with a new shuffle. A new quiz of the missed questions is created at **Level N + 1**. | Down to Level N + 1 |
| **Score < threshold, nothing correct** | Level N's quiz is **reset** to a new attempt with a new shuffle. No new level is created, because it would be an identical copy of Level N. | The reset Level N quiz |

Why these rules hold together:

- **The quiz being played is always the deepest level.** Clearing a quiz with
  misses keeps the same depth, clearing one with no misses pops a level, and
  failing pushes a level. So when the user finishes the quiz at Level N, no
  Level N + 1 exists yet. Missed questions never have to be merged into an
  existing quiz.
- **Upper levels wait.** A quiz above the deepest level keeps whatever state it
  had when the level below it appeared, and nothing touches it until the user
  climbs back to it. That is a rule of play, not of sync: the server accepts a
  `grade` or `move_cursor` on any active quiz, deepest or not, so a device that
  finished a level before learning that another device had restored a quiz on
  top of it keeps every grade it made and loses only its `finish` (see
  [Conflicts](#conflicts)). A quiz that a finish pushed down from is in its reset,
  reshuffled state and starts from its first question. A quiz that is waiting
  because a [segment](#segments) went down, or because a quiz was
  [restored](#trash-restore-and-purge) on top of it, keeps its attempt, its
  grades and its cursor, and resumes at the card it was on.
- **The same question can be on several levels.** A question missed at Level 2
  is still in Level 2's reset quiz and also in the new Level 3 quiz. That is
  intended: the user has to get it right in both places.
- **Score is compared exactly**, as `correct × 100 ≥ threshold × question_count`
  in integer arithmetic, so there is no rounding at the boundary.
- **A threshold of 100** means only a perfect attempt clears. The threshold is
  **1–100**; 0 is not allowed, because every attempt would clear. With a
  threshold of at least 1 an attempt with nothing correct is always below it,
  so the rows above never overlap.

The "nothing correct" rule is the one departure from a literal reading of "go
down with the misses". Without it, a Level N quiz where every question was missed
would be followed by an identical Level N + 1 quiz, doubling the work for no
benefit. It can be removed without changing anything else.

### The Source quiz

The Source quiz is permanent. It is always the active quiz at Level 1, a finish
never clears, replaces or removes it, and it leaves the cascade only when the
user moves the cascade to the Trash. Every other quiz is at Level 2 or deeper.
When the user finishes an attempt of the Source quiz:

| Result | What happens | Where the user goes next |
|---|---|---|
| **Some correct, some missed** (any score) | The Source quiz is **reset** to a new attempt with a new shuffle. A new quiz of the missed questions is created at **Level 2**. | Down to Level 2 |
| **No misses** | The Source quiz is **reset**, exactly as in the other rows; nothing is created because nothing was missed. The cascade is **complete**: its `completed_at` is recorded and the completion screen shows how many levels and attempts it took since the previous completion (or since creation). | The completion screen, then the reset Source quiz if the user keeps studying |
| **Nothing correct** | The Source quiz is **reset**. Nothing is created. | The reset Source quiz |

- The threshold still decides whether the attempt was a pass, and that is
  derivable from the attempt's counts and the cascade's threshold, which never
  changes, so no column records it; it does not decide where the user goes:
  the Source quiz's misses always go down, because there is nothing for them
  to replace.
- Progression does not apply to the Source quiz, for the same reason. The
  Level 2 quiz it creates takes the cascade's progression like any other quiz,
  so a Drill cascade is the Source quiz plus a drill chain at Level 2.
- A cascade can be completed more than once; each finish with no misses
  records a new `completed_at`. The Cascades page marks complete cascades, and
  the completion screen offers **Keep studying**, **Start over** and **Move to
  Trash**.
- **What the completion screen counts.** *Levels* is the deepest level the
  cascade reached since the previous completion (`cascades.peak_depth`, reset
  to 1 at each completion), and *attempts* is the number of attempts finished
  since then, on any level, including the attempt that completed it
  (`cascades.attempts_since_completion`; the screen takes both numbers, then
  both reset). Both are stored on the cascade and both sides maintain them with
  the same rule, so a device that has pulled everything shows the figures the
  server records. A device holding finishes another device made and it has not
  pulled shows its **own**, lower count: the `finish` result carries neither
  number, since the server's `Completed` has already reset them, so nothing
  corrects the screen afterwards. The screen is a milestone, not an audited
  total, and that is the reason it is drawn only from a completion this device
  computed (see [Conflicts](#conflicts)).
- **Segments do not make completion easier.** A question missed in a run and
  drilled one level down is still missed in the Source quiz's attempt (see
  [Segments](#segments)), so a completion needs an attempt in which no run
  descended.
- Segments work on the Source quiz exactly as on any other quiz: a run's
  misses go to Level 2 as a drill quiz.
- Restoring a quiz from the Trash always pushes it to Level 2 or deeper, since
  Level 1 is taken.

### Quiz options

Three options change how a quiz is studied:

| Option | Default | Effect |
|---|---|---|
| **Segment size** | `0` (off) | Study the quiz in runs of this many questions, drilling each run's misses before going on. See [Segments](#segments). |
| **Progression** | **Ladder** | What finishing a quiz below the clear threshold does. See [Progression](#progression-ladder-or-drill). |
| **Answers in alphabetical order** | off | In typed anagram mode, answers have to be entered in tile order. See [Typed mode](#typed-mode-anagram-quizzes-only). |

The options belong to the **cascade** and to each **quiz**:

- **The cascade holds the options a new quiz starts with.** They are set when the
  cascade is created, prefilled from the user's
  [preferences](#preferences), and can be changed at any time afterwards from the
  Cascades page or the player. Changing them affects quizzes created from then
  on, never quizzes that already exist.
- **Every quiz is created with a copy of its cascade's options**, and that copy
  can be changed at any time after the quiz exists, from the quiz settings menu
  in the player or from the level in the cascade's ladder panel. A change takes
  effect on the current card.
- **A quiz created from another quiz's misses** — a replacement, a descent or a
  segment — copies the **cascade's** options, not the finishing quiz's, so a
  change made to one quiz never propagates down the cascade. The one exception is
  a quiz in a **segment chain** (see [Segments](#segments)): a run's drill quiz
  and every replacement of it carry `segment_chain`, their progression is
  **Drill for their whole life**, they are never segmented themselves, a
  `set_quiz_options` that names `progression` or `segment_size` for one is
  rejected, and a [restore](#trash-restore-and-purge) copies only alphabetical
  order onto it.
- The **clear threshold** is not one of these options: it stays a single
  cascade-wide setting, because a cascade whose levels cleared at different
  scores would be hard to reason about.

Every option is validated on both sides: segment size is 0, meaning off, or an
integer from **5** to
`MAX_QUIZ_QUESTIONS` (the device uses the value `/api/auth/me` reported into
its `meta` store, and 300,000 before its first login), progression is Ladder
or Drill, and alphabetical order is a flag. The cap bounds **new** values only:
it can be lowered on the server, and a cascade, quiz or preference that already
holds a larger segment size keeps it and keeps working, since nothing
re-validates options after they are stored. So the builder, the options dialog
and the quiz settings menu **clamp what they prefill** to the cap in `meta`,
rather than sending a value the server would refuse as a field the user never
typed. The minimum of 5 rules out an
attempt made of one-question runs. It does not rule out a single short run after
a [mid-attempt size change](#segments), since boundaries follow the current size
while `run_start` keeps the one an earlier size set: a quiz at `run_start` 119,
reached with S = 7, whose size becomes 120 has a run of one question, which costs
one card and one Trash entry and breaks nothing. Nor does the minimum bound the
work on its own. **An
attempt creates at most `ceil(question_count / S)` chain quizzes**, one per run
that missed something, each of which becomes a Trash entry, so the segment size
is the user's choice of how much that will be: 5 on a 300,000-question quiz can
leave 60,000 of them. The builder and the quiz settings menu therefore warn
when `question_count / segment_size` is above 2,000, saying how many levels the
attempt could create; the count is the quiz's own in the quiz settings menu,
and the cascade's in the builder and the cascade's options dialog. A segment size at or above a quiz's question count means the same as 0 for
that quiz.

### Progression: Ladder or Drill

Progression decides what a finish below the clear threshold does, for quizzes
at Level 2 and deeper (the [Source quiz](#the-source-quiz) is exempt). A finish
reads the finishing **quiz's** progression, which starts as a copy of the
cascade's and can be changed on the quiz alone; the cascade's setting only
decides what new quizzes start with:

| Progression | Finishing below the clear threshold |
|---|---|
| **Ladder** (default) | The table above: Level N's quiz stays, reset and reshuffled, and its misses become a new quiz at Level N + 1. The user has to come back and clear Level N before the cascade is done. |
| **Drill** | Level N's quiz is finished and goes to the Trash whatever the score, and a new quiz of its missed questions takes its place at Level N. The cascade never grows a level from a finish. |

So with Drill, the score decides nothing about where the user goes; only the
misses do:

| Result | What happens | Where the user goes next |
|---|---|---|
| **Some correct, some missed** (any score) | Level N's quiz is finished (to the Trash). A new quiz of the missed questions takes its place at Level N. | The new Level N quiz |
| **No misses** | Level N's quiz is finished (to the Trash). Level N is removed. | Back up to Level N − 1's quiz. |
| **Nothing correct** | Level N's quiz is **reset** to a new attempt with a new shuffle. Nothing is created, because the replacement would be an identical copy. | The reset Level N quiz |

- The attempt's outcome still records whether the score met the clear
  threshold, `cleared` when it did and `replaced` when it did not, so
  the Trash and the cascade's history still show which attempts were passes.
- The "nothing correct" rule is kept for the same reason as in Ladder: replacing
  a quiz with a copy of itself is work for no benefit.
- A Drill cascade that has never had a segment descent or a restore is at most
  two levels deep for its whole life: the Source quiz, and the drill chain its
  misses feed at Level 2. That is what makes it a straight drill: keep going
  until nothing is missed.

**Naming.** *Progression: Ladder / Drill* is the name used throughout this plan
for the option described as "go back up the ladder". Other candidates, if a
better one is wanted later: **Repeat style** (*Climb back* / *Straight through*),
**Keep failed quizzes** (a plain boolean), **Second chances**, **Review mode**
(*Ladder* / *Chain*), or **On a fail** (*Go down a level* / *Retry the misses*).
Renaming it touches the enum `quiz_progression`, two columns, the two option
fields on the wire and the UI strings, and nothing else.

### Segments

A **segment size** of 0 means no segments: an attempt is one run from the first
question to the last. A segment size S greater than 0 splits the attempt into
runs of S questions in the attempt's shuffled order — positions 0…S−1, S…2S−1,
and so on — so the user studies a sitting's worth at a time and drills what they
missed in it before going on.

Run boundaries are the positions S, 2S, … that fall **strictly inside** the
quiz. The last run always ends at the last question, where finishing the attempt
applies the rules above. So a quiz of 250 questions with S = 100 has runs of 100,
100 and 50, and a quiz of 80 questions with S = 100 behaves exactly as if
segments were off.

Moving on from the last question of a run that is not the last run:

| Result | What happens | Where the user goes next |
|---|---|---|
| **Some of the run missed** | A new quiz of that run's missed questions is created at **Level N + 1**, marked `segment_chain`, with Drill progression and no segments. Level N's quiz keeps its attempt, its grades and its counters, with its cursor at the start of the next run. | Down to the new Level N + 1 |
| **None of the run missed** | Nothing is created, and nothing is finished. | The first question of the next run |

- **A segment chain always drills and is never segmented.** However the
  cascade's options are set, the quiz made from a run's misses is marked
  `segment_chain`, and so is every quiz that replaces it: each has Drill
  progression and a segment size of 0, so finishing one always replaces it with
  its own misses, in a single run, until an attempt has no misses at all. Then
  its level is removed and the user comes back up to the parent quiz's next run.
  A run is a sitting to be finished, not a level to come back to, and a chain
  sits one level below the run it serves until it is cleared, however many
  misses the run had. A chain quiz restored from the Trash simply returns to
  whatever level is above it when it clears.
- **Coming back up** puts the user on the parent quiz's cursor, which is the
  first question of the next run, in the same attempt and the same shuffled
  order.
- **A run starts where the last one ended.** Each quiz records `run_start`, the
  position of the last boundary it passed in the current attempt, whether or not
  that run descended. The current run is the positions from `run_start` up to,
  but not including, the next boundary, and passing a boundary sets both
  `run_start` and the cursor to it. `run_start` is 0 in a new attempt.
- **Previous never crosses a run boundary.** Going back stops at `run_start`,
  so on the device a run that has been passed is never re-entered, regraded or
  descended again in the same attempt. Like **Upper levels wait** above, that
  is a rule of play and not of sync: the server accepts a `grade` on any
  question of an active quiz whatever that quiz's `run_start`, so a device that
  had not yet learned of a boundary another device passed keeps every grade it
  made (see [Conflicts](#conflicts)). What no device can do is descend a run
  twice, because `origin_segment_end` is unique per parent attempt, so **a run
  descends at most once per attempt** whatever arrives afterwards.
- **The finish still sees every miss.** A question missed in run 1 and then
  drilled at Level N + 1 is still missed in Level N's attempt: it counts against
  the attempt's score, and it is in the quiz that the finish creates, whether
  that is a descent (Ladder) or a replacement (Drill). Segments change **when**
  misses are drilled, not what the attempt was.
- **Resetting** a quiz starts its runs again from position 0 in the new attempt.
- **Changing the segment size mid-attempt** is allowed. Boundaries are always
  computed from the current size, so the next boundary is the smallest multiple
  of the current S that is greater than the cursor and less than the question
  count, and the run that ends there still begins at `run_start`. So a quiz of
  250 with S = 100 whose first run descended at 100, then switched to S = 30,
  has its next run at positions 100–119, not 90–119. Runs that have already
  been passed stay passed, because `run_start` and a descent's
  `origin_segment_end` are **positions**, not run numbers. **`run_start`
  survives every size change**, a change to 0 and one to a value at or above the
  question count included, which mean the same thing: a passed run stays passed
  whether or not the quiz is segmented any more, so `set_quiz_options` never
  rewrites attempt state and the two rule modules never disagree about where
  Previous stops. Because no run indicator is shown for an unsegmented quiz, the
  player says "you have already finished this part of the attempt" when Previous
  stops at `run_start`, so a blocked Previous always has a visible cause.
- **How a run is numbered.** Everything the rail, the top bar and the banners
  show is computed from the **current** segment size: the run number is the
  count of boundaries at or below `run_start` plus one, the total is the count
  of boundaries strictly inside the quiz plus one, and the position within the
  run is `cursor − run_start + 1` of `next_boundary − run_start`, or of
  `question_count − run_start` on the last run, where `next_boundary` is None.
  A size change
  mid-attempt therefore renumbers them, which is expected: the 250-question
  quiz above, at `run_start` 100 after switching to S = 30, reads
  `run 4 of 9 · 1 of 20`.
- **Segments never clear a quiz**, never change its attempt number and never
  touch the clear threshold. Only a finish does those.

### Trash, restore and purge

- **Finished quizzes** at Level 2 and deeper go to the Trash automatically,
  labelled with their cascade, level and final score, and with whether the
  score cleared the quiz or it was replaced under
  [Drill progression](#progression-ladder-or-drill). The Source quiz never
  does.
- **A complete cascade** (Source quiz finished with no misses) stays active.
  The user sees a completion screen showing how many levels and attempts it
  took, with **Keep studying**, **Start over** and **Move to Trash**. A
  cascade reaches the Trash only when the user moves it there.
- **Trashing a cascade manually** (e.g. "I'm done with this word list") moves the
  whole cascade to the Trash with its levels as they are.
- **Restoring a finished quiz** pushes it back onto its cascade as the **new
  deepest level**, so it is the next thing the user studies. Its attempt number
  goes up by one, its cursor, counters and `run_start` return to 0, its
  questions are reshuffled with the operation's seed, and it gets a fresh copy
  of the cascade's current [quiz options](#quiz-options) (a segment-chain quiz
  takes only alphabetical order and stays Drill with no segments). The quiz that
  was the deepest level is not
  touched: if that quiz was mid-attempt, it keeps its attempt, grades and cursor
  and resumes there when the user climbs back. If the cascade had been trashed,
  it comes back too. Level 1 is always the Source quiz, so a restored quiz is at
  Level 2 or deeper. Restoring never merges quizzes and never leaves a gap in the
  levels.
- **Restoring a manually trashed cascade** brings it back exactly as it was,
  except that the retention clocks of its cleared quizzes restart (see
  **Purging** below).
- **Purging** happens after the trash retention period: a cleared quiz is purged
  that long after it was cleared, and a trashed cascade that long after it was
  trashed. Trashing a cascade freezes the clocks of the cleared quizzes under
  it: they are purged with the cascade, never before it, so a trashed cascade is
  one unit in the Trash until it is gone. Restoring the cascade, by **Restore**
  or by restoring one of its quizzes, sets `cleared_at` to now on the cleared
  quizzes still under it, so each gets a full retention period again instead of
  being purged at once for time that passed while the cascade was trashed.
  Purging a cascade purges every quiz in it. Users can also purge from the
  Trash immediately with **Delete forever**.
- **Start over** on a complete cascade (or any cascade, from the row menu on
  `/cascades`) creates a new cascade with the same name, a private copy of the
  same filters, and the same threshold and [quiz options](#quiz-options).
- **Exporting** works on anything in the Trash that has not been purged, so a
  cleared quiz can still be downloaded as a word list (see
  [Exporting words](#exporting-words)).

### Cascade limit

A user can have at most **100 cascades** (`MAX_CASCADES_PER_USER`). Cascades in
the Trash count toward the limit, because they still take storage and can be
restored. Only purging frees a slot, either automatically after the retention
period or with **Delete forever**.

- **Visibility.** The Cascades page shows `87 of 100 cascades`, and the cascade
  builder shows a warning from 90.
- **At the limit.** Create Cascade and Start over are disabled, with a link to
  the Trash, and the server refuses them with `409`.
- **Restores.** Restoring a quiz or cascade never creates a new cascade, so it is
  always allowed.
- **Races.** The limit is checked inside the creation transaction after locking
  the user's row, so two simultaneous creations can't both take the last slot.
  **The search runs before that transaction opens**, so the row lock never
  covers CPU-bound work or a wait for a
  [`SEARCH_CONCURRENCY`](#search-engine) permit; a cheap pre-check refuses an
  over-limit request before searching at all, and a creation that loses the
  race after its search is refused with the same `409`.

---

## User Experience

### Accounts

Every page except the landing, login, registration, email confirmation and
password reset pages requires a logged-in user. An account is a username, an
email address and a password. Some accounts are **admins**, who can also upload
and delete catalog data (see [Admin](#admin)). See
[Authentication](#authentication).

### Preferences

Each user has a set of display and answering preferences. They are edited on
`/account` and also from a settings menu (gear icon) in the quiz player, where a
change takes effect on the current card straight away. That same menu also holds
the current quiz's [options](#quiz-options), which belong to the quiz rather than
the user. Preferences belong to the user, not to a cascade, and they sync like
everything else (see [Offline and Sync](#offline-and-sync)). The three
`Default …` preferences below only prefill the cascade builder; changing one
never changes a cascade that already exists.

| Preference | Applies to | Default | Options |
|---|---|---|---|
| **Default clear threshold** | New cascades | `80`% | 1–100 |
| **Default segment size** | New cascades | `0` (off) | 0 (off), or 5 – `MAX_QUIZ_QUESTIONS` |
| **Default progression** | New cascades | **Ladder** | Ladder / Drill |
| **Default alphabetical order** | New cascades | off | on / off |
| **Leave value decimal places** | Leave Value answers | `1` | `0`, `1`, `2`, `3` |
| **Show definitions with anagrams** | Anagram answers | off | on / off |
| **Show hooks with anagrams** | Anagram answers | off | on / off |
| **Anagram answer mode** | Anagram quizzes | **Flashcard** | Flashcard / Typed |
| **Controls** | Desktop quiz area | Show / Next: left click or Space · Toggle grade: right click or `X` · Previous: middle click or Backspace | Up to three bindings per action: any mouse button, wheel direction or key, with modifiers. See [Controls](#controls). |

Question order is **not** a preference: questions are always shuffled.

### Creating a cascade

`/cascades/new` is laid out like Zyzzyva's Search tab. It needs a connection,
because searching runs on the server.

1. **Quiz type**: Anagram, Definition or Leave Value.
2. **Lexicon**: e.g. `CSW24`. When the type is Leave Value, lexicons without
   leave values are disabled, with a tooltip saying why.
3. **Filters**: a tree of condition rows in AND / OR [groups](#groups). Each
   row has a `+` menu (add a row or a group below), a `−` button (remove this
   row), a **Not** checkbox, a filter type dropdown, and inputs that change with
   the type. The dropdown only lists filters that apply to the chosen quiz type
   (see the [applicability table](#filter-applicability-by-quiz-type)). The Not
   checkbox is disabled for filters that do not support negation. The top-level
   group is AND, so a plain list of rows means all of them must match, as in
   Zyzzyva. For distributions whose tiles are not all plain A–Z, a **tile
   palette** under each tile input inserts tiles by clicking.
4. **Load Search… / Save Search…**: save the current filter rows under a name,
   or load a saved set. Load lists the saved searches by name, with how many
   word-list entries each holds, and fetches only the tree of the one picked
   (see [the endpoints](#catalog-and-search)), since a user can keep many lists
   of up to 300,000 entries. That fetch shares the search bucket, so a `429`
   on it waits for `Retry-After` like the preview. Save Search… is refused while any row is invalid or
   flagged, naming the row, and the server validates the tree again (see
   [the endpoint](#catalog-and-search)). A saved search stores only the filters, never the
   results, so it can be reused with any lexicon and any quiz type. It does
   record the quiz type it was saved under, because an In Word List row's entries
   are canonical for that type and the load flags the row when the two sides of
   Leave Value differ (see
   [Filter applicability](#filter-applicability-by-quiz-type)).
5. **Clear threshold**: prefilled from the user's default.
6. **Quiz options**: segment size, progression and alphabetical order, each
   prefilled from the user's defaults. They are explained inline ("a segment of
   40 means you study 40 at a time and drill what you missed before going on")
   and can be changed later on the cascade and on any individual quiz. See
   [Quiz options](#quiz-options). The alphabetical-order option is shown for
   Anagram cascades only, since nothing else has typed answers, but it is stored
   whatever the type.
7. **Cascade name**: prefilled with a summary of the filters, such as
   `CSW24 · Length 7–7 · Probability Order 1–1000`, which the user can edit.
   The summary is built on the device only, from the same `lib/filters.ts`
   table that labels each row, and the device always sends the field's text,
   so the server never generates a name. When the summary is longer than 200
   Unicode scalar values it is cut to its first 199 followed by `…`.
8. **Preview**: runs the search as filters change, debounced by
   `PREVIEW_DEBOUNCE_MS` (400 ms) and then throttled to at most one request
   every `PREVIEW_MIN_INTERVAL_MS` (2 s), both beside the other client
   constants, with the latest edit always sent when the interval expires so the
   count on screen is never left behind the filters. The debounce is what makes
   the first count after a pause feel immediate; **the interval is what keeps a
   user editing a pattern under `SEARCH_RATE_PER_MINUTE`**, which the debounce
   alone does not: someone who types two or three characters and pauses to think
   crosses 400 ms on every pause, which would be 120 requests a minute against a
   limit of 30, making the `429` path below the ordinary case rather than the
   exception. At one request every 2 s the ceiling is exactly the limit — which
   is why the preview also **keeps a reserve**: it stops firing once it has spent
   all but two of `SEARCH_RATE_PER_MINUTE` in the rolling minute, showing the
   last count with "the server is busy" as a `429` would, because preview,
   Create Cascade, Start over and Save Search… all draw on that one bucket and a
   preview sized to the whole of it would refuse the very action the page leads
   to. The reserve counts **this tab's own** requests, since the bucket is the
   user's and nothing tells one tab what another spent, so two builders open at
   once can still empty it between them; that is what the `429` retry below is
   for, and the reserve is what keeps the ordinary single-tab case from needing
   it. It shows
   how many
   questions it matches plus the first few, so the user can adjust before
   committing. While an In Word List row holds more than 10,000 entries the
   preview runs only on an explicit **Preview** button, so a list of up to
   300,000 entries is not resent on every keystroke. The form shows inline errors for invalid rows, such as a
   malformed pattern, a tile not in the distribution, or min > max. Searches
   with more than 300,000 results say so and show the count. A `503` from the
   [search semaphore](#search-engine) is not a row error: the last count stays
   on screen with "the server is busy" beside it, and the preview retries once
   after `Retry-After`, so a loaded server never looks like a bad filter. **A
   `429` takes the same path**, since the user cannot act on the difference
   between a busy instance and a spent bucket and neither is a fault of their
   filters; only a `400` ever marks a row.
   Create Cascade retries a `5xx` with the same body, as it already does, **and
   a `429` the same way**, after `Retry-After`, with the button reading "the
   server is busy" until it goes through: the device-minted ids make the repeat
   safe, and without it a minute of previewing could refuse the create that
   follows it. **Start over** and **Save Search…** do the same.
9. **Create Cascade**: runs the search, shuffles the questions into the Source
   quiz, saves the cascade, starts downloading it for offline use, and goes
   straight to the first card. At the [cascade limit](#cascade-limit) the button
   is disabled, with an explanation.

### Cascades page

`/cascades` lists the user's cascades, newest activity first. Each shows:

- name, quiz type, lexicon and clear threshold
- its [quiz options](#quiz-options) in short form, e.g. `segments of 40 · drill`,
  or nothing when they are all at their defaults
- a compact ladder of its levels, e.g. `L1 · 250 (waiting, run 3 of 7) →
  L2 · 38 (waiting) → L3 · 9 (4/9 done)`
- a **Complete** badge with the date, once the Source quiz has been finished
  with no misses
- an **Available offline** badge, download progress, or "not downloaded on
  this device, open to download" for a cascade whose question rows this
  device does not hold (outside its window or dropped for the storage
  [budget](#on-the-device), not kept offline, nothing pending)
- a row menu with **Quiz options**, **Export…** (see
  [Exporting words](#exporting-words)), **Start over**, **Keep offline** and
  **Move to Trash**

Selecting a cascade opens the player at its deepest level. The header shows how
much of the [cascade limit](#cascade-limit) is used (`87 of 100 cascades`) and
the sync status, which is [`SyncStatus`](#frontend) in every one of its states —
**Synced**, **3 changes waiting to sync**, **Offline**, **Log in to sync**,
**not enough room on this device**, **signed out in another tab** and **reload to
keep syncing** — since this is the page a user studies from and the last four
are the ones that need acting on.

### Trash page

`/trash` lists finished quizzes and trashed cascades, grouped by cascade. Each
entry shows its final score, whether it cleared or was replaced, and when it will
be purged (`cleared_at` or `trashed_at` plus the retention period from the
`meta` store). A quiz this device cleared offline has no server `cleared_at`
yet, and a cascade it trashed offline has no `trashed_at`, which is what puts a
cascade in this list at all, so `applyLocally` writes the operation's `at` into
the overlay row as a provisional one in both cases, which satisfies the row's
own shape and keeps a cascade trashed on a plane from vanishing from the
Cascades page and the Trash alike. Such an entry reads
"purges 30 days after this syncs" until the acknowledged row replaces it.
Each entry has **Restore**, **Export…** and **Delete forever** actions.
**Entries are grouped and not all rendered.** A cascade's group shows its name,
how many entries it holds and the earliest purge date among them, collapsed;
expanding it renders a virtualized list in pages of 100 behind a **Show more**
control. A group holding one entry is rendered expanded, since collapsing a
single row hides nothing. One segmented attempt can leave tens of thousands of cleared quizzes
(see [Quiz options](#quiz-options)), and a flat list of them would lock the tab.
Restore is offered offline for a cascade that is not on this device too, with
the note "downloads when online", since the restored level cannot be played
until its rows are fetched. A
trashed cascade is one unit: the cleared quizzes listed under it can still be
restored (which brings the cascade back) and exported, but Delete forever is
shown only on the cascade itself, never on its quizzes.

### Taking a quiz

`/cascades/:id` plays the deepest level's quiz one card at a time. Upper levels
are shown as waiting and cannot be opened. The player chooses its layout from the
device's **primary pointer**:

- **Desktop layout** for a mouse or trackpad (`pointer: fine`) in a window at
  least 900 px wide.
- **Touch layout** otherwise (see [Touch zones](#touch-zones)).
- A tablet with a keyboard and mouse gets the layout for its primary pointer.
  Key bindings work in both layouts.

#### Desktop layout

```
┌────────────┬────────────────────────────────────────────┬────────────────┐
│ Wordfall   │                                            │ Level 2 of 3   │
│            │                                            │ attempt 3      │
│ Cascades   │                                            │ 37 / 250       │
│ New        │                  AEINRST                   │ 31 ✓   5 ✗     │
│ Trash      │                                            │ clear at 80%   │
│ Account    │     ANESTRI  ANTSIER  NASTIER  RATINES …   │                │
│            │                                            │ Ladder         │
│            │                 ✓ Correct                  │  L1 250 waiting│
│            │                                            │  L2  38 ◀ now  │
│ ● Synced   │                 quiz area                  │ ⚙ Preferences  │
└────────────┴────────────────────────────────────────────┴────────────────┘
```

- **The quiz area** is the large central panel. It takes the full height and at
  least 60% of the window width. It shows the question, the answer and the
  grade, and it is the only place where mouse controls act.
- **The side rails** hold everything else, so clicking them never counts as a
  quiz action:
  - left: navigation and sync status
  - right: level, attempt, progress, clear threshold, the run indicator when the
    quiz has a [segment size](#segments) (`run 2 of 3 · 37 of 100`, numbered
    as [Segments](#segments) defines), the
    cascade's ladder, and the settings menu (user preferences and this quiz's
    [options](#quiz-options))

  Both rails collapse to icons in narrower windows.
- **Inside the quiz area**, text selection is off and the browser's context menu
  is suppressed. Middle-click autoscroll and (on Linux) middle-click paste are
  prevented, so all three mouse buttons are free for quiz actions.

#### The three actions

Every quiz is driven by three actions, whatever the device:

| Action | Before the answer is shown | After the answer is shown |
|---|---|---|
| **Show / Next** | Show the answer | Save the grade and go to the next card |
| **Toggle grade** | Mark the card missed in advance ("I don't know this one") | Flip the grade between Correct and Missed |
| **Previous** | Go back to the previous card | Go back to the previous card |

- **The grade** appears with the answer, as a large **✓ Correct** or **✗ Missed**.
  It starts as Correct unless the card was toggled before the reveal, so a user
  who knows the answer only ever uses Show / Next.
- **Saving.** A grade is saved only when Show / Next moves on. Going back from a
  card discards that card's unsaved reveal and toggle.
- **Going back.** Previous shows the earlier card with its answer and saved
  grade. Toggle changes that grade, and Show / Next saves it and moves forward
  again. Previous does nothing on the first card of the attempt or of the
  current [run](#segments).
- **The last card.** Moving on from it finishes the attempt (see
  [Finishing a quiz](#finishing-a-quiz)).
- **Repeats.** The same action fired twice within 120 ms counts once, so a
  bouncing mouse button or an accidental double tap never skips a card.

#### Answers

This is how Definition and Leave Value quizzes always work, and how Anagram
quizzes work by default (**flashcard mode**). The answer that Show / Next reveals:

- **Anagram**: each valid anagram, one per line, in alphabetical order. The
  card carries the words already in that order, and the device never re-sorts
  them.
  - With *show hooks* on, each word is written Zyzzyva-style with its front
    hooks to the left and back hooks to the right, e.g. `bcfm AA hls`. The
    hooks are drawn by `TileText` like any other tiles, in
    [tile order](#tiles), and **lower-cased only where a tile is a single ASCII
    letter**: that is what makes the English display the one Zyzzyva users know,
    while a multi-character or non-ASCII tile keeps its own form, since its
    lower-case form is its `blank_letter` — `ny`, `l·l` — which means "a blank
    playing that tile" everywhere else in Wordfall and is an error in an upload
    file. So a Catalan front hook reads `NY`, never `ny`.
  - With *show definitions* on, each word's definition appears in smaller text
    beneath it.
- **Definition**: the full definition text.
- **Leave Value**: the value with its sign, rounded to the user's decimal places
  (half away from zero), e.g. `+34.1` or `-8.4`. A value that rounds to zero is
  shown as `0.0`, never `-0.0`. Both sides round the same way, on the `f64`
  value: with `d` decimal places, `n = trunc(|v| × 10^d + 0.5)` computed in
  `f64`; the text is `n` written as an integer, with a `.` inserted `d` digits
  from the right (zero-padded, so `d = 1` and `n = 3` give `0.3`); the sign is
  an ASCII `-` when `v < 0` and `n > 0`, a `+` on screen when `v > 0` and
  `n > 0`, no sign when `n = 0`, and never a `+` in exports. Neither side uses `toFixed`, `format!("{:.d}")` or any other
  library rounding, which round half to even on the binary value.

#### Typed mode (Anagram quizzes only)

- The question shows `0 of 9 found` with a focused text input.
- The user types a word and presses **Enter**. Input is upper-cased, trimmed,
  and converted to tiles (see [Tiles](#tiles)).
  - A word that is **one of the answers and not yet entered** joins the found
    list, shown in alphabetical order, and the counter goes up.
  - A word **already entered** is ignored, with a brief "already entered" note.
  - An entry whose **tile count differs from the question's** is not counted:
    it cannot be an answer, and it is almost always two words typed together
    (`RETAINS NASTIER`, which the space-separates-tiles rule reads as one
    14-tile entry). It is left in the input with "type one word at a time",
    and neither joins the wrong list nor affects the grade.
  - Any other word is **wrong**. It is listed in red under the input, and the
    question will be graded missed.
- The answer is shown when **every anagram has been found**, or on **Show /
  Next**, which here means giving up. **Enter on an empty input** is always Show /
  Next as well. Showing the answer displays the full list, formatted by the hook
  and definition preferences, with the words the user did not find highlighted.
- The grade is set automatically: **correct** only if every anagram was found
  with no wrong entries, otherwise **missed**. Toggle grade flips it, and Show /
  Next (or Enter) saves it and moves on.
- **A card whose answer the device does not hold falls back to flashcard mode**,
  with the "answer needs a connection" line the reveal shows there. Typed mode
  grades from the answer list, so without one every entry would be wrong and
  the card would grade missed by itself; the fallback starts the grade at
  Correct and leaves it to Toggle, as flashcard mode does, and the
  alphabetical-order option is inert on such a card. This is the same fallback
  a missing distribution forces (see [On the device](#on-the-device)).
- **Alphabetical order.** With the quiz's **Answers in alphabetical order**
  [option](#quiz-options) on, each answer has to be entered at or after the
  answers already given, in tile order (see [Tiles](#tiles)). An entry that is a
  valid, not-yet-entered answer but sorts before the furthest answer entered so
  far is still added to the found list, marked **out of order**, and counted as a
  wrong entry, so the card grades missed. Accepting it anyway means the user can
  finish finding the rest instead of being stuck, and comparing against the
  furthest answer so far — not the previous one — means one slip doesn't make
  every later answer wrong too. The input shows a hint of what is expected next
  ("after `RETSINA`"). The option does nothing in flashcard mode, and nothing at
  all in Definition and Leave Value cascades, but it is stored for every cascade
  so that switching answer modes later keeps it.
- **Protecting typing.** While the input has focus, every stroke that **types,
  edits or submits** goes to the input and is never dispatched as a binding:
  any key that produces a character, **with or without Shift**, Space,
  Backspace, Delete and **Enter**. Enter keeps its two typed-mode meanings —
  submit the word, and Show / Next on an empty input — even when it is also
  bound, so one keystroke never both submits and acts. Shift alone is not a
  modifier here, so a `Shift+T` binding types a `T` while the input has focus.
  Strokes with Ctrl, Alt or Meta still act, and so do Escape, the arrow keys
  and function keys. Every binding acts as usual in flashcard mode and whenever
  the input does not have focus.
- **Protecting against accidental give-ups.** Before the answer is shown, a
  stroke bound to **Show / Next** inside the quiz area focuses the input instead
  of acting. Strokes bound to Toggle grade and Previous, such as the default
  right and middle clicks, still act, so a card can be marked "I don't know
  this one" or left with the keyboard covered.
- Switching modes in the middle of a card resets that card's typed entries.

#### Controls

The default desktop bindings are:

| Action | Mouse (in the quiz area) | Keyboard |
|---|---|---|
| **Show / Next** | Left click | Space |
| **Toggle grade** | Right click | `X` |
| **Previous** | Middle click | Backspace |

- **What can be bound.** Each action can have up to three bindings. A binding
  can be:
  - a mouse button: left, middle, right, back or forward
  - a wheel direction: up or down
  - any key

  Each can be combined with any mix of Ctrl, Shift, Alt and Meta.
- **Where bindings act.** Mouse and wheel bindings act only inside the quiz
  area. Key bindings act anywhere on the player page except text fields outside
  the quiz area, such as the preferences menu. **A wheel binding takes the
  wheel** inside the quiz area: a bound direction acts and does not scroll, and
  an unbound direction scrolls as usual. An answer that overflows the panel — a
  long definition, an anagram list with definitions — therefore always shows a
  scrollbar, and scrolls with it or with any arrow or Page key that is not
  itself bound, so a user who
  binds the wheel can still read the end of it; the capture box says so when a
  wheel direction is bound.
- **Editing.** Bindings are changed in **Controls**, on the Account page and in
  the player's preferences menu. The user chooses **Add binding** and then
  presses the key, or clicks or scrolls, inside a capture box. Escape cancels
  capture and cannot itself be bound.
- **Rules.**
  - A stroke can belong to only one action; binding it to another action moves
    it there, with a notice.
  - Every action must keep at least one binding.
  - **Reset to defaults** restores the table above.
- **Keyboard layouts.** Keys are recorded by physical position
  (`KeyboardEvent.code`) and displayed using the user's keyboard layout where
  the browser supports it (`navigator.keyboard.getLayoutMap()`).
- **Strokes the page may not receive.** Some belong to the browser or operating
  system: Ctrl+W, Cmd+Q, and on some systems the back and forward mouse
  buttons. The capture box warns when a stroke is one of these.
- **Wheel bindings** act at most once every 150 ms, so one flick of the wheel is
  one action.
- **Sync.** Bindings sync across devices along with the other preferences.

#### Touch zones

In the touch layout, the quiz area fills the screen below a slim top bar. The
bar holds the menu button, level and progress (with the run when the quiz has a
[segment size](#segments)), and the settings button, and the menu opens a drawer
with navigation, the ladder, the quiz's [options](#quiz-options) and cascade
details. The quiz
area is divided into three tap zones, one per action.

Portrait:

```
┌─────────────────────────────┐
│ ☰    L2 · 37/250 · 80%    ⚙ │  top bar (not part of the quiz area)
├─────────────────────────────┤
│         ↶  Previous         │  15%
├─────────────────────────────┤
│                             │
│           AEINRST           │
│                             │
│   ANESTRI ANTSIER NASTIER … │  60%  Show / Next
│                             │
│          ✓ Correct          │
│                             │
├─────────────────────────────┤
│       ✓ ⇄ ✗   Toggle        │  25%
└─────────────────────────────┘
```

Landscape:

```
┌──────────┬────────────────────────────┬──────────┐
│    ↶     │          AEINRST           │  ✓ ⇄ ✗   │
│ Previous │                            │  Toggle  │
│          │   ANESTRI ANTSIER …        │          │
│   20%    │     Show / Next · 55%      │   25%    │
└──────────┴────────────────────────────┴──────────┘
```

Why the zones are arranged this way:

- **Show / Next** is used twice per card, so it gets the largest zone, in the
  middle of the screen where the thumb rests when holding a phone in one hand.
- **Toggle** is only needed on missed cards. It gets the bottom band, within
  reach without changing grip but separate from the main zone. A mistaken toggle
  is visible straight away and undone with a second tap.
- **Previous** is the rarest action and the only one that moves backwards. It gets
  the smallest zone, along the top edge where it is hardest to hit by accident.
- **In landscape** the phone is usually held in both hands, so the zones become
  columns under the thumbs: Previous on the left, Toggle on the right.

Details:

- **Labels.** Each zone shows a faint icon and label, with subtle dividers
  between zones.
- **Taps and drags.** A tap acts, and a drag scrolls. Long answer lists and
  definitions therefore scroll inside the Show / Next zone without advancing.
- **Responsiveness.** `touch-action: manipulation` removes the double-tap-zoom
  delay. Toggle gives a short vibration where the device supports it.
- **Typed mode.** The on-screen keyboard covers the zones, so while it is open a
  row of three buttons sits just above it: Previous, Show / Next, Toggle, in the
  same left-to-right order as landscape.
- **Customization.** The zones are fixed; only mouse and keyboard controls can
  be customized.

#### Saving progress

Every grade is saved locally at once and synced when there is a connection, so
closing the tab, losing the connection, or switching devices resumes at the
current card.

### Finishing a quiz

Moving on from the last card applies the [cascade rules](#cascade-rules) and
goes straight on, with no summary screen. A small, non-blocking banner says what
happened:

- `Level 2 cleared with 87%. Its 5 missed questions are now Level 2.`
- `Level 2 cleared with 100%. Back to Level 1.`
- `Level 2: 64%, and 80% is needed to clear. Level 2 is reshuffled and waiting.
  Down to Level 3 with 18 missed questions.` (Ladder progression)
- `Level 2: 64%. Replaced with its 18 missed questions.` (Drill progression)
- `Level 2: 0%. Reshuffled. Try again.`
- `Level 1: 87%. Reshuffled, and its 5 missed questions are now Level 2.` (the
  Source quiz, whatever the progression)
- `Level 1: 100%. Cascade complete after 4 levels and 9 attempts.` This one
  leads to the completion screen, which offers **Keep studying**, **Start
  over** and **Move to Trash**.

Moving on from the last card of a **run** that is not the last run does not
finish anything; it drills that run's misses (see [Segments](#segments)) and
says so:

- `Run 2 of 3 done, 9 missed. Down to Level 3 to drill them.`
- `Run 2 of 3 done, nothing missed. On to run 3.`
- `Level 3 done. Back to Level 2, run 3 of 3.`

All of this works offline.

### Exporting words

Any cascade or quiz can be downloaded as a word list. **Export…** appears in the
row menu on `/cascades`, on each level in the player's ladder panel, and on each
entry on `/trash`, and opens a small dialog:

| Choice | Options | Default |
|---|---|---|
| **What** | The whole cascade, or one of its quizzes | whatever the menu was opened from |
| **Which questions** | All · Correct · Missed · Not yet answered | All |
| **Format** | **Word list** (`.txt`, one entry per line) or **Spreadsheet** (`.csv`, one row per question) | Word list |
| **Lines** (word list) | The answers, or the questions | the answers for Anagram cascades, the questions otherwise |
| **Columns** (spreadsheet) | question, answer, definition, hooks, grade | question, answer, grade |

What "correct" and "missed" mean:

- **For a quiz**: the grades of its current attempt. For a quiz in the Trash, the
  attempt it finished on.
- **For a cascade**: the union over its **active** quizzes' current attempts. A
  question is *missed* if it is graded missed in at least one of them, *correct*
  if it is graded correct in at least one and missed in none, and *not yet
  answered* if it is in no active quiz or ungraded everywhere. This is what
  "everything I'm still getting wrong in this list" means, which is what the
  export is for. The dialog spells it out in a line of help text.

What ends up in the file, per quiz type:

| Quiz type | Questions | Answers |
|---|---|---|
| **Anagram** | one alphagram per line | every word of each selected alphagram, one per line, in alphabetical order |
| **Definition** | the word | the definition |
| **Leave Value** | the leave | the value, at the user's decimal places |

- Entries are written in the quiz's shuffled order for a quiz export, and in
  search order for a cascade export. For a quiz export the dialog offers
  **alphabetical instead**, which sorts by question key in
  [tile order](#tiles); it is not offered for a cascade export, whose search
  order already is that order (see [Search Engine](#search-engine)), so the two
  would produce the same bytes.
- [Tiles](#tiles) are written in MAGPIE notation, so a multi-character tile
  round-trips (`A[NY]S`). Files are UTF-8 without a byte-order mark. A word
  list uses LF line endings; a CSV is RFC 4180, so CRLF line endings, a header
  row, and a field quoted with `"` only when it contains `,`, `"`, CR or LF,
  with `"` doubled inside. Both end with a line ending, except that a word
  list with no entries is a zero-byte file; a CSV with no entries is its
  header row alone.
- **CSV cells.** `question` and `answer` are as in the table above; an Anagram
  `answer` cell holds the alphagram's words separated by single spaces, in
  alphabetical order. `definition` is the word's definition, or for an Anagram
  row the definitions of its words separated by ` | ` in the same order
  (definitions themselves contain ` / `, so that cannot be the separator; a
  definition may hold ` | ` too, so an Anagram row's `definition` and `hooks`
  cells are for reading, not for splitting back into words).
  `hooks` is `front hooks|back hooks`, each a space-separated list of upper-case
  tiles in MAGPIE notation (empty when none), and for an Anagram row one such
  pair per word separated by ` / `. `grade` is `correct`, `missed` or empty. A
  Leave Value `answer` is the value as the [Answers](#answers) rule writes it
  for exports (no `+`), and a Leave Value row's `definition` and `hooks` are
  empty; the dialog does not offer those columns for a Leave Value cascade. The header row is the column names in the order chosen.
- The file is named after the source, e.g. `CSW24 7s - L2 missed.txt`: the
  cascade name, then ` - L<level>` for a quiz export, then, after a single
  space, the selection's API value when it is not `all` (`correct`, `missed`
  or `ungraded`), then the extension. Every Unicode scalar value (a code point,
  never a UTF-16 code unit) outside `A–Z`, `a–z`, `0–9`, space, `.`, `_` and
  `-` is replaced with one `_`, and the name is cut to 100 characters before
  the extension. The server sends the same name in
  `Content-Disposition`.

**How it is produced.** The export is built on the device from its view of the
local rows (overlay over base, see [On the device](#on-the-device)), after
materialising the cascade if it is not yet (which needs a connection when its
index lists have never been fetched or its rows were dropped on leaving the
window, and the dialog says so), so it
works offline for a downloaded cascade, in a worker and in chunks so a
300,000-question list doesn't block the page, and handed over as a Blob. The
questions come from the `questions` store, which eviction never touches, so a
questions-only export works offline for a cascade whose keys are complete; when
they number fewer than the cascade's `question_count`, because a download never
finished, it falls back to the server like any other missing piece. The dialog's
live count comes from that same store, which is why it counts the **questions**
the selection holds rather than the lines an answers export would write: those
lines are words, they live in the `cards` store, and eviction can empty it, so a
count of lines would be the one figure the dialog could not produce offline. With
the cards complete locally it shows the entry count beside the question count.
Answers,
definitions and hooks come from the cascade's answer cards, and definitions and
hooks are only in the local cards if the user's preferences asked for them, so an
export that needs something the device doesn't have falls back to
the server, in two steps. A page cannot see the status of a navigation, and a
navigation that lands on an error body can replace the app, so the dialog first
`fetch`es `POST /api/cascades/:id/export-token` with the export's choices and
`X-Wordfall-User`: that request is the one that sees `401`, `404` and `429`, and
the dialog reacts to them as below and as every client of a limited endpoint
does. Its answer is a short-lived, single-use URL for
`GET /api/cascades/:id/export`, bound to the user and the choices, and the dialog
navigates a **hidden `<iframe>`** to it — never the tab — so the file streams to
disk as an attachment without ever being held in the tab's memory, and any
response that is not an attachment (a `204`, a `404` for a cascade purged in the
intervening second, a `500`, or the ALB's own `502`/`503` page while a task is
replaced during a deploy) renders invisibly in the frame and is discarded
instead of replacing the app. The token is the proof of account, so the
download carries no header. It is a PASETO v4.local under an **export key
derived from `SESSION_SIGNING_KEY` with HKDF** (label `wordfall export token`),
never the session key itself, so an export token can never decrypt as a session
cookie or a session token as an export token, whatever claims either parser
checks. It carries the user id, a hash of the choices, an expiry 60 seconds out
and a random `jti`, so any task can check it; its **one use** is recorded by inserting
the `jti` into `export_tokens_spent`, where a conflict means it was already used,
so a token issued by one task and redeemed on the other works, and a second
redemption on either is refused. An expired, reused or mismatched token answers
`204 No Content`, the tidiest answer for a frame that nothing reads, so a double
click or a tab throttled past the minute produces no file and no error; the dialog, having seen the token request succeed, re-enables its
button, and a retry mints a fresh token. The choices carry every choice that
shapes the bytes, including the user's leave value decimal places, so the server
never reads a preference the device may have changed offline. When that is needed and there is no connection, the dialog says
so and offers to export the questions alone, which never needs anything but
local data. A `404` from the token request means the quiz or cascade was purged on
another device or by the retention period while the dialog was open; the dialog
says it has been deleted and refreshes the page behind it, rather than offering
the offline fallback, which would fail for the same reason.

---

## Admin

Admins manage the catalog from `/admin`. It lists every letter distribution,
lexicon and leave value set, with its size, uploader, upload time and how many
cascades reference it. Admin status is the `users.is_admin`
flag. **No endpoint can set it**; it is granted with SQL (`scripts/dev.py` does
this for the local dev user), so no bug in the web API can create an admin.

### Uploads

Each upload is a form plus one file:

| Upload | Form fields | File |
|---|---|---|
| **Letter distribution** | name (defaults to the file name without `.csv`, e.g. `english`) | [Letter distribution CSV](#letter-distribution-file) |
| **Lexicon** | name (e.g. `CSW24`), letter distribution | [Lexicon TSV](#lexicon-file) |
| **Leave values** | lexicon | [Leave values CSV](#leave-values-file) |

Every upload is validated in full before anything is written. If there are
problems, nothing is stored and the page lists them with line numbers: the first
1,000, plus a total count. A valid upload is written in one transaction.

### File formats

These rules apply to all three files:

- **Encoding:** UTF-8. A leading byte-order mark is ignored.
- **Lines:** one record per line, with LF or CRLF line endings. Blank or
  whitespace-only lines are ignored. There is no header row and no comment
  syntax.
- **Fields:** each record has exactly the number of fields listed for its file,
  separated by that file's delimiter (tab or comma). There is no quoting or
  escaping, so a field can never contain its file's delimiter. Whitespace around
  each field is trimmed.
- **Tiles** (lexicon and leave value files): written in
  [MAGPIE notation](#tiles). Single-character tiles are written as they are,
  and multi-character tiles in square brackets (`A[NY]S`). In that notation,
  lower-case tiles mean a blank standing for a tile, so they are errors in these
  files.
- **Numbers:** plain decimals: an optional `+` or `-`, digits, and an optional
  `.` followed by digits. For example `542388`, `28.292000`, `-0.378`. No
  exponents, thousands separators, `NaN` or `Infinity`.
- **Size:** at most 100 MB.

In the examples below, `⇥` stands for a tab character.

#### Letter distribution file

The format is MAGPIE's, the same as the files in
[MAGPIE-DATA](https://github.com/jvc56/MAGPIE-DATA/tree/main/data/letterdistributions)
(`english.csv`, `english_super.csv`, `catalan.csv`, `dutch.csv`, `french.csv`,
`german.csv`, `polish.csv`). Those files upload unchanged. It is a
comma-separated file with one line per tile and **5 or 7 fields** per line:

`letter,blank_letter,count,value,is_vowel[,fullwidth_letter,fullwidth_blank_letter]`

| Field | Rules |
|---|---|
| `letter` | How the tile is written: one or more characters, e.g. `A`, `Ą`, `Ç`, `NY`, `L·L`. Unique within the file. Apart from the blank's `?`, it cannot contain `[`, `]`, `,`, `?`, `*`, `.` or whitespace. At most **8 bytes** of UTF-8, which is below MAGPIE's `MAX_LETTER_BYTE_LENGTH` and covers every real distribution (`L·L` is 4); the cap is what lets every text column that holds tiles have a fixed size. |
| `blank_letter` | How the tile is written when a blank stands for it, conventionally the lower-case form (`a`, `ą`, `ny`, `l·l`). Unique within the file, never equal to any tile's `letter`, and with the same character rules. |
| `count` | Non-negative integer: how many of this tile are in the bag. |
| `value` | Non-negative integer: points. |
| `is_vowel` | `1` or `0`. |
| `fullwidth_letter`, `fullwidth_blank_letter` | Optional; give both or neither. These are the fullwidth forms MAGPIE uses to align text output. They are stored for fidelity but not used by the Wordfall UI. |

- **The first line is the blank** and must be `?,?,<count>,0,0`. MAGPIE treats
  the first tile as the blank. A count of `0` means a bag with no blanks.
- **Line order is tile order.** Alphagrams and leaves are sorted in this order,
  so the blank always sorts first.
- **A line with other than 5 or 7 fields is an error**, as in MAGPIE. The file
  may end with or without a newline.
- **The name** entered in the form defaults to the file name without `.csv`
  (`english`, `english_super`) and must be unused.

An excerpt of MAGPIE-DATA's `catalan.csv`:

```
?,?,2,0,0
A,a,12,1,1
B,b,2,3,0
C,c,3,2,0
Ç,ç,1,10,0
…
L,l,4,1,0
L·L,l·l,1,10,0
M,m,3,2,0
N,n,6,1,0
NY,ny,1,10,0
O,o,5,1,1
P,p,2,3,0
QU,qu,1,8,0
…
```

#### Lexicon file

A tab-separated file (`.tsv`) with three fields per line:
`word⇥playability⇥definition`. There is one line per word, in any order, and at
least one word.

| Field | Rules |
|---|---|
| `word` | 1–15 tiles of the chosen letter distribution, in MAGPIE notation, with no blanks. No duplicates. In a Catalan lexicon, `ANYS` is written `A[NY]S`. |
| `playability` | Decimal number. Higher means more playable. Words with equal values tie, as described under [Lax](#filter-reference). |
| `definition` | Required, 1–10,000 characters, no tabs. Part-of-speech tags in square brackets (`[n -S]`, `[v]`) are what the Part of Speech filter reads. |

A line with fewer or more than three fields is an error. The name entered in the
form must be unused.

```
AA⇥3811⇥(Hawaiian) a volcanic rock consisting of angular blocks of lava with a very rough surface [n -S]
AAH⇥2104⇥an interjection expressing surprise [interj] / to exclaim in surprise [v -ED, -ING, -S]
QI⇥542388⇥the vital force that in Chinese thought is inherent in all things [n -S]
```

#### Leave values file

A comma-separated file with two fields per line: `leave,value`. There is one
line per leave, in any order, and at least one leave.

| Field | Rules |
|---|---|
| `leave` | 1–6 tiles of the **lexicon's** letter distribution, in MAGPIE notation, with `?` for a blank. The tiles may be in any order within the field (`SIER?` and `?EIRS` are the same leave) and are stored in canonical order: the distribution's tile order, blank first. No tile may appear more times than the bag holds. No duplicates after canonical ordering. |
| `value` | Decimal number, with an absolute value of at most **1,000,000**. The bound is what keeps the rounding in [Answers](#answers) identical on both sides: at three decimal places the rounded integer still fits in 64 bits, so "written as an integer" has one meaning in Rust and in TypeScript. A larger value is a line error. |

The file does not have to list every possible leave; a leave missing from it
simply cannot be quizzed. The chosen lexicon must not already have leave
values. To replace them, delete the existing set (allowed only while no cascade
uses it) and upload again.

```
?,28.292000
A,-0.378000
EIRS?,34.117000
```

### Upload limits

Uploads are synchronous. The largest realistic file, around a million leaves, is
expected to validate and insert in well under a minute. Admin upload endpoints
accept request bodies up to 100 MB and have a 120-second timeout, and the ALB
idle timeout is raised to match.

### Immutability and deletion

Catalog data is **immutable once uploaded**. There are no edit endpoints; a
corrected word list is a new lexicon with a new name (`CSW24` → `CSW24-fixed` or
`CSW27`). Immutability is what lets a cascade store only question keys, lets
downloaded answers be cached forever, and makes re-running a stored search always
give the same questions.

An admin can delete an item only when nothing references it. The foreign keys
enforce this, and the admin page disables the delete button and explains what is
still using the item:

- **A letter distribution** is in use while any lexicon refers to it.
- **A lexicon** is in use while it has leave values, or while any cascade refers
  to it, either as its own lexicon or through an In Lexicon row in the
  cascade's spec. A **saved search's** In Lexicon row does not pin it: a saved
  search records its In Lexicon target by **name**, and when that lexicon is
  deleted the row is flagged on load and refused at creation, exactly as one
  naming a lexicon on another distribution is. A cascade leaves the catalog
  alone within a retention period of being trashed; a saved search can sit for
  years under an owner who never logs in again, and letting it block deletion
  would leave the admin nothing to do but edit user data by hand. If a later
  upload takes the deleted lexicon's name, the saved search resolves to it
  without comment: a name is what the user chose, a re-uploaded name is almost
  always a corrected list, and a corrected list is meant to get a new name in
  any case (see above). The distribution check still flags a mismatch.
- **A lexicon's leave values** are in use while any Leave Value cascade refers
  to them.

### Loading changes into running servers

After an upload or deletion commits, the backend runs
`NOTIFY catalog_changed`. Every backend instance `LISTEN`s on that channel and
reconciles its in-memory indexes with the database: it builds indexes for new
items and drops deleted ones. It also reconciles every 60 seconds in case a
notification is missed. Each instance records what it has loaded in
`catalog_instance_status` (one row per instance and item, with a heartbeat
the instance refreshes every 60 seconds), so a new item appears in the
cascade builder once every instance with a heartbeat in the last 3 minutes
has loaded it; until then, the item is marked **loading** in `/admin`, which
lists each instance's rows. An instance writes **no** rows until its startup
load is complete, the moment `/health` turns ready, so a task still booting
during a deploy never makes an item look unloaded, and it deletes an item's
row when it drops that item's index, so a deleted item leaves `/admin` at
once. Rows whose heartbeat is older than 3 minutes are
ignored and pruned by the purge task.

---

## Tiles

Words, alphagrams, leaves, patterns and question keys are all **sequences of
tiles**, not strings of English letters. Tiles follow MAGPIE's letter
distributions.

- **Tile.** Each tile is written as its distribution's `letter`. Usually that
  is one character (`A`, `Ą`, `Ç`), but it can be several (Catalan `NY`, `QU`,
  `L·L`). Length always means number of tiles, so Catalan `ANYS` is 3 tiles.
- **Tile order** is the line order of the distribution file. Alphagrams and
  leaves are sorted in that order, so a blank comes first (`?EIRS`).
- **Alphabetical order** of words, wherever this plan says it (answer lists,
  exports, typed-mode order, rank tie-breaks), means comparing two words tile by
  tile in tile order, with a word that is a prefix of another sorting first. For
  plain A–Z distributions this is ordinary string order; with multi-character
  tiles it differs from Zyzzyva's string order (`L·L` sorts after `L`, not
  between `LA` and `LZ`). Hook lists, on screen and in exports, are in tile
  order, and the answer cards carry them already in that order, so the device
  never sorts them. On screen a hook list is lower-cased only where the tile is
  a single ASCII letter (see [Answers](#answers)); in exports it is always
  upper-case MAGPIE notation.
- **MAGPIE notation** is how tile sequences are written in upload files, stored
  keys and the API. Single-character tiles are written as they are, and
  multi-character tiles in square brackets: `A[NY]S`, `?A[L·L]`. The notation is
  unambiguous. In MAGPIE, lower-case tiles (a tile's `blank_letter`) mean a blank
  standing for that tile. Wordfall never stores them.
- **Display** shows each tile's `letter` without brackets. A multi-character
  tile is drawn as one joined tile, so `ANYS` visibly reads as three tiles.
- **Typed text** in filter inputs, In Word List entries and typed-mode answers
  is converted to tiles in three steps:
  1. Upper-case it.
  2. Treat any bracketed group as a multi-character tile (`A[NY]S`), except in
     pattern inputs.
  3. Split the remaining text at spaces, and match each piece greedily, longest
     tile first, so `ANYS` becomes `A`, `NY`, `S` and `AN YS` becomes `A`,
     `N`, `Y`, `S`. There is no backtracking: a piece that greedy matching
     cannot consume in full is an error ("not a tile of this distribution"),
     even if another split would have worked.

  In pattern inputs, square brackets already mean a set of tiles (see
  [Pattern syntax](#pattern-syntax)). There, multi-character tiles are typed
  plainly or inserted from the tile palette. A **space separates tiles**
  everywhere, and the palette inserts a tile with a space on each side, so a
  palette `N` followed by a typed `Y` stays two tiles. That is how to enter a
  sequence that greedy matching would otherwise join.
- **The canonical form.** The client does the conversion and sends tiles in
  their canonical text form, which is also what is stored and what the server
  parses: MAGPIE notation for tile lists (Includes Letters, prefixes, suffixes,
  word list entries, answers), and for patterns the tokens separated by single
  spaces (`. W * M . S`, `[A NY] L`). The server rejects anything that is not in
  canonical form, so both sides always tokenise the same text the same way. The
  rule covers tile and pattern parameters only: Definition's text is matched as
  typed, and In Lexicon names a lexicon.
- **Blank.** Written, typed and displayed as `?` everywhere: in upload files,
  In Word List entries, Includes Letters and patterns. The single-tile wildcard
  in patterns is `.`, never `?`, so the two never collide.
- **Vowels and point values** come from the distribution, so Number of Vowels,
  Consists of `AEIOU`-style sets, Point Value and probability all work for any
  language.

Words and leaves both use the **lexicon's** letter distribution.

---

## Filters

There are 23 filters: 20 of Zyzzyva's condition types, in the order of its
search dropdown, plus three Wordfall-only filters at the end: **Front Inner
Hook**, **Back Inner Hook** and **Leave Value**. Zyzzyva's Belongs to Group
condition is deliberately left out; the other filters are enough to build any
study list, and its "Inner Hooks" group is what the two inner hook filters
replace. A filter has a type, a Not flag, and parameters, and it is either a
**predicate** or a **limit**:

- A **predicate** tests one candidate on its own. Zyzzyva checks predicates in
  three phases for speed: a word graph walk, then SQL, then post-processing. That
  split is an optimization, not part of their meaning. Wordfall evaluates every
  predicate in memory (see [Search Engine](#search-engine)).
- A **limit** (Limit by Probability Order, Limit by Playability Order) ranks
  the candidates that passed every predicate and keeps a range of that ranking.
  Limits are **always applied last within their group, after that group's
  predicates**, however the rows are ordered (see [Groups](#groups)).

### Groups

Filter rows live in **groups**. A group has an operator, **AND** or **OR**, and
holds rows and other groups in order. The top of the form is a group, AND by
default, so a plain list of rows means what it does in Zyzzyva: every row must
match. For anything else the user wraps rows in a group and picks its operator,
so the precedence is whatever the user builds:

- `Length 7 AND (Includes Q OR Includes Z)`: an AND group at the top holding a
  Length row and an OR group of two Includes Letters rows.
- `(Length 7 AND Probability Order 1–500) OR (Length 8 AND Probability Order
  1–300)`: an OR group at the top holding two AND groups.

The rules:

- **Every group is evaluated over a candidate set.** The top group's
  candidates are the whole target. A child group of an **AND** group gets its
  parent's candidates narrowed by the parent's **predicate rows** (not by its
  sibling groups or by the parent's limits), and a child group of an **OR**
  group gets its parent's candidates unchanged. So a nested group only ever
  sees words its enclosing AND rows already accept.
- **A group's result is a set of candidates.** A predicate row's result is the
  candidates it matches. An AND group intersects its children's results and an
  OR group unions them, once every child has been evaluated.
- **Limits belong to the group they are in.** A group's limit rows rank the
  group's combined result, and the kept slice is what the group passes to its
  parent. A limit at the top ranks the final result, which is Zyzzyva's
  behaviour when there are no groups. Several limit rows of the same kind in
  one group combine as under [Lax](#filter-reference). An OR group's limits
  rank the union of its other children. A group whose children are all limit
  rows ranks every one of its candidates: at the top that is every word of the
  target, which is what `Limit by Probability Order 1–50` on its own means in
  Zyzzyva, and under an AND group it is that group's predicate survivors, so
  `Length 7 AND (Limit 1–50)` means the same as `Length 7, Limit 1–50`.
  Because candidates come from the parent, `Length 7 AND (Includes V AND Limit
  by Probability Order 1–50)` is the 50 most probable 7s with a V, not the 50
  most probable V-words of any length cut down to the 7s.
- **Not** applies to a row, never to a group. A negated group can always be
  written by negating its rows and flipping its operator.
- **Validation.** A group must hold at least one row or group. An empty group,
  a group nested more than 4 deep, a form with more than 100 rows in total, or
  one with more than **100 groups** counting the top one is an error. The group
  cap matches the column that holds a group's id (see [Schema](#schema)) and is
  a real limit, not a restatement of the row cap: 100 rows each wrapped in their
  own group is 101 groups, legal by every other rule here, and without this check
  it would be refused by a constraint violation on insert rather than by a field
  error on the row that crossed it. Applicability is checked per row, wherever the row sits, and
  errors are keyed by the row's **path**: its child indexes from the top group,
  such as `[1, 0]`.

On the form, each row's `+` menu offers **Add row** and **Add group**, a group
is drawn as an indented box with an AND / OR switch in its header, and rows and
groups can be dragged between groups. A saved search stores the whole tree. The
cascade's default name writes groups with parentheses, such as
`CSW24 · Length 7 · (Includes Q or Includes Z)`.

### Pattern syntax

Anagram Match, Pattern Match and Subanagram Match take a pattern made of tiles
plus:

| Token | Meaning |
|---|---|
| `.` | Any single tile. In a Leave Value pattern that includes the blank. |
| `*` | Any number of tiles, including none. More than one `*` in an anagram or subanagram pattern means the same as one. |
| `[ABC]` | Exactly one tile from the set |
| `?` | The blank tile itself (Leave Value quizzes only), exactly as it is written in leave files and leaves |

`.` is the only single-tile wildcard. Zyzzyva uses `?` for it; Wordfall does
not, because `?` is how the blank is written everywhere else, and a leave
pattern needs both.

Input is upper-cased as it is typed and converted to tiles, with multi-character
tiles matched greedily, separated by spaces, or inserted from the tile palette
(see [Tiles](#tiles)). Brackets in a pattern always mean a tile set, never
MAGPIE's multi-character notation, so `[A NY]` and `[ANY]` both mean "`A` or
`NY`" in a Catalan lexicon. A tile that is not in the distribution, an
unbalanced bracket, or an empty bracket fails validation, and so does `?` in
any pattern or tile input (Includes Letters, Consists of, Takes Prefix or
Suffix, In Word List) of an Anagram or Definition cascade, with the message
"use `.` for any single tile", since that is what Zyzzyva users will type.

### Filter reference

Unless a row says otherwise, "word" means the candidate: a word in Anagram and
Definition quizzes, or a leave in Leave Value quizzes. Lengths and counts are in
tiles.

| # | Filter | Parameters | Not | Meaning (as implemented in Zyzzyva) |
|---|---|---|---|---|
| 1 | **Anagram Match** | pattern | ✓ | The word uses exactly the pattern's tiles, in any order. `.` and `[..]` each stand for one tile; `*` allows any number of extra tiles. `ETX.` → EXIT, NEXT, SEXT, TEXT, VEXT. |
| 2 | **Pattern Match** | pattern | ✓ | The word matches the pattern in order, left to right. `T.P` → TAP, TIP, TOP, TUP. `.W*M.S` → SWAMIS, SWAMPS, TWASOMES, … |
| 3 | **Subanagram Match** | pattern | ✓ | Every tile of the word can be taken from the pattern; not every pattern tile has to be used. `LX.` → AL, AX, EL, … LAX, LEX, LOX, LUX. A `*` matches everything. |
| 4 | **Length** | min, max (1–15) | — | The word has min–max tiles. Setting min = max gives an exact length. |
| 5 | **In Lexicon** | lexicon | ✓ | The word is also valid in a second lexicon that uses the **same letter distribution** as the cascade's lexicon; the dropdown lists only those, and any other is a validation error, so tiles are always compared within one distribution. Negated, it finds words that are new or unique compared with that lexicon, e.g. CSW24 words not in CSW21. |
| 6 | **In Word List** | list of words | ✓ | The word appears in a list the user pastes or uploads (one word per line, up to 300,000 entries). **300,000 is the limit for a whole filter tree, not for one row**: several In Word List rows share it, and the row that takes the total past it is a field error naming the total, checked on preview, on save and on creation. That is the figure `API_MAX_BODY_BYTES` and the storage estimate under [Capacity](#deployment-and-operations) are both sized against, and without it eight rows of 250,000 entries would fit in one 16 MB body and cost forty times the 6 MB one list does. The list is saved with the filter. Entries that are not valid in the cascade's lexicon are ignored. |
| 7 | **Number of Vowels** | min, max | — | Count of the distribution's vowel tiles is within min–max. |
| 8 | **Includes Letters** | tiles | ✓ | Each tile appears in the word at least as many times as it appears in the parameter (`EE` means two or more Es). Negated, as in Zyzzyva, the word does **not** contain all of them: `Not Includes U` matches words without a U, so `Includes Q` plus `Not Includes U` finds Q-without-U words, while `Not Includes AB` matches words that lack an A or lack a B. To exclude every tile of a set, add one Not row per tile; the row's help text says so. |
| 9 | **Probability Order** | min, max, lax | — | The word's precomputed probability rank among **all words of the same length** in the lexicon is within min–max. Probability always assumes two blanks; there is no blanks parameter. See [Probability](#probability-and-probability-order). |
| 10 | **Limit by Probability Order** | min, max, lax | — | *Limit.* Rank the words that survived every predicate by probability, then keep ranks min–max of that list. Example: `Length 7`, `Includes V`, `Limit 1–50` gives the 50 most probable 7s with a V. |
| 11 | **Playability Order** | min, max, lax | — | The word's precomputed playability rank among all words of the same length is within min–max. |
| 12 | **Limit by Playability Order** | min, max, lax | — | *Limit.* Like Limit by Probability Order, but ranks by playability value. |
| 13 | **Number of Unique Letters** | min, max | — | Count of distinct tiles is within min–max. |
| 14 | **Point Value** | min, max | — | Sum of tile values is within min–max. Tiles count at face value even when a word needs a blank (ZYZZYVA = 43 in English). The maximum allowed is 15 × the distribution's highest tile value, or 6 × for a Leave Value cascade. |
| 15 | **Takes Prefix** | tiles | ✓ | Prefix + word is also a valid word. `PRE` with `VAL*` keeps VALENCE but not VALID. |
| 16 | **Takes Suffix** | tiles | ✓ | Word + suffix is also a valid word. |
| 17 | **Part of Speech** | one of: Adjective, Adverb, Conjunction, Definite Article, Indefinite Article, Interjection, Noun, Preposition, Pronoun, Verb | ✓ | The definition contains that part-of-speech tag in brackets: `[adj`, `[adv`, `[conj`, `[definite_article`, `[indefinite_article`, `[interj`, `[n`, `[prep`, `[pron`, `[v`. Zyzzyva matches `[tag ` (tag, then a space) or `[tag]`, so `[n -S]` and `[n]` are nouns and `[interj]` is not. |
| 18 | **Definition** | text | ✓ | The definition contains the text as a literal, case-insensitive substring. No wildcards. |
| 19 | **Consists of** | tiles, min %, max % | — | `floor(100 × (tiles of the word that are in the set) / length)` is within min–max. Example: `AEIOU`, 70–100 finds words that are at least 70% vowels. |
| 20 | **Number of Anagrams** | min, max | — | The number of valid words with this word's alphagram (including itself) is within min–max. |
| 21 | **Front Inner Hook** *(Wordfall only)* | — | ✓ | The word with its first tile removed is also a valid word: SPORT, because PORT is a word. A one-tile word never has one. Zyzzyva has no condition for this; it is the front half of its "Inner Hooks" group. |
| 22 | **Back Inner Hook** *(Wordfall only)* | — | ✓ | The word with its last tile removed is also a valid word: SPORTS, because SPORT is a word. |
| 23 | **Leave Value** *(Wordfall only)* | min, max (decimals; either may be blank) | — | *Leave Value quizzes only.* The leave's stored value is between min and max, inclusive. A blank bound is open, so `min 10, max blank` means "worth at least 10". At least one bound is required, and min ≤ max when both are given. The comparison uses the full stored value, not the rounded display value. |

Details that are easy to get wrong:

- **Range defaults.** A new integer range row starts at the smallest allowed
  value (0, or 1 for Length and the four order filters, whose ranks are
  1-based) and the maximum allowed value, its **ceiling**:
  15 for Length, Number of Vowels and Number of Unique Letters, 15 × the
  highest tile value for Point Value, 100 for Consists of, and, for Number of
  Anagrams, the **largest `num_anagrams` in the target**, which the
  [index](#derived-attributes) already computes for every word and leave and
  which `GET /api/lexicons` reports as `max_num_anagrams` and
  `max_leave_num_anagrams`, since nothing else would tell the builder where this
  one tops out. For the **four order filters** the ceiling is the largest rank
  a candidate can actually hold: the size of the **largest length bucket**, which
  the same endpoint reports as `max_order_rank` and `max_leave_order_rank`, for
  Probability Order and Playability Order, whose ranks run within a length (or,
  on leaves, within a size), and the whole target's size (`word_count` or
  `leave_count`) for the two **limit**
  filters, which rank the survivors of a group together whatever their length.
  Reading the target's size for all four would put the two predicates' default
  max past any rank that exists, and a row stopping at the largest bucket would
  then be accepted although it narrows nothing. On a Leave Value cascade the 15s
  become 6 and
  the Point Value ceiling 6 ×, and the anagram ceiling is the leave set's own
  largest count; in the degenerate case where that is 0, because every leave in
  the set holds a blank, no Number of Anagrams range can narrow anything and
  every such row is refused by the rule below. A row is valid only if it actually narrows
  something, which means min above **that filter's own smallest allowed value**
  or max below the ceiling, never min above 0: the floor is 1 for Length and the
  four order filters, so a test written against 0 would accept `Length 1–15` and
  `Probability Order 1–N`, which are the defaults and narrow nothing. A row also
  needs min ≤ max, and a bound above the ceiling is a
  field error naming the ceiling, so a saved `Length 4–15` loaded into a Leave
  Value cascade is flagged rather than silently clamped. Leave Value rows start with both
  bounds blank and are invalid until one is filled in. Inner hook rows have no
  parameters.
- **Lax** (the order filters). Every word has a unique rank, plus the lowest and
  highest rank shared by words with the *same* value (`min_order`, `max_order`).
  Ties are broken by alphagram, then by the word. Strict mode compares the
  unique rank. Lax mode matches any word whose tie range overlaps min–max,
  meaning `max_order ≥ min && min_order ≤ max`, so a whole group of tied words
  is taken or left together. Lax is on by default, as in Zyzzyva.
- **Lax for the limit filters**, exactly as Zyzzyva's `WordEngine` does it. The
  survivors are ranked by (value descending, alphagram, word), where the value
  is the word's combinations or playability itself, never its per-length rank,
  so a limit over `Length 7–8` ranks the 7s and 8s together by raw value; the
  [parity list](#zyzzyva-parity) holds that case. Within one group
  the limit rows of one kind are reduced to a strict range and a lax range,
  each the intersection of its rows (highest min, lowest max), a missing one
  being 1…∞. The kept slice starts at the higher of the two mins and ends at
  the lower of the two maxes, and is then widened to include neighbours with an
  equal value, but never past the **strict** min or max. So a lax row alone
  widens freely, a strict row alone never widens, and a strict row caps a lax
  one. If the min is past the last survivor the result is empty.
- **Both limit kinds in one group** each rank the group's predicate result
  **independently**, and the kept set is the **intersection** of the two slices.
  Nothing applies one kind to the other's output, so the answer cannot depend on
  which kind goes first — the same property the same-kind rule above has, where
  row order never decides. `Length 7` with `Limit by Probability Order 1–50` and
  `Limit by Playability Order 1–10` is therefore the 7s that are both among the
  50 most probable and among the 10 most playable, which may be fewer than ten
  and may be none, and not "the 10 most playable of the 50 most probable" or the
  reverse — two readings that give different word lists from the same rows, and
  a cascade keeps only its question index, so the difference would never be
  visible again. Zyzzyva is not the authority here, since it has no comparable
  case, which is why the [parity list](#zyzzyva-parity) holds none.
- **Several rows of one type** in an AND group are simply ANDed. Two Length
  rows intersect; two Includes Letters rows both have to hold. In an OR group
  either may hold.

### Probability and probability order

Following Zyzzyva's `LetterBag::getNumCombinations`, a word's **combinations**
is the number of distinct draws from the bag that spell the word, allowing up to
two of the bag's blanks to stand in. Zyzzyva lets the user choose 0, 1 or 2
blanks; Wordfall always uses 2, Zyzzyva's default, since that is what players
mean by probability. It is the sum of three terms:

- **no blank**: the product over each distinct tile of `C(count in bag, count
  in word)`
- **one blank**: for each distinct tile, `C(blanks in bag, 1)` times the
  product with that tile's count in the word reduced by one
- **two blanks**: for each unordered pair of tile slots (the same tile twice
  when the word holds it at least twice), `C(blanks in bag, 2)` times the
  product with both counts reduced

With no blanks in the distribution, the blank terms are zero. Probability order
ranks all words of a length by combinations, highest first, with ties broken by
alphagram and then word.

### Filter applicability by quiz type

**Anagram quizzes** search over words, then turn the matches into questions:
each distinct alphagram among them is one question. The answer is every valid
word with that alphagram, **including words that did not match the filters**.
The question is "what can these tiles make", not "which of these tiles'
anagrams passed my filters". Limit filters rank words, not alphagrams, the way
Zyzzyva does, and the limited words are then collapsed to alphagrams.

**Definition quizzes** search over words, and each matching word is one
question.

**Leave Value quizzes** search over the **lexicon's leave values**. A leave has
no word-only attributes (validity, definition, playability, hooks), so filters
that depend on those do not apply. Leave probability uses the lexicon's letter
distribution, with the blank treated as an ordinary tile.

| Filter | Anagram | Definition | Leave Value |
|---|---|---|---|
| Anagram / Pattern / Subanagram Match | ✓ | ✓ | ✓ (Pattern Match matches against the alphabetized leave) |
| Length | ✓ | ✓ | ✓ (1–6) |
| In Lexicon | ✓ (same distribution) | ✓ (same distribution) | — |
| In Word List | ✓ | ✓ | ✓ (the list holds leaves; each entry is put in canonical order on input, which is why a type change flags the row) |
| Number of Vowels | ✓ | ✓ | ✓ (the blank is not a vowel) |
| Includes Letters | ✓ | ✓ | ✓ |
| Probability Order / Limit by Probability Order | ✓ | ✓ | ✓ (ranked among leaves of the same size, with the blank as an ordinary tile and no blank substitution) |
| Playability Order / Limit by Playability Order | ✓ | ✓ | — |
| Number of Unique Letters | ✓ | ✓ | ✓ (the blank is one distinct tile) |
| Point Value | ✓ | ✓ | ✓ (a blank is worth 0) |
| Takes Prefix / Takes Suffix | ✓ | ✓ | — |
| Front Inner Hook / Back Inner Hook | ✓ | ✓ | — |
| Part of Speech / Definition | ✓ | ✓ | — |
| Consists of | ✓ | ✓ | ✓ (the blank is an ordinary tile: `?` may be in the set and matches only itself) |
| Number of Anagrams | ✓ | ✓ | ✓ (valid words in the lexicon using exactly the leave's tiles; 0 if the leave contains a blank) |
| Leave Value | — | — | ✓ |

Changing the quiz type on the creation form keeps the filter rows that still
apply and flags the ones that don't, rather than deleting them silently.
Changing the lexicon, or loading a saved search, flags In Lexicon rows whose
target is on another distribution the same way, and loading a saved search also
flags one whose target lexicon has since been deleted, since a saved search
names its target rather than pinning it (see
[Immutability and deletion](#immutability-and-deletion)). **An In Word List row is flagged
on a change to or from Leave Value**, although the filter applies to every type,
because its entries are stored in the canonical form of the type they were
entered under: a leave is sorted into tile order and a word is not, so a list of
leaves read as words, or of words read as leaves, would silently match almost
nothing rather than fail. That holds on **both** paths, which is why
`search_specs` records a `quiz_type`: a change inside the form compares against
the type the form had a moment ago, and **loading a saved search** compares
against the type stored on its spec, since nothing else on the load path could
tell leave-canonical entries from word-canonical ones. The row's editor recounts
on every type or lexicon
change and says how many entries are now invalid, so a list that has become
empty is visible before Create Cascade rather than after.

---

## Architecture

```
Browser
├── Service worker        (caches the app so it loads offline)
├── IndexedDB             (cascades, quizzes, grades, question keys, answers, outbox of operations)
└── Sync engine ──HTTPS──▶ ALB ──▶ ECS Fargate task
                                   ├── nginx      (SvelteKit static build; proxies /api)
                                   └── wordfall   (Axum; in-memory catalog indexes;
                                         │  ▲      cascade rules; sync; purge task)
                                         ▼  │ LISTEN catalog_changed
                                    RDS Postgres  (users, preferences, catalog, saved searches,
                                                   cascades, quizzes, grades, sync bookkeeping)
```

- **Postgres is the source of truth** for everything.
- **The backend holds a read-only in-memory index** for each lexicon and each
  leave value set, built from Postgres at startup and kept current through
  `LISTEN/NOTIFY`. Search and answer lookups use these indexes, so no search query
  ever hits the database.
- **The browser is local-first for studying.** The player only ever reads and
  writes IndexedDB. A sync engine sends the user's operations to the server and
  pulls back changes, whether the connection comes and goes or never drops. See
  [Offline and Sync](#offline-and-sync).
- **Cascade rules exist twice**, in Rust on the server and in TypeScript in the
  browser. Both must pass the same shared test vectors (see [Testing](#testing)).

### Tech Stack

| Concern | Decision |
|---|---|
| Language (backend) | Rust (stable toolchain, pinned via `rust-toolchain.toml`) |
| Web framework | Axum |
| DB access | SQLx (compile-checked queries, migrations run at startup) |
| Database | Postgres 16 (RDS in production) |
| Frontend framework | SvelteKit, built as a static SPA with `adapter-static` |
| Styling | Tailwind CSS |
| Component library | shadcn-svelte, dark mode only (Tailwind `darkMode: 'class'` with `dark` always on the root) |
| Offline app shell | SvelteKit service worker (`src/service-worker.ts`) |
| Local storage | IndexedDB via the `idb` wrapper |
| Frontend serving | Nginx container serving the static build and proxying `/api` to the backend |
| Compute | AWS ECS on Fargate: one task definition with two containers (backend and Nginx) |
| Load balancer | ALB, HTTPS only (port 80 redirects) |
| Auth | Built in-house: Argon2 password hashes and PASETO v4.local session cookies |
| Email | AWS SES (a `console` backend locally) |
| Rate limiting | `governor` middleware (in-memory token buckets, per backend instance; see [Security](#authentication)) |
| Compression | `tower-http` gzip and brotli for API responses (answer card pages in particular) |
| Secrets | AWS SSM Parameter Store, injected into the task as environment variables |
| Infrastructure as code | Terraform (VPC, ALB, ECS, RDS, SES, SSM, backups) |
| Logging | `tracing` with `tracing-subscriber` JSON output to CloudWatch |
| Local development | Docker Compose: Postgres, backend and Nginx frontend, plus an optional Vite dev server profile for hot reload |

Object storage is not needed in v1. Uploaded files are parsed and written
straight into Postgres; the original files are not kept.

### Repository layout

| Path | Contents |
|---|---|
| `backend/` | Axum and SQLx server: auth, admin uploads, catalog indexes, search engine, cascade rules, sync, exports, purge task. `migrations/0001_initial.sql`. |
| `frontend/` | SvelteKit SPA, including `lib/cascade/` (rules), `lib/local/` (IndexedDB), `lib/sync/` (sync engine and downloads) and `lib/export/` (word lists). |
| `contract-fixtures/` | Shared JSON test data: filter types and parameters, cascade rule vectors, shuffle vectors, export fixtures (see [Testing](#testing)). |
| `fixtures/catalog/` | The committed [fixture catalog](#the-fixture-catalog): unlicensed distributions, lexicons and leave values, used by the unit tests, the end-to-end tests and `./scripts/dev.py` alike. |
| `e2e/` | Playwright configuration, `globalSetup` and the [journeys](#the-journeys). |
| `docker/` | Backend Dockerfile (multi-stage Rust build → `debian:bookworm-slim`). |
| `infra/` | Terraform. |
| `scripts/` | `stack.py` (the one way to bring a Wordfall up: compose, health, seed, reset, down), `dev.py` (its command line), `backup.py` and `restore.py`. |
| `Makefile` | The [test targets](#running-the-tests), each runnable with no arguments. |
| `docker-compose.yml` | Local stack. |

Lexicon data files are licensed (e.g. Collins Scrabble Words © HarperCollins)
and are **never committed**. Admins upload them from files they supply.

---

## Catalog Indexes

### Derived attributes

Postgres stores uploaded data as given. The backend builds each lexicon's
in-memory `LexiconIndex`, computing for every word:

- the word as a tile sequence, `alphagram`, `length`, `num_vowels`,
  `num_unique_letters`, `point_value`, and a letter count vector sized to the
  distribution
- `num_anagrams` (from an alphagram → words map, which also serves Anagram
  answers)
- `front_hooks` and `back_hooks` (for the hooks display preference)
- `has_front_inner_hook` and `has_back_inner_hook`: whether the word minus its
  first, or last, tile is in the lexicon
- `combinations` (with two blanks), `probability_order`,
  `min_probability_order` and `max_probability_order`
- `playability_order`, `min_playability_order`, `max_playability_order`
- parsed parts of speech from the definition tags

Each leave value set gets a `LeaveSetIndex` with the same tile-based attributes
for every leave (length, vowels, unique letters, point value, combinations and
probability order within size, anagram count against its lexicon), plus the
value. It also holds the two structures the search's candidate shortcuts need:
per-size buckets, and a lookup from a canonical leave to its entry, which is
what a literal Anagram Match narrows by on leaves where a word search uses the
alphagram map (see [Search Engine](#search-engine)).
Each index also keeps the two figures a filter row's [ceiling](#filter-reference)
needs and nothing else on the device could work out: the **largest
`num_anagrams`** it holds, and the size of its **largest length bucket** (its
largest size bucket for leaves), which is the highest per-length rank a
candidate can have. `GET /api/lexicons` reports both.

Computing these when an index is built, instead of storing them, means there is
only one implementation of each attribute and no derived column can drift from
the inputs. It is also cheap: roughly 280,000 words and about 1 million leaves
take a few seconds of single-core work, which startup and a reload can afford.
If that is measured and turns out to be too slow, the fix is to cache the built
index, not to add columns.

Memory use is on the order of 100 MB per full lexicon including definitions,
plus about 50 MB per full leave value set. The Fargate task is sized for the
catalog offered, and each index's size is logged when it is built and shown in
`/admin`.

**Where those two figures are checked.** Neither the few seconds nor the 100 MB
means anything on [the fixture catalog](#the-fixture-catalog), and licensed data
never reaches CI, so no unit or scale test holds them to a budget. Every index's
build time and resident size are logged when it is built and shown in `/admin`,
the [parity run](#zyzzyva-parity) records both for a real CSW24 upload and its
leave value set, and the runbook compares the recorded total against the task's
memory reservation before a catalog is offered in production.

`/health` reports ready only once every catalog item in the database has been
indexed at startup. Items that arrive later are built in the background and
never block requests.

---

## Search Engine

`backend/src/search/` is a pure Rust module with no I/O:

```rust
pub struct SearchSpec { pub root: Group }

pub enum GroupOp { And, Or }
pub struct Group { pub op: GroupOp, pub children: Vec<Node> }
pub enum Node { Condition(Condition), Group(Group) }

pub enum ConditionKind {
    AnagramMatch(Pattern), PatternMatch(Pattern), SubanagramMatch(Pattern),
    Length(Range), InLexicon(LexiconId), InWordList(HashSet<TileString>),
    NumVowels(Range), IncludesLetters(TileCounts),
    ProbabilityOrder { range: Range, lax: bool },
    LimitByProbabilityOrder { range: Range, lax: bool },
    PlayabilityOrder { range: Range, lax: bool },
    LimitByPlayabilityOrder { range: Range, lax: bool },
    NumUniqueLetters(Range), PointValue(Range),
    TakesPrefix(TileString), TakesSuffix(TileString),
    PartOfSpeech(Pos), Definition(String),
    ConsistsOf { tiles: TileSet, min_pct: u8, max_pct: u8 },
    NumAnagrams(Range),
    FrontInnerHook, BackInnerHook,
    LeaveValue { min: Option<f64>, max: Option<f64> },
}

pub struct Condition { pub kind: ConditionKind, pub negated: bool }

pub enum Target<'a> { Words(&'a LexiconIndex), Leaves(&'a LeaveSetIndex) }

pub fn search(target: Target, catalog: &Catalog, quiz_type: QuizType,
              spec: &SearchSpec) -> Result<Vec<QuestionKey>, SearchError>;
```

Filter inputs arrive in their [canonical text form](#tiles) and are parsed into
tiles against the target's distribution during validation, so the engine itself
only compares small tile indexes (`u8`).

How a search runs:

1. **Validate** the spec against the quiz type and target: applicability,
   negation allowed, ranges, pattern syntax, tiles present in the distribution,
   and the [group](#groups) rules. It returns every error, keyed by row path,
   so the form can mark each bad row.
2. **Pick candidates.** Words or leaves of the target. If the top group is AND
   and has a Length row, iterate only the per-length buckets inside its range,
   and if it has an Anagram Match that is not negated and holds only literal
   tiles, start from the alphagram map for a word target or the canonical-leave
   lookup for a leave target. These are shortcuts only; the results
   must match a full scan. They are safe whatever the tree holds, because a
   nested group's candidates are already narrowed by the top group's predicate
   rows (see [Groups](#groups)), which is exactly what the shortcut narrows by.
3. **Evaluate the tree.** Each group receives its candidate set as
   [Groups](#groups) defines it and produces a bitset over it. In an AND group
   the predicates are applied to each candidate cheapest first (integer and
   leave value ranges, then tile counts, then patterns, then definition
   substring scans) and stop at the first failure; the survivors are the
   candidates handed to its child groups, whose bitsets are then intersected
   in. In an OR group every child is evaluated over the group's own candidates
   and the bitsets are unioned.
4. **Apply limits** inside each group to that group's result, grouped by kind,
   with lax widening as described in [Filters](#filters), before the result is
   handed to the parent group. Each kind ranks that same result, never the other
   kind's output, and the two slices are intersected, so the order the kinds run
   in is not a decision the engine has to make. The top group's result is the
   search result.
5. **Make questions.** Anagram: dedupe alphagrams. Definition: words.
   Leave Value: canonical leaves. The result is sorted in
   [alphabetical order](#tiles) of the question key. That
   order becomes the cascade's question index (see [Cascades](#cascades)).

Pattern matching:

- **Anagram and Subanagram** compare tile count vectors. `.` and bracket sets
  are matched by a small bipartite assignment: sets are few and short, so a
  greedy most-constrained-first assignment with backtracking is enough.
- **Pattern Match** compiles to an anchored matcher over tile indexes (`.` → any
  one tile, `*` → any run, `[..]` → a tile set, `?` → the blank tile). Compiled patterns are cached
  per request.

The search runs on `tokio::task::spawn_blocking`. A search over a full lexicon
is expected to take tens of milliseconds. That figure stands like the index
sizes in [Catalog Indexes](#derived-attributes): the fixture catalog is too
small to measure it, so the [parity run](#zyzzyva-parity) records the
wall-clock time of every search on its list against a real CSW24 upload, and a
regression shows up there rather than in CI. A `SEARCH_TIMEOUT_MS` budget (default
2 seconds) is checked cooperatively inside the scan, every 4,096 candidates and
before each limit ranking; when it is exceeded the search stops and the request
returns `422` with "search too broad". A blocking task cannot be cancelled from
outside, so the check inside the loop is what stops one broad search.

It does not bound how many searches run at once, and the blocking pool holds far
more threads than the task has cores, so **`SEARCH_CONCURRENCY` (default 2, the
task's vCPU count) is a semaphore taken before `spawn_blocking`**. Without it,
four users at the default `SEARCH_RATE_PER_MINUTE` would each get a fraction of
a core, every search would reach its deadline having done little work, and a
perfectly narrow search would be answered "search too broad" — which sends the
user to narrow a filter that was fine, and which the builder's debounced preview
would then repeat. A request that waits longer than `SEARCH_TIMEOUT_MS` for a
permit is answered `503` with `Retry-After` instead, so a loaded server never
blames the user's filters. The queue stays short, because an admitted search
takes tens of milliseconds.

**Result caps.** A cascade's Source quiz holds at most `MAX_QUIZ_QUESTIONS`
questions, default and ceiling **300,000**. The database enforces the same
ceiling (see [Schema](#schema)), so configuration can lower the cap but not
raise it. A search over the cap is refused with the count, so the user knows
how much to narrow it; results are never silently truncated. For a sense of
scale, all 7- and 8-letter words in CSW24 fit, but a full English leave value
set (around a million leaves) does not. Preview returns the count and the first
20 questions.

---

## Cascades

### Questions are stored once per cascade

When a cascade is created, its search results are stored once, in search order,
as the cascade's **question index**: `cascade_questions(idx → question_key)`.
Every quiz in the cascade refers to questions by `idx` rather than repeating the
key. Every level's questions come from the Source quiz's questions, so:

- a level quiz costs a few bytes per question
- the answer cards downloaded for the Source quiz cover every level that can ever
  exist in the cascade, including levels created offline

### Deterministic shuffles

Shuffles must come out the same on the device and on the server. A device can
create a level or reset a quiz while offline, and the server has to end up with
the same order the user actually studied. Rather than sending whole orderings,
an operation carries a 64-bit **shuffle seed**, and both sides compute the order
with the same algorithm:

1. Sort the questions being shuffled by `idx`, ascending.
2. Seed **SplitMix64** with the seed: Vigna's reference `splitmix64.c`, where
   each call adds `0x9E3779B97F4A7C15` to the state, then mixes the result
   with `z ^= z >> 30; z *= 0xBF58476D1CE4E5B9; z ^= z >> 27;
   z *= 0x94D049BB133111EB; z ^= z >> 31`, all in 64-bit wrapping arithmetic,
   so the first output follows the first increment.
3. Run **Fisher–Yates** from the last position down to position 1, swapping
   position `i` with `j = next_u64() mod (i + 1)`.

The Rust and TypeScript versions are checked against the same test vectors. The
TypeScript version uses `BigInt` for the 64-bit arithmetic. The Source quiz is
shuffled the same way, with a seed the server generates and returns from
`POST /api/cascades`, so the creating device derives the order without
downloading it. **Every attempt is identified by its seed**: `quizzes` stores
the `shuffle_seed` of the current attempt, positions never travel over the wire
(a pull carries the seed and the device derives the order), and every operation
on an attempt names the seed it was played under, so a grade or cursor from a
different shuffle can be recognised (see [Operations](#operations)).

**Every quiz also carries `questions_hash`**: the FNV-1a 64-bit hash (offset
basis `14695981039346656037`, prime `1099511628211`) of its question indexes
in ascending order, each fed as a little-endian 32-bit integer, computed identically in Rust and TypeScript and fixed by a contract
vector like the shuffles. A quiz's question set never changes, so the server
computes it once at creation and stores it on the row, and every pulled quiz
row and every `finish`, `finish_segment` or `restore_quiz` result carries it.
The device compares its own rows against it whenever a quiz's rows reach the
base by any path (a pull plus index-list fetch, promotion of rows the device
built, or materialisation over retained rows), so a divergent question set is
detected and repaired rather than discovered later as rejected grades.

### Rule implementation

The rules in [Cascade Rules](#cascade-rules) are pure functions, implemented in
`backend/src/cascade/` and `frontend/src/lib/cascade/`:

```
finish(cascade, quiz, grades, shuffle_seed, new_quiz_id) → Outcome
    Finished { cleared: bool, replacement: Option<NewQuiz> }
                                                     // level ≥ 2 only: quiz to the Trash; replacement
                                                     // at the same level, or none (level removed).
                                                     // `cleared` says whether the score met the threshold.
    Descended { reset_seed, new_level: NewQuiz }     // Ladder, or the Source quiz under any progression:
                                                     // quiz reset; new quiz at level + 1
    Completed { reset_seed }                         // Source quiz, no misses: reset in place; the
                                                     // cascade records completed_at
    Reshuffled { reset_seed }                        // nothing correct; reset in place

next_boundary(quiz) → Option<position>               // smallest multiple of the quiz's segment size
                                                     // above its cursor and below its question count.
                                                     // None when the segment size is 0, when it is at or
                                                     // above the question count, or for a segment_chain
                                                     // quiz — so nothing ever divides by the size

finish_segment(cascade, quiz, grades, segment_end, shuffle_seed, new_quiz_id) → SegmentOutcome
    Drilled { new_level: NewQuiz }                   // the run's misses, one level down, segment_chain
    Continued                                        // nothing missed in the run
                                                     // both set cursor and run_start to segment_end

restore_quiz(cascade, quiz, shuffle_seed) → Restored  // pushed as the new deepest level: attempt + 1,
                                                     // cursor, counters and run_start 0, shuffled with seed;
                                                     // options copied afresh from the cascade row, only
                                                     // alphabetical order for a segment_chain quiz
```

- `finish` reads the finishing **quiz's** `progression` to choose between
  `Finished` and `Descended`: the quiz's copy is what the settings menu edits,
  and the cascade's copy only seeds new quizzes. Every quiz these functions
  create takes its
  [options](#quiz-options) from the **cascade** row, except a quiz in a segment
  chain, which is always Drill with no segments. Both sides read the same cascade
  row, so both build the same quiz. A quiz these functions create or restore
  takes `options_device_id` from the operation's device, and `options_seq`
  and `options_changed_at` from the operation, never from the cascade row.
- One `shuffle_seed` in an operation derives every shuffle that operation needs.
  The replacement, new level or restored quiz uses `seed` unchanged, and a reset
  uses `seed ^ 0x9E3779B97F4A7C15`, so a single number keeps both sides in
  agreement. Whichever it is becomes the new attempt's `shuffle_seed`.
- **The completion counters are part of these functions.** Every outcome that
  increases `depth` (`Descended`, `Drilled`, `restore_quiz`) sets
  `cascades.peak_depth = max(peak_depth, depth)` in the same step, so the
  schema's `peak_depth >= depth` holds after a restore into a completed
  cascade as much as after a descent. Every `finish` increments
  `attempts_since_completion`, and `Completed` records `completed_at`, reports
  both counters and then resets them to 1 and 0. Both sides do this inside
  the rule functions, never in the code that calls them.

### Working at 300,000 questions

Every step involving a quiz's full question set is written for the maximum size:

- **Creation** stores the question index and the Source quiz in two statements,
  each passing all values as array parameters through `UNNEST`. No per-row round
  trips.
- **Finishing** reads the missed `idx` values (up to 300,000 integers) into Rust,
  shuffles them, and inserts the new quiz through `UNNEST`.
- **Resetting** rewrites positions and clears grades in one `UPDATE … FROM
  UNNEST`. The `(quiz_id, position)` uniqueness constraint is `DEFERRABLE
  INITIALLY DEFERRED`, so rearranging positions mid-statement doesn't collide.
  The statement does not bump the question rows' `updated_seq`; only the quiz
  row is stamped, because other devices rebuild the order from the new seed.
- **Replacing** clears the old quiz before inserting the replacement at the
  same level, in that order within the transaction, because
  `quizzes_one_active_per_level` is a partial unique index, which Postgres
  checks per statement and cannot defer.
- **Answer cards** are downloaded in pages of 10,000, compressed. Anagram cards
  without definitions come to roughly 10–15 MB uncompressed for 300,000
  questions, with definitions considerably more. Definitions and hooks are only
  included when the user's preferences ask for them, and a later preference
  change refetches them (see [Downloads](#on-the-device)).
- **Sync pulls** are paged, so a first sync on a new device never builds one
  enormous response, and they carry neither positions nor index lists: a quiz
  new to the device arrives as one row plus, when its cascade is in the
  request's `question_rows_for`, its graded questions, its index
  list is fetched on demand when the device materialises it (a Source quiz's is
  implied), and a reset arrives as a new seed on the quiz row. A
  300,000-question reset therefore costs other devices one row, not 300,000.
- **Exports** are written in chunks, in a worker on the device and as a stream on
  the server, so a 300,000-question word list never needs the whole file in
  memory at once. With every anagram's words included, such a file is on the
  order of tens of megabytes.
- **Storage**: a question row plus its indexes costs roughly 100 bytes, so a
  full 300,000-question quiz is about 30 MB, and the Source quiz plus question
  index about 45 MB. This is watched on the database dashboard. The Trash's
  automatic purge keeps cleared quizzes from piling up.

---

## Offline and Sync

### Principles

- **One code path.** The player never talks to the server. It reads and writes
  IndexedDB, and every change it makes is also appended to an **outbox** as an
  operation. Being online just means the outbox empties within a second, so the
  offline case gets exercised every time anyone studies.
- **The server is authoritative.** It validates every operation against the same
  cascade rules. If it rejects one, or other devices have made changes, the
  device's local state is rebuilt from the server's.
- **Operations are safe to repeat.** Each has a device-generated UUID. Sending
  it twice, for example after a timeout, has no further effect.

### What needs a connection

| Needs a connection | Works offline |
|---|---|
| Logging in, registering, password reset | Opening the app, including after a reload or browser restart |
| — | Logging out: the signed-in pointer is cleared at once and the server call is retried later |
| Creating a cascade (search runs on the server) | Studying any downloaded cascade in both answer modes |
| Start over (creates a new cascade) | Finishing quizzes: clearing, going down, going back up |
| Saved searches, admin, account changes other than preferences | Trash: restoring quizzes and cascades (a restored quiz whose rows were dropped is playable once they are fetched), Delete forever |
| Downloading a cascade's answer cards | Changing preferences, controls and quiz options |
| Exporting answers or definitions the device has not downloaded | Exporting anything the device already has |

### On the device

**App shell.** The service worker precaches the built app (SvelteKit's
`$service-worker` `build` and `files` lists) and answers every navigation **to an
app route** with the cached `index.html`, so `/cascades/:id` loads with no
connection. A navigation whose path begins with `/api/` goes to the network
untouched, as does any request the worker does not recognise as part of the
built app: the server export is a navigation of a hidden frame to
`/api/cascades/:id/export`, and answering it with the shell would download
nothing and boot a second copy of the app inside the frame. It never caches
`/api` responses either; downloaded data lives in IndexedDB instead. **A new
app version installs and waits**, and a plain reload does not activate it: the
browser keeps the old worker while any page it controls is open, a reload does
not release the page it reloads, and the old worker answers that navigation from
its cached `index.html`, so Nginx's `no-cache` never comes into it. The worker
does **not** call `skipWaiting()` on install either, because taking over a running
page would remove the old build's lazily loaded chunks from under a card in
progress. Instead the app watches `registration.waiting`, offers "a new version
is ready", and when the user accepts it — or presses reload in the `426` state —
posts `SKIP_WAITING` to the waiting worker, which calls `skipWaiting()`. The
tab that posted it sets a flag first and reloads on `controllerchange` only when
that flag is set, which is the one reload the user asked for: the event fires in
**every** tab the new worker takes over, and the usual handler that reloads on
it would reload the others on their own initiative, losing a revealed card. Each
other tab shows "Wordfall was updated in another tab — reload to continue" and
keeps running. So that it can, the new worker's `activate` deletes the previous
build's cache only once `clients.matchAll()` finds no page still on that build,
checking again whenever an old tab reports that it is closing, so a tab left on
the old build can still load the chunks the card it is on needs. The
app never reloads on its own initiative, so a card in progress is never
interrupted by an update, and a user who never accepts keeps the old version
until the `426` line. IndexedDB schema upgrades run from versioned migrations,
and every tab listens for `versionchange` on its connections: the tab still
running the old build closes them and shows "Wordfall was updated in another tab
— reload to continue", so the new build's upgrade is never **blocked** by an
old tab holding the previous version open.

**IndexedDB stores**: one database per user id, so two accounts on one device
never mix, plus one **unscoped** database holding two things: an `accounts`
table with one row per account that has data here, and a single `signed_in`
pointer naming the account signed in now. A row carries that account's id and
username, when it last signed in, **its rows, keys and answer bytes** as its own
drop pass and eviction last measured them, and a flag set while a
`POST /api/auth/logout` for it is still unacknowledged. The pointer is what lets a
reload with no connection open the right data: the user id is needed to find the
per-user database, so it cannot live inside it, and `GET /api/auth/me` needs a
connection. Login inserts or updates that account's row, sets the pointer and
clears the unacknowledged-logout flag on **every** row, its own included,
because the response that signed this account in has already replaced the
session cookie those queued logouts existed to clear;
**every** logout clears the pointer, before
the app navigates away, so the next startup finds no signed-in user and shows the
login page rather than the last account's cascades; and removing an account's
data deletes its per-user database and empties its row, which is then deleted
too unless it still holds an unacknowledged logout (see
[Authentication while offline](#authentication-while-offline)). Everything the
Account page needs about an account other than the signed-in one is in its row,
so **no per-user database but the signed-in one is ever opened**: opening another
at this app's schema version would run that account's migrations behind its back,
and `indexedDB.databases()` is missing in Safari and Firefox in any case. This
database has its own versioned migrations, like the per-user stores. The per-user
stores are:

| Store | Contents |
|---|---|
| `meta` | user id, username, `device_id` (a UUID made once per device **and user**: it lives in this user-scoped store, so each account on a device syncs as its own device, and its `device_seq` counts from 1 for that account), sync cursor, last sync time, the trash retention period and `max_quiz_questions` as `/api/auth/me` last reported them, when this device last opened each cascade (see [Downloads](#on-the-device) for what counts as an open), the ids of the cascades marked **Keep offline** on this device, and the ids the [budget](#on-the-device) pass dropped while they were still inside the window, which are treated as outside it until the user opens them. The distribution tiles live in their own store below, not here |
| `preferences` | the user's preferences and bindings as the server last sent them, a base like the cascade stores. The view the app shows is this row with the fields carried by pending `set_preferences` and `set_bindings` operations laid over it, so a change made a moment ago cannot flip back before it is pushed, and a change rejected as `stale` reverts the moment its operation is dropped. The server stamps the row with a sync sequence when the account is created, so a device's first pull always carries it; until that first pull the documented defaults apply |
| `distributions` | the tiles (letter, blank letter, value, vowel) of every distribution the user's cascades use, keyed by name. A pulled cascade row carries its `lexicon` and `letter_distribution` names, and the download manager fetches a distribution the store lacks from `GET /api/letter-distributions/:name` before any cascade on it shows **Available offline**; typed mode on a cascade whose distribution is missing shows "needs a connection" like a missing card, since it cannot split typed text into tiles |
| `cascades`, `quizzes`, `quiz_questions`, `quiz_attempts` | the **base**: the server's rows as of the last pull, written only by the sync engine. A base question row also holds its `position`, which the device computes from the quiz's seed in a worker when it builds or rebuilds the quiz, once per attempt; the player keeps the inverse map in memory for the quiz being played. A base quiz row also carries a `pending` flag, set by the staging move and the drop pass on an **active** quiz when its question rows are yet to be fetched and its counters show any grade (an unplayed quiz needs only its index list; a cleared quiz is never flagged, since its rows are fetched only for a restore or an export), and cleared only by the full fetch sequence |
| `overlay_cascades`, `overlay_quizzes`, `overlay_quiz_questions`, `overlay_quiz_attempts` | the **overlay**: only the rows the outbox has changed or created, keyed like the base. The player's view of any row is the overlay's version if there is one, else the base's. It holds a few hundred rows in normal use, never a second copy of a cascade, except for a level created by a `finish` that is still in the outbox, which lives there until the finish is acknowledged, and except during a long offline session, when it holds one row per pending grade until the outbox drains |
| `questions` | the **question keys** of every cascade the device holds rows for, keyed by `(cascade_id, idx)`: the alphagram, word or canonical leave the card shows. Written from the same card pages the answers come from, and **never evicted**, because the key is the question itself; a cascade's keys are dropped only with its question rows, by the drop pass. About 10 bytes per question for an English alphagram, so a 300,000-question cascade costs about 3 MB; a Definition cascade on a distribution with multi-character tiles runs four or five times that, since a 15-tile key in MAGPIE notation is tens of bytes, which is why `ROW_STORAGE_BUDGET` counts this store by measurement rather than trusting the estimate. Bounded by the same window that bounds its rows |
| `cards` | the **answers** per cascade, keyed by `(cascade_id, idx)`, with the definitions and hooks when the preferences asked for them. This is the only store eviction ever touches |
| `staging_cascades`, `staging_quizzes`, `staging_quiz_questions`, `staging_quiz_attempts` | the **staging** stores, mirroring the four base stores: where a pull's rows land when the rebase cannot write them straight to the base (see [The sync cycle](#the-sync-cycle)). Written by the pull, read only by the rebase, and cleared at the start of every pull, so an interrupted one leaves nothing behind |
| `outbox` | operations not yet accepted by the server, in `device_seq` order |

**Downloads.**
- A cascade starts downloading the moment it is created. That covers the user
  who creates one just before boarding.
- Every cascade **opened or created on this device in the last 14 days** is
  kept downloaded, plus any cascade the user marks **Keep offline**. The
  window is judged entirely locally: the `meta` store records, per device and
  user, when this device last opened each cascade, where an **open** is:
  navigating to its player, creating it, exporting it, turning Keep offline on
  for it, or an `applyLocally` of a `grade`, `move_cursor`, `finish`,
  `finish_segment` or `restore_quiz` on it. Trash, purge, options and
  preferences operations never extend the window, so trashing a cascade does
  not hold its rows for a fortnight. The
  server-wide `last_activity_at` only orders the Cascades page. So a cascade
  studied daily on the phone takes no question rows and no cards on a tablet
  until the user opens it there, and one created here but studied elsewhere
  leaves this device's window 14 days after its last open here; the Cascades
  page marks every cascade whose question rows this device does not hold
  "not downloaded on this device, open to download", so it can be pulled
  ahead of a flight. A cascade whose active
  quizzes hold more than `AUTO_KEEP_ROWS` question rows (a constant beside the
  window in `lib/sync/config.ts`, 50,000 by default) is treated as kept
  offline from the moment this device has opened it and learned its size, so
  reopening it never means refetching hundreds of thousands of rows; until
  then its toggle reads "kept automatically once opened". This keep holds
  against the **14-day window** only. It never holds against
  `ROW_STORAGE_BUDGET` below, which drops automatically kept cascades once
  nothing else is left to drop, and never against the answer limit; only the
  user's own Keep offline does either. The Cascades page
  shows its Keep offline toggle on, with a hint saying why, and the user can
  turn it off; the opt-out
  is recorded in the `meta` store beside the kept ids, and the window then
  applies to that cascade like any other. The automatic keep is re-evaluated
  from the pulled quiz rows after every pull, so a cascade whose large level
  clears and whose active rows fall below the threshold becomes subject to
  the window again at the next drop pass; its rows come back on open. Cleared
  quizzes do not count toward the threshold, and **a cleared quiz's question
  rows never travel in a pull**, whatever cascade they belong to: the server
  sends question rows only for active quizzes. A cleared quiz keeps whatever
  rows the base already holds, as a quiz cleared on this device does, so its
  Trash export works offline. A restore or an export fetches them from the
  questions and grades endpoints when the base holds none, holds fewer than
  the quiz's `question_count`, holds them without positions, or holds a set
  that fails the `questions_hash` check, so a partial set left by an
  interrupted download can never become a silently incomplete file. Either
  way they are dropped with the cascade like any other rows, and a cleared
  quiz is never flagged pending: a quiz a pull reports cleared has its flag
  cleared, and the fetch above is what makes its rows whole.
- **Question rows follow the same rule.** A pull writes cascade and quiz rows
  for everything and grade rows for every graded question of an active quiz,
  but never a quiz's
  full index list: that comes from
  `GET /api/cascades/:id/quizzes/:quiz_id/questions` (paged, immutable for the
  quiz's life) when the device materialises the quiz. After every pull the
  download manager, judging the window from the `meta` store's last-open times,
  the automatic keep from the pulled quiz rows and the **budget-dropped** ids
  from `meta` too, which count as outside the window however recently they were
  opened (see [the budget](#on-the-device)), fetches the missing index
  lists, then the graded rows of every
  **pending** quiz (always an active one) from the grades endpoint, then the
  missing **question keys** from the card pages with `keys=1`, then the missing
  answers from the same pages in full, for
  cascades inside the window or kept offline, in a worker, materialising each
  quiz's positions once its list and grades are present, behind the same
  download progress the cards show. Keys come before answers, and before any
  other cascade's answers, because they cost far less and are what makes
  a level playable: a user who creates several cascades and boards a plane gets
  every question and its grading, then the answers as the budget allows. **How
  much less they cost is two different ratios**, and both are worth stating
  exactly: a keys page holds ten times as many rows as a card page, so the
  pass is a **tenth of the requests**, which is what the shared rate bucket
  counts; in bytes it is about a **quarter** of anagram cards without
  definitions, since an alphagram is most of a card whose answer is one word,
  and far less once definitions are included. The request ratio is what makes
  several cascades playable quickly; the byte ratio is why it is worth doing at
  all. A
  quiz's pending mark is cleared only by
  that full sequence (index list, graded rows, positions, hash check),
  whichever of the download manager or a first open runs it; an index list
  alone never clears it. **The grades fetch is checked for completeness too**:
  the hash covers the question set, not the grades, so the device compares the
  graded rows it wrote against the quiz row's `correct_count + missed_count`
  before clearing the flag and fetches again when they differ, since a fetch
  that stopped a page early would otherwise leave the level looking ready with
  the user's answers missing, and the next `finish` rejected `ungraded`. For any other cascade the same sequence runs the first time
  the user opens it on this device, behind a short progress indicator, and so
  needs a connection for whatever it has never fetched. A cascade in the
  window or kept offline opens offline, recomputing positions if needed; if
  its cards are gone the question still shows, from the `questions` store, and
  the reveal shows "answer needs a connection", so grading
  still works, with a typed card falling back to flashcard mode (see
  [Typed mode](#typed-mode-anagram-quizzes-only)). **A card whose key the
  device does not hold either**, which only a download that never finished
  leaves, shows "this question needs a connection" and cannot be graded: the
  three actions move past it and emit nothing, so the attempt cannot be
  finished until the keys arrive, which the ordinary "every question is graded"
  rule already enforces. At the last card of a run, or of the attempt, the
  device therefore emits no `finish_segment` and no `finish`: Show / Next does
  nothing and the player says "some questions in this run still need a
  connection", with the count, so the user is never left pressing a dead
  control. The ladder marks such a level as still downloading. Turning **Keep offline** on runs the same sequence for
  that cascade at once, behind the same progress; turning it off makes the
  cascade subject to the window again at the next drop pass. **Cards record
  which of definitions and hooks they hold.** When a preference asks for
  something a cascade's cards lack, the download manager refetches that
  cascade's cards, as full cards with the flags the preferences now ask for,
  on the same schedule as index lists: at once for cascades in the window or
  kept offline, on first open otherwise, behind the same progress. Until they
  arrive the player shows the words without the missing part, with one line
  in the answer area saying definitions are downloading, or need a
  connection. No confirmation is asked for the size; the window is already
  the user's opt-in. The download
  manager evaluates the window, and drops what has left it, after every pull
  and whenever Keep offline changes; a budget-dropped cascade is neither
  fetched nor dropped again, since it already holds nothing. Lists are fetched
  one quiz per call, deepest level first, so the quiz the user will play next
  is ready before the waiting levels above it; new non-source quizzes are rare
  per pull, so no batch endpoint is needed. Grade rows pulled for a quiz in
  the window whose index list has not arrived yet are stored as they arrive
  and get their positions then; for a cascade outside the window, not kept
  offline and holding no rows here they are not sent at all, since the device
  leaves it out of the sync
  request's `question_rows_for`; a cascade whose rows the drop pass spared for
  its pending operations stays in the list, so its rows never go stale
  (see [The sync cycle](#the-sync-cycle)). A
  device's sync therefore costs time proportional to the graded rows of the
  cascades in its `question_rows_for`, none on a fresh device, and never to
  index lists or positions, so any account's first sync takes seconds, and
  **Available offline** means index lists, positions, question keys and cards
  are all present and no quiz is pending.
- The player shows **Available offline** or download progress. While a download
  is still running and the device is online, the player fetches the pages it
  needs on demand, so studying never waits for a download to finish. **That
  fetch takes priority**: the download manager yields while one is outstanding
  and does not start another page of its own, because both spend the one
  `DOWNLOAD_RATE_PER_MINUTE` bucket and a background download must never be
  what makes the card in front of the user read "answer needs a connection". A
  `429` on either is honoured as [Security](#authentication) describes, so the
  card waits for `Retry-After` rather than falling back.
- Answer data above a 500 MB soft limit is evicted, least recently opened on
  this device first. Only the `cards` store is ever evicted, never the
  `questions` store, never unsynced operations, and never
  the cards of a cascade the user marked **Keep offline** or with pending
  operations. Keeping the keys is what lets a cascade whose answers are gone
  still be played and exported: the question is local, only the answer is not,
  and a typed card falls back to flashcard mode (see
  [Typed mode](#typed-mode-anagram-quizzes-only)).
  An automatic keep by size protects a cascade's rows from the **window**
  alone, never its cards and never its rows from the budget, so the limit
  still holds; when its cards are evicted the badge shows
  download progress instead of **Available offline**, which is never shown for
  a cascade whose cards are gone — and **"answers need a connection" while the
  device is offline**, since there is no progress to show until the download
  manager can fetch again, and a bar sitting at nothing would read as a stuck
  download rather than as the state the reveal already explains. When the user-kept cascades alone exceed the
  limit, nothing is evicted. The Account page's storage line lists every kept
  cascade, automatic or not, with its answer size, its much smaller key size
  and its Keep offline toggle — but only while the device actually holds data
  for it, so a cascade the [budget](#on-the-device) dropped leaves the list
  rather than sitting in it as "kept automatically" with nothing behind it,
  which would say the opposite of what the pass just did. Its badge reads
  "kept automatically once opened", exactly as for a cascade this device has
  never opened, and the Cascades page offers "open to download". The line
  gives the device's total rows and keys
  against `ROW_STORAGE_BUDGET`, saying when the user's own Keep offline is what
  holds that total above the budget, since the drop pass will not touch those,
  so the storage problem and its remedy are on one page. That budget and the
  answer limit are **per user**, so a device several accounts have used can hold
  several of each; the line lists every account in the unscoped `accounts` table
  that still has data here — never a row emptied by the removal action and kept
  only for an unacknowledged logout — with the totals that row holds, and offers
  **Remove this account's data from
  this device** for each, **the signed-in one included** — removing the
  signed-in account's data also clears the `signed_in` pointer and returns to the
  login page, since deleting the database under a live session would leave the
  app pointing at nothing. It is the same action the logout dialog offers (see
  [Authentication while offline](#authentication-while-offline)).
- **Keep offline is a property of this device** and never syncs: the set of
  kept cascade ids lives in the `meta` store, per device and user, and a
  second device shows the toggle off, unless the cascade is kept automatically
  by size, until its user turns it on. Syncing it
  would let one phone's choice fill a tablet that never asked.
- **Local size.** A cascade costs one copy of its quiz rows in the base plus the
  overlay's few hundred rows, or, while an offline session's grades are still
  unsent, one overlay row per pending grade: roughly 100–200 bytes per question
  per level in IndexedDB, so a 300,000-question cascade with two levels is on
  the order of 60–120 MB before its question keys and answers, plus about 3 MB
  of keys, and up to half as much again until a
  fully offline attempt has drained. **Cleared levels count too**, since their
  rows stay until the cascade leaves the window, so a Ladder cascade that has
  cleared five 150,000-question levels inside the fortnight holds those rows
  as well; the window is what bounds them, and the automatic keep never
  protects them. The [scale test](#scale-tests) asserts the row count
  and records the bytes. When a cascade leaves the window and is not kept
  offline, the device drops everything it can fetch or recompute again: every
  quiz's question rows, graded or not, their positions, its question keys and
  its cards, keeping
  only the cascade, quiz and attempt rows; nothing is dropped while the cascade
  has pending operations or its player is open in any tab (each player tab
  holds a Web Lock named for its cascade, and the drop pass skips every
  cascade whose lock `navigator.locks.query()` reports held; where Web Locks
  is missing, only the last-open time protects a cascade and the pass skips
  just the cascade the running tab's own player has open). On the next open the index lists come back from the
  questions endpoint, each active quiz's graded rows for its current attempt
  from `GET /api/cascades/:id/quizzes/:quiz_id/grades`, and the positions from
  the seeds, behind the same progress. The device syncs before that fetch,
  and if the fetched `(attempt, shuffle_seed)` still differ from its quiz
  row's it discards the fetch and syncs again; it never builds positions from
  a fetch that does not match its row. A cleared quiz's rows are fetched only
  when the user restores or exports it, on any device, since no pull carries
  them. On the device, `restore_quiz` on a
  quiz with no local question rows writes the overlay quiz row (attempt + 1,
  the seed, cursor 0) and marks it pending; when the restore is acknowledged,
  step 3 of the rebase promotes nothing for it, and if the device is online
  the download manager runs the full sequence for that one quiz at once (index
  list, graded rows, positions, hash check) together with the cascade's missing
  question keys, as it
  does for Keep offline, so the level is playable when the user opens it;
  offline, its first open fetches its list and grades. The restore was an
  open, so the cascade is in the window and the drop pass leaves those rows
  alone. So the window and Keep offline bound local storage: the
  base holds question rows only for cascades in the window or kept offline,
  plus cascade, quiz and attempt rows for the rest.
- **A total budget bounds the rows and keys**, because the window alone does
  not: a hundred cascades of 300,000 questions each, all opened inside a
  fortnight, would be gigabytes, and the 500 MB soft limit governs the
  answers only. `ROW_STORAGE_BUDGET` (2 GB, beside the window in
  `lib/sync/config.ts`) covers the base's question rows, their positions and
  the `questions` store. When the total is over it, the drop pass drops
  cascades in last-opened order, oldest first, until the total is under the
  budget; so a cascade can leave before its 14 days are up, which is the only
  thing that overrides the window. It takes them in **two tiers**: first every
  cascade that is not kept at all, oldest first, and then the ones kept
  **automatically by size**, oldest first. It never drops a
  cascade the **user** marked Keep offline, one that holds pending operations,
  or one with its player open. The automatic keep has to be in the second tier
  rather than skipped, because the budget exists for a device full of large
  cascades and every cascade large enough to fill it is automatically kept: a
  pass that skipped them could never bring forty 300,000-question cascades
  under 2 GB, which is the case the budget was written for. What the automatic
  keep still buys in that case is order — the small unkept cascades go first,
  and a large one is refetched on its next open only when nothing smaller is
  left to drop. When the cascades the user kept offline exceed the budget on
  their own, nothing more is dropped, exactly as nothing is evicted when
  user-kept answers exceed the answer limit, and the Account page's storage
  line says so beside the total, since only turning Keep offline off can
  resolve it. That line shows the total against the budget beside the
  per-cascade sizes.
- **A budget drop is remembered, or it would undo itself.** A cascade the
  budget drops is typically still inside the 14-day window — that is the whole
  point of the budget overriding it: the pass takes the oldest last-open first,
  and on a device that is over budget that can be a cascade opened days ago. The window is judged from the `meta`
  store's last-open times, which a drop does not change, so without a record
  the next pull's download manager would see the cascade in the window and
  refetch its index lists, grades, keys and cards at once, putting the base
  back over the budget for the following pass to drop again — an endless
  drop-and-refetch cycle that would spend `DOWNLOAD_RATE_PER_MINUTE` forever
  and never settle. So the pass **records the id** in the `meta` store, beside
  the Keep offline ids and the automatic-keep opt-outs, and from then on the
  device treats that cascade exactly as it treats one outside the window: no
  index lists, no graded rows, no keys and no cards are fetched for it, it is
  left out of the sync request's `question_rows_for` (it holds no rows and will
  not be opened by itself), and its quizzes stay pending. **Opening it clears
  the mark**, like any other open: the open refreshes its last-open time, its
  rows come back through the ordinary first-open sequence, and it is a
  candidate for the window and the budget again like any cascade. The
  [quota path](#on-the-device) below runs the same pass and so leaves the same
  mark, which is why its retry does not refetch what it just dropped. The mark
  goes with the cascade's other `meta` entries when a tombstone or a full
  pull's deletion removes it.
- **A quota error is handled, not assumed away.** A `QuotaExceededError` from
  any IndexedDB write aborts that unit of work alone. The device then runs the
  **drop pass** as though `ROW_STORAGE_BUDGET` were exceeded **and then the
  answer eviction** as though the 500 MB limit were, in that order — rows and
  keys first because they are the cheaper thing to fetch again, a tenth of the
  requests answers cost — and retries once. Eviction has to run too: the
  `cards` store is usually the largest thing on the device, and a browser whose
  quota is below the two limits together can be over it with nothing for the
  drop pass to take. What the retry means depends on which write failed:
  - **A pull's per-cascade transaction or a download**: the sync cursor is left
    unadvanced, the staging rows are for the next pull to discard and the outbox
    is untouched, so nothing the user did is lost and studying continues on what
    is already local.
  - **An `applyLocally`**: the aborted unit **is** what the user just did. The
    grade, cursor move or finish reached neither the overlay nor the outbox, so
    the player cannot pretend it was saved and cannot advance past the card. It
    stays on that card and shows the storage message where the grade would go,
    with the remedies below, and writes the action when the user frees space —
    the one place in the plan where studying stops, because the alternative is a
    Show / Next that silently does nothing.
  If the retry also
  fails, the sync status reads "not enough room on this device", and the Account
  page's storage line says the same beside a **Keep offline** column the user
  can turn off **and beside the other accounts' totals with their removal
  action**, which is the only remedy when the space belongs to an account that is
  not signed in, since the drop pass and eviction are per user.
- The app calls `navigator.storage.persist()` on first login. The Account page
  notes that some browsers, notably Safari, can clear site data after a period
  of not being used.

### Operations

| Operation | Fields | Server applies it when… | Effect |
|---|---|---|---|
| `grade` | quiz, attempt, attempt seed, question `idx`, grade, at (stored as `graded_at`, as `move_cursor`'s `at` is stored as `cursor_moved_at`; there is no separate `graded_at` field) | the quiz is active, its cascade is not trashed, the attempt and its seed match, the question is in the quiz (else `not found`), and the question is ungraded, or its current grade is from this device, or the device had seen the current grade (`seen_seq`, below), or the operation's `at` is later than the stored `graded_at` | Set the grade and the device that set it, and update counters. A device's own grades apply in `device_seq` order whatever their timestamps; `graded_at` only arbitrates between devices that changed the same question without seeing each other's change |
| `move_cursor` | quiz, attempt, attempt seed, position, at | the quiz is active, its cascade is not trashed, the attempt and its seed match, the position is not below `run_start`, is below `next_boundary(quiz)` when the quiz has one and below the question count otherwise, and the cursor is unset, or was last set by this device, or had been seen by the device (`seen_seq`), or the new `at` is later | Set the cursor, `cursor_moved_at` and the device that moved it. The outbox keeps only the latest pending cursor per quiz, re-appended with a fresh `device_seq` (see [The sync cycle](#the-sync-cycle)). A rejected cursor move (`bad_cursor`) is logged but never counted in a notice; it is not the user's work |
| `finish` | quiz, attempt, attempt seed, shuffle seed, new quiz id | the quiz is active, its cascade is not trashed, it is at the deepest level, the attempt and its seed match, every question is graded, and `new_quiz_id` is not already a quiz (`invalid`) | Apply [Cascade Rules](#cascade-rules) using the server's grades and the quiz's own progression; any new quiz uses the device's id; record the attempt. A reset or new quiz starts at cursor 0, stamped with the operation's `at` and device. The result carries the outcome the server reached and the new quiz's question count and `questions_hash`, so the device can see when the server's grades led somewhere else (see [Conflicts](#conflicts)) |
| `finish_segment` | quiz, attempt, attempt seed, segment end, shuffle seed, new quiz id | the quiz is active, its cascade is not trashed, it is at the deepest level, the attempt and its seed match, its segment size is greater than 0, the segment end is a multiple of it strictly between 0 and the question count and greater than the cursor, every question before the segment end is graded, no quiz already exists for this quiz, attempt and segment end, and `new_quiz_id` is not already a quiz (`invalid`) | Create the drill quiz one level down from the misses in positions `run_start` … segment end − 1 (nothing if there are none), and set the cursor and `run_start` to the segment end, stamping `cursor_moved_at` and `cursor_device_id` from the operation. See [Segments](#segments) |
| `set_cascade_options` | cascade, changed option fields, at | the cascade exists, is neither trashed nor purged, every carried value is in range (segment size 0 or 5 … `MAX_QUIZ_QUESTIONS`, progression one of the two, alphabetical order a flag; otherwise `invalid`, so an out-of-range value is a rejection and never a constraint violation recorded as `error`), and either its options were last changed by this device (`options_device_id`), or the device had seen its current options (`seen_seq` against `options_seq`), or `at` is later than its `options_changed_at`; otherwise rejected as `stale` | Set the carried fields, `options_changed_at` and `options_device_id`, as one unit. Only later quizzes are affected |
| `set_quiz_options` | quiz, changed option fields, at | the quiz is active, its cascade is not trashed, every carried value is in range as for `set_cascade_options`, `progression` and `segment_size` are not carried for a `segment_chain` quiz and `progression` is not carried for the Source quiz, which its rules ignore (`invalid`), and either its options were last changed by this device (`options_device_id`), or the device had seen its current options (`seen_seq` against `options_seq`), or `at` is later than its `options_changed_at`; otherwise rejected as `stale` | Set the carried fields, `options_changed_at` and `options_device_id`, as one unit |
| `restore_quiz` | quiz, shuffle seed | the quiz is cleared and not purged | Push it back as the new deepest level with a new attempt shuffled by the seed, cursor 0 stamped from the operation; bring back its cascade if it was trashed, and only then set `cleared_at` to now on its other cleared quizzes |
| `trash_cascade` | cascade | the cascade is not trashed (`trashed`) | Trash it |
| `restore_cascade` | cascade | the cascade is trashed (`not_trashed`) and not purged | Restore it as it was, setting `cleared_at` to now on its cleared quizzes |
| `purge_quiz` | quiz | the quiz is cleared (`not_cleared`) and its cascade is not trashed (`trashed`) | Delete it permanently and record a tombstone |
| `purge_cascade` | cascade | the cascade is trashed (`not_trashed`) | Delete it and all its quizzes permanently and record tombstones |
| `set_preferences` | changed fields, at | every carried value is in the range [Preferences](#preferences) gives it (clear threshold 1–100, leave value decimals 0–3, segment size 0 or 5 … `MAX_QUIZ_QUESTIONS`, progression one of the two, the answer mode one of the two, and the remaining preferences flags; otherwise `invalid`, checked before any write exactly as for the options operations, so an out-of-range value is never a constraint violation recorded as `error`), and the preferences were last changed by this device (`changed_by_device_id`), or the device had seen the current preferences (`seen_seq`), or `at` is later than the row's `changed_at`; otherwise rejected as `stale` | Set the carried fields, `changed_at` and `changed_by_device_id`, as one unit |
| `set_bindings` | the full list of bindings, at | the list is valid (`invalid`, before any write): every action has 1–3 bindings, no stroke is used twice, and every binding is one the schema allows — a mouse button named `left`, `middle`, `right`, `back` or `forward`, a wheel direction `up` or `down`, or a key whose `code` matches the column's pattern and is not `Escape`; and the bindings were last changed by this device (`bindings_device_id`), or the device had seen the current bindings (`seen_seq`), or `at` is later than `bindings_changed_at`, otherwise `stale` | Replace the bindings as one unit and set `bindings_device_id` |

Every operation also carries its `id`, `device_seq`, the device's
timestamp, and `seen_seq`: the device's sync cursor when the operation was
created, or 0 when the device has never synced. `device_id` is sent once per
sync request and applies to every
operation in it; an operation-level `device_id` is a `400`.

**The cursor is the position of the card being shown.** Show / Next on the
card at position `p` saves its grade and emits `move_cursor` to `p + 1`, and
Previous emits `move_cursor` to `p − 1`, never below `run_start`. From the last
card of a run that is not the last run the device emits the card's `grade` and
then only `finish_segment`, and from the last card of the attempt the card's
`grade` and then only `finish`, with no cursor move; those two are the only
operations that move the cursor onto or past a run boundary, and neither leaves
it at the question count. So the cursor is always below the question count, a
device that follows this never has a cursor move rejected, and the rail's
`37 / 250` is the cursor plus one.

**`seen_seq` decides what "latest" means.** A pull delivers every row with
`updated_seq` up to the device's cursor, so `seen_seq` says exactly which version
of a row the device had in front of it. When a grade, cursor move, options change
or preferences change arrives for a row whose `updated_seq` is at most the
operation's `seen_seq`, the device saw the current value and changed it
knowingly, and the change applies whatever the timestamps say. Only when the row
has moved on since the device last pulled, the true concurrent case, does the
server fall back to comparing timestamps. Device clocks therefore never decide
between a change and the change a user made after seeing it. For grades the
comparison is against the question row's `updated_seq`, which moves only when
the grade does. For options it is against the cascade or quiz row's
`options_seq`, the sequence of its last options change, because those rows'
`updated_seq` also moves with every grade counter and cursor move and would
make the rule fall back to timestamps far too often. For preferences and
bindings the preferences row's `updated_seq` serves, since it moves only for
them.

**Device clocks are clamped.** Wherever a timestamp is compared or stored for
these rules (`graded_at`, `cursor_moved_at`, `options_changed_at`,
`changed_at`, `bindings_changed_at`, and `finished_at`, which is clamped
although nothing compares it), the server uses
`least(at, now() + 5 minutes)`, so a device whose clock is a year ahead can
win a concurrent comparison by at most five minutes and its later changes
lose to real later ones. The operation itself is still applied; only the
timestamp is capped. A clock that is behind is left alone, since an offline
device's timestamps are legitimately old. **A device's own changes never
compete on timestamps**: grades, cursor moves, options, preferences and
bindings each record the device that last changed them, and a later change
from that same device applies in `device_seq` order whatever its `at`, so
two option changes from a fast clock in one batch are both applied rather
than the second losing to the first's clamped stamp.

**Rejection reasons are a fixed vocabulary**, because monitoring counts them:
`not_found` (the target belongs to another user, was purged, **never existed** —
the quiz an operation rejected earlier in the same batch was to create — or names
a question not in the quiz; which of those it was decides the notice, see
[Conflicts](#conflicts)), `trashed`, `not_active` (the quiz is cleared), `not_deepest`,
`stale_attempt` (attempt number or seed mismatch), `ungraded`, `bad_segment` (no
segment size, not a multiple, not past the cursor, or past the end),
`duplicate_segment`, `stale` (a grade, cursor move, options, preferences or
bindings change that lost the `seen_seq` and timestamp comparison to another
device's),
`not_cleared` (restore or purge of a quiz that is not cleared), `not_trashed`
(restore or purge of a cascade that is not trashed), `bad_cursor`
(below `run_start`, or at or past the next boundary or the question count),
`invalid` (a malformed operation, including a `seen_seq` above the user's
`sync_seq`) and `error` (a database error that is not transient, such as a
constraint violation, inside the operation's savepoint; see
[The sync cycle](#the-sync-cycle)). Each condition in the table maps to one of
these. A rejection with reason `error` is handled on the device like any other
rejection, and when it is a `finish` or `finish_segment` the notice's first
sentence is "Level N could not be saved on the server."; `invalid` on one of
those takes the same sentence; `error` or `invalid` on a `restore_quiz` reads
"This quiz could not be restored on the server."; and `error` on a grade takes
the grade sentence under [Conflicts](#conflicts).

**The attempt seed is what ties operations together.** A `grade`, `move_cursor`,
`finish` or `finish_segment` names the `shuffle_seed` of the attempt it was
played under. Two devices can both produce "attempt 4" of the same quiz, each
from its own finish or restore, but never with the same seed, so the server
rejects an operation from the losing device's attempt as `stale_attempt`
instead of applying its grades to a different shuffle. On the device the same
rule defines "depends on": during the replay after a pull, an outbox operation
is dropped when its `(quiz, attempt seed)` is held neither by the base nor by an
earlier operation replayed in the same pass, and likewise an operation on a quiz
that neither the base nor an earlier replayed operation created. The check is
made in replay order, never as a filter against the base alone, because a
`finish` still waiting in the outbox legitimately owns the attempt seeds of the
grades queued behind it. The dropped grades are what the notices count.

**A trashed cascade is frozen.** Every operation on it or on its quizzes other
than `restore_cascade`, `restore_quiz` and `purge_cascade` is rejected with the
reason `trashed`; that includes `purge_quiz`, since a trashed cascade is purged
as one unit. The player does not open a trashed cascade; it shows the
ladder as it was and offers **Restore**.

### The sync cycle

`POST /api/sync` does a **push** and then a **pull**, in one request.

**Push.** The request itself is checked before anything is locked or applied: a
body carrying more than `SYNC_MAX_OPS` operations, a `page_token` together with
any operations, a `question_rows_for` longer than `MAX_CASCADES_PER_USER`, a
non-numeric `app_version`, or an operation-level `device_id` is a `400` with
nothing written. An operation whose `device_seq` is below 1, or already recorded
for this device under a different `id`, is rejected `invalid` like any other
malformed operation, so a device bug never reaches the
`(user_id, device_id, device_seq)` constraint and is never recorded as `error`,
which alarms. Then:
1. The server locks the user row and takes a new value of the user's sync
   sequence: `UPDATE users SET sync_seq = sync_seq + 1 RETURNING sync_seq`. The
   lock also means two syncs for the same user never run concurrently. Every row
   this request changes is stamped with that value.
2. Operations are applied in `device_seq` order, one savepoint each. An
   operation whose `id` has already been recorded **for this user** returns its
   recorded result without being applied again, the same result in every field,
   so a `finish` whose first response was lost is acknowledged on its second
   send with the same outcome, count and hash and promotes its rows exactly as
   it would have the first time; an `id` recorded for another
   user is rejected as `not found`. **Records are kept only as long as a resend
   could need them.** A device drops a batch from its outbox only after it has
   received that batch's response, and sends in `device_seq` order, so the
   lowest `device_seq` in a request proves that every earlier operation of that
   device was acknowledged. The server keeps that figure per device in
   `sync_devices.acked_below`, raising it with each request that carries
   operations (a retried batch after a lost response starts at the same
   `device_seq`, so it raises nothing). In the same transaction it deletes that
   device's records **below the mark whose operations return no result
   fields** — every kind but `finish`, `finish_segment` and `restore_quiz`, for
   which a repeat needs only "applied" or the rejection's reason. The three
   result-bearing kinds are kept for `SYNC_RETENTION_DAYS`, since their
   outcome, count and hash must come back unchanged, and there are a few per
   attempt. An operation that arrives **below its device's mark with no
   record** — only a stale resend can, from a second tab that read the outbox
   before the rebase dropped it where Web Locks are missing — is answered
   `applied` and **not applied again**, so a replayed grade can never overwrite
   the newer grade the same device made after it. Records past
   `SYNC_RETENTION_DAYS` go too, so an operation older than that, which only a
   device offline that long can resend, is applied afresh; that is almost
   always a `stale_attempt` or `not_cleared` rejection, and such a device is
   also past the `426` line, so it reloads before it resends.
3. Each operation is recorded as `applied` or `rejected`, with a reason and,
   for a `finish` or `finish_segment`, its `outcome`, and for those and a
   `restore_quiz` its `new_quiz_question_count` and
   `new_quiz_questions_hash`. One
   rejection does not stop the rest of the batch.
4. A savepoint that fails with a database error is rolled back. A **transient**
   error (serialization failure, deadlock detected, lock or statement timeout,
   lost connection) aborts the whole request with `503` and `Retry-After` and
   records nothing, so the device sends the same batch again and every
   operation is applied then. Any **other** database error, such as a
   constraint violation from a rules bug, records that one operation as
   `rejected` with reason `error`, logs the operation in full, and the batch
   continues. `error` should never occur, so monitoring alarms on any.

**Pull.**
1. The server returns every row belonging to the user with `updated_seq` greater
   than the device's cursor and at most the sequence this request's push took,
   in this table order: cascades, quizzes, quiz attempts, quiz questions,
   preferences with bindings, then tombstones for purged items, so a quiz row
   always precedes its questions. A quiz question row is sent exactly when its
   `updated_seq` is past the cursor, **it is graded, its quiz is active, and
   its cascade is in the request's `question_rows_for`** (the cascades in the
   device's window and not budget-dropped, kept offline, or whose base holds
   question rows; with an
   empty list a pull carries no
   question rows at all, and the quizzes left out stay pending on the device;
   a cleared quiz's rows never travel, see [Downloads](#on-the-device)); a quiz's full index
   list never travels in a pull, however new the quiz is (the device fetches it
   on demand, see [Downloads](#on-the-device), and a Source quiz's is implied).
   A row carries only the question index and grade; positions are never sent,
   because the device derives them from the quiz's `shuffle_seed`.
   Within a table rows are ordered by primary key, and a page runs up to
   **50,000 rows of any kind**, counting cascades, quizzes, attempts, question
   rows and tombstones alike, with an opaque page token that encodes the table
   and the primary key it stopped at, the
   request's `question_rows_for`, and **the sequence the first page's push
   took**, which every later page uses as its ceiling. Bounding the total
   rather than the question rows alone is what keeps a pull that carries no
   question rows in hand: a cleared quiz's rows never travel, so an attempt
   studied with a segment size of 5 on a 300,000-question quiz can leave
   60,000 chain quizzes in the Trash (see [Quiz options](#quiz-options)) and,
   a retention period later, 60,000 tombstones, each of which a question-row
   bound would have delivered as one unpaged response. Tables are still sent
   in their order, so `quizzes` is exhausted before `quiz_questions` begins
   and a quiz row still precedes its own question rows however the pages
   fall. A
   request with a page token carries no operations and takes no new sequence,
   and the token's list stands for the rest of the pull; a page
   request's own list is ignored. Fixing the ceiling in the token is what
   makes a paged pull one snapshot.
2. When the last page arrives, the device's cursor is set to the sync sequence
   the push took. Rows another device commits between two pages have a higher
   sequence and arrive on the next sync.
3. If the device's cursor is below the user's `sync_floor_seq` (the highest
   tombstone sequence the purge task has pruned; tombstones are pruned after
   90 days), the server replies `resync_required`; a cursor at or above the
   floor has missed no pruned tombstone, even when older tombstones remain
   above the floor. That response carries the `results` of the push that
   arrived with it and no `changes`, so the device drops the acknowledged
   operations as usual and then makes a **full pull**: a sync request with
   `cursor: null`, pushed exactly like any other sync (it may carry
   operations, and usually does on a first sync, since a device can create a
   cascade and study it before it has ever synced), which the server answers
   with every row of the user that still exists, trashed cascades and cleared
   quizzes included, in the usual pages and with no tombstones, and to which
   it never answers `resync_required`. On the last page the device applies the pull
   exactly as steps 2 to 5 below, with one addition: every base row of a
   cascade, quiz, attempt or graded question that the full pull did not carry
   **and whose `updated_seq` is at most the sequence the full pull took** is
   deleted, which is what tombstones would have done. **Every base row carries
   the sequence of the response that wrote it** — a pull's `sync_seq`, a
   promotion's, a creation's, or, for rows fetched from the questions or
   grades endpoints, that of the sync the device made first — which is what
   this comparison reads, because a question row's own `updated_seq` is never
   sent. So rows an earlier pull wrote carry a lower sequence and go, while a
   cascade the device
   created through `POST /api/cascades` while the pull was in flight carries a
   higher sequence and is left alone. A cascade the deletion removes loses its
   `questions` and `cards` entries in the same transaction as its rows, as a
   tombstone does, since nothing else would ever reclaim them: the drop pass
   only sees cascades the base still holds, and eviction orders answers by a
   last-open time an orphan has none of. The same transaction also removes that
   cascade's entries from the `meta` store — its last-open time, its
   **Keep offline** id, its automatic-keep opt-out and its budget-dropped
   mark — since nothing else ever
   would, and the Account page's storage line would otherwise list a cascade
   that no longer exists. The deletion of graded question rows
   applies only to the **active** quizzes of cascades in the request's
   `question_rows_for`, since the pull carries no row for a cleared quiz and
   none for a cascade left out of the list, which by definition holds no rows
   to delete; the drop pass governs every other cascade's rows. Nothing else is
   discarded: the ungraded question rows and positions of a quiz whose
   `(attempt, shuffle_seed)` is unchanged are kept, as after any pull, so a
   resync never refetches an index list or recomputes an order it already
   has. The device then sets its cursor to the sequence that request took,
   lowers every pending operation's `seen_seq` to that cursor where it is
   higher, and its outbox survives. A device that has never
   synced sends `cursor: null` on its first sync for the same reason: a
   cursor of 0 on an account whose tombstones have been pruned is below the
   floor, and would otherwise be told to resync forever. A cursor **above** the user's
   `sync_seq`, which only a database restored to an earlier point can produce,
   gets the same reply before any operation is applied, so nothing is rejected
   as `invalid` for a `seen_seq` the server no longer recognises; the
   [restore procedure](#restoring) makes this case unreachable in practice.

**Applying a pull on the device** works like a rebase, run once after the last
page of the pull has arrived. Pages are written as they come, to one of two
places. Question rows go straight to the base when the player could not be
using an order they might disturb: for a quiz in the window or kept offline
whose `pending` flag is not set (the grades endpoint brings a pending quiz
everything it needs, so nothing is written for it here), grade rows for an
unchanged attempt of a quiz that is also not flagged pending, and every row of
a quiz that has no base row at all (new to
the device, so no positions exist yet). Everything else
(cascade rows, quiz rows, attempt rows, preferences, tombstones, and the
question rows of a quiz whose base row exists with a different
`(attempt, shuffle_seed)`) goes to a **staging** store. So the bulk of a large
pull, on a new device as on an old one, is written once. Positions for every quiz
that needs a rebuild are computed from the staging rows in a worker first,
outside any transaction; then, per cascade, one IndexedDB transaction moves
the staging rows and their positions into the base, deletes and promotes
overlay rows, and replays the outbox (steps 2 to 4 below). The move judges
the window from the `meta` store's last-open times and its budget-dropped ids,
and the automatic keep from
the **pulled** quiz rows, never the base's, and sets the `pending` flag on the
active quizzes of a cascade the device left out of `question_rows_for`, whose
question rows the pull therefore did not carry, **and whose rows the base does
not hold**; a quiz whose rows are complete is never flagged, since the cascade
it belongs to was in the list and its rows arrived. The player reads only base and
overlay, never staging, so it never sees a quiz row from one page beside
positions from an older attempt. An interrupted pull, or a browser closed
between two cascades' transactions, leaves the cursor unadvanced and staging
rows the next pull discards, and re-pulling from the old cursor converges:
1. Drop every outbox operation the server has now acknowledged, whether applied
   or rejected, and note which cascades they touched.
2. Write the server's rows into the **base** stores. Two different things can
   follow. A **rebuild** happens when a pulled quiz's `(attempt, shuffle_seed)`
   differs from the base's, or the base does not have the quiz: its question
   rows are created or reset, grades cleared, positions computed from the seed
   in a worker, and the pulled grades applied on top. A quiz whose `pending`
   flag is set stays pending whatever rows the base holds, and the cases below
   apply only to quizzes without the flag. When the attempt and seed
   match, what happens depends on what the base holds: rows with positions
   need nothing; rows without positions (a quiz whose index list has just
   arrived) get a **materialisation**, which computes positions over the
   existing rows and touches nothing else, so retained grades survive; and no
   rows at all leave the quiz **pending**, unless its pulled row shows
   `correct_count + missed_count = 0`, in which case only the index list is
   needed and no grades fetch is made. Otherwise (a cascade whose rows the
   drop pass removed, or one the staging move skipped) its `pending` flag is
   set: on first
   open, or in the download manager's post-pull sequence when the cascade is
   in the window, the device syncs,
   fetches the index list, then the quiz's graded rows from the grades
   endpoint, computes positions and hash-checks, as
   [Downloads](#on-the-device) describes. A pull's grade rows for a pending
   quiz are never used to rebuild it. A rebuild or materialisation runs only once the quiz's rows are complete: at once
   for a Source quiz, whose rows are idx 0 … count − 1 and are created locally;
   for any other quiz once its index list has been fetched, or at once when its
   graded rows already number `question_count`; and at the last page of the pull for
   a reset, which sends only the rows graded since. A quiz the pull reports
   as cleared takes neither path, and its `pending` flag is cleared if it was
   set: whatever rows
   and positions the base holds for it stay as they are until the cascade
   leaves the window, so a level finished on this device can still be exported
   from the Trash offline, and a device holding none of its rows, or only part
   of them, fetches them when a restore or an export needs them (see
   [Downloads](#on-the-device)). So the player never sees a
   quiz with a new order and no grades. A quiz that an operation applied in
   this batch created or reset takes neither path here **when the pulled quiz
   row's `(attempt, shuffle_seed)` equals that of the rows the device built
   for it in the overlay**: its rows arrive by promotion in step 3, and it
   counts as complete once they have. When they differ, because the server's
   outcome did something else to the finished quiz than the device did (the
   device cleared it and the server reset it, or the reverse), the quiz takes
   the ordinary path above like any pulled quiz whose seed differs from the
   base's or whose grades the base already holds in full. After any
   rebuild, materialisation or promotion the device hashes its rows and
   compares the result with the quiz row's `questions_hash`; a mismatch on the
   quiz being played is handled like an outcome mismatch (rebase, refetch,
   notice), and on any other quiz by refetching the index list. For a cascade
   outside the download window the work is recorded as pending and done on
   first open instead (see [Downloads](#on-the-device)).
3. Delete the **overlay** rows of every cascade that an acknowledged operation
   or the pull touched, with one exception: the question rows and positions the
   device built for a quiz that an **applied** `finish`, `finish_segment` or
   `restore_quiz` created or reset are moved into the base rather than deleted.
   For a quiz the operation **reset or restored**, the condition is the one
   step 2 used: the pulled quiz row's `(attempt, shuffle_seed)` equals the
   device's rows' (its question set is fixed for life, so its hash needs no
   check). For a quiz the operation **created**, the pulled quiz row must
   exist and the result's `new_quiz_questions_hash` must equal the hash of the
   device's rows, which it does whenever the server built the quiz from the
   same grades; a matching count is not enough, since two miss sets of the
   same size can differ. So a finish never ends with a fetch, and an offline
   finish's new level is playable the moment it is acknowledged. Promoted rows
   are stamped with the response's `sync_seq`, like the quiz row they belong
   to, so a full pull in flight never deletes them. When the
   hashes differ, the rows are dropped, the index list fetched, and the
   outcome-mismatch notice shown, since grades on the missing questions were
   not kept. When the server's outcome carries no new quiz at all (a
   `finish_segment` the device computed as `drilled` that the server applied as
   `continued`, or a `finish` the server answered `cleared` with no misses,
   `completed` or `reshuffled`), the device's rows are dropped
   with no fetch, since there is nothing to fetch, and the notice is shown.
   Anything else that existed only in the overlay, such as
   a quiz created by a `finish` the server rejected, disappears here, and rows
   the server has now confirmed are read from the base from now on.
4. Replay the operations still waiting in the outbox into the overlay, in
   `device_seq` order, using the same cascade rules, and recompute the
   preferences view from its base row and the pending preference and binding
   operations; that recomputation happens whenever a preferences row was
   pulled or a preference or binding operation was acknowledged, whichever
   path the cascades take below. An operation is dropped and reported (see below) when its
   attempt seed or quiz is held neither by the base nor by an earlier operation
   in this replay, or when it no longer applies. A replay drop takes the
   reason the rules would have given on the server, since the replay runs the
   same checks, and every operation dropped for one cause makes one notice,
   counted together. Steps 2 to 4 run in one
   IndexedDB transaction per cascade that includes the outbox store, and the
   operations to replay are read inside it, so the player never observes an
   empty overlay, and an `applyLocally` from another tab during the rebase
   waits for it and then lands on the rebuilt overlay.

   **The fast path.** For a cascade where every acknowledged operation was
   applied, none of them created, reset or restored a quiz, and **every pulled
   row of that cascade carries `updated_seq` equal to the response's
   `sync_seq`**, the sequence this push took — read from that column on its
   cascade, quiz and attempt rows, and from each question group's
   `min_updated_seq`, folded across every page of the pull, on its question
   rows, since a question row's own
   `updated_seq` never travels — steps 3 and 4 reduce to deleting
   the overlay rows that no remaining outbox operation touches: every other
   overlay row already reflects the pending operations in order, and nothing
   that could change their outcome has arrived. A row another device changed
   that this push then stamped again passes that test, and that is harmless:
   any pending operation that conflicted with the foreign change was rejected,
   which forces the full path, and one that did not conflict still applies as
   the overlay shows it. A row stamped by another device alone carries a lower
   sequence and forces the full path, and a tombstone for a quiz or cascade
   counts as a pulled row of that cascade for this test, with its `seq` as the
   sequence. A pending `finish` at the end of the outbox is not re-validated on
   such a batch; it is when the full path next runs. This is what keeps a
   large offline session's drain linear: each of the 600 batches of a
   300,000-grade attempt deletes its own 500 overlay rows instead of clearing
   and replaying the other 299,500. The full path runs for any other batch: a
   rejection, a created or reset quiz, or a row from another device.
5. Refresh the player if what it was showing changed.

**When the device syncs:**
- half a second after a new operation, if online, and no more than once a
  second while operations keep arriving, which keeps a fast session under
  `SYNC_RATE_PER_MINUTE`
- on the browser's `online` event
- when the tab becomes visible again
- every 30 seconds while the app is open
- right after logging in
- and, while the outbox still holds more than one batch, again as soon as the
  previous response arrives, subject to `429` backoff, so a long offline
  session drains in minutes rather than one batch per interval

Pushes are sent in batches of up to 500 operations. A batch with a large
`finish` still fits easily, because shuffle seeds replace orderings. The outbox
keeps only the latest pending `move_cursor` per quiz: a new one removes the
pending one and is appended with a fresh `device_seq`, never written into the
old slot, so it can never overtake a `finish_segment` queued after the old one.
`device_seq` therefore has gaps, which the server allows; only the order
matters. So a 300,000-question
attempt studied offline is about 300,000 operations, or 600 requests, and the
per-user sync rate limit (`SYNC_RATE_PER_MINUTE`) is set so that drains in
minutes, not hours, and the rebase's fast path (above) keeps each
acknowledgement's work proportional to its own batch; the
[scale test](#scale-tests) asserts the budget. `429`
backoff applies to sync like any other request, and so does `503`: the device
resends the same batch after `Retry-After`, and nothing is dropped, because
nothing was recorded.

### Conflicts

Conflicts need **two devices changing the same cascade while at least one is
offline**. Using one device on a plane never conflicts. The rules:

| Situation | Result |
|---|---|
| The same question graded on two devices in the same attempt | If one device had seen the other's grade (`seen_seq`), its change wins as a deliberate regrade. Otherwise the later `graded_at` wins. A grade that loses is rejected as `stale` and counted in a notice on the losing device. |
| The same quiz finished on two devices | The first `finish` the server receives wins. The other device's `finish` is rejected as `stale_attempt` when the first finish reset the quiz, or `not_active` when it cleared it. Its operations on the attempt seeds it created (the reset quiz's next attempt, the level it created locally) are dropped by the rebase, because the server has neither seed. The device shows: "Level 3 was finished on another device. 42 answers from this device weren't kept." |
| A quiz restored on one device and purged on another | Whichever operation arrives first wins; the other is rejected. |
| The same quiz restored on two devices | The first `restore_quiz` wins. The second is rejected as `not_cleared`, and its grades name an attempt seed the server never had, so the rebase drops them and counts them in the notice. |
| Grades arriving for a quiz that has since been cleared or reset | Rejected as `not_active` (cleared) or `stale_attempt` (reset). They belong to an attempt that no longer exists, and the losing device counts them in the notice for whatever finished it. |
| The same run finished on two devices | The first `finish_segment` wins. The second is rejected, as a duplicate for that quiz, attempt and segment end when the run missed something, or — when it missed nothing, so no drill quiz exists to be a duplicate of — because its segment end is not past the cursor; both take the run-finished-elsewhere notice below. The device rebases onto the drill quiz the first one created, if there is one. The loser's grades on its own drill quiz are dropped, since the server never saw that quiz, and counted in the notice. |
| A question of an already-passed run regraded on another device | The grade applies: a `grade` needs only an active quiz and a matching attempt seed, and never checks the position against `run_start` (see [Segments](#segments)). It changes that attempt's counters and nothing else, because the run's drill quiz already exists and `origin_segment_end` is unique per parent attempt, so the run cannot descend again. The regrading device's own `move_cursor` inside the passed run is rejected as `bad_cursor`, which is never counted, and its `finish_segment` for that boundary as `duplicate_segment`, which carries the run notice. A question graded correct at Level N while it is being drilled at Level N + 1 is the ordinary case of the same question on several levels. |
| A cascade trashed on one device while the other keeps studying it | The trash wins once it reaches the server. The other device's grades, cursor moves, finishes and option changes on that cascade are rejected as `trashed` and dropped in the rebase, with a notice: "This cascade was moved to the Trash on another device. 17 answers from this device weren't kept." Restoring it brings back what the server has. |
| A cascade trashed on one device while another restores one of its quizzes | Whichever arrives first applies, and the other applies on top, since `restore_quiz` is allowed on a trashed cascade: a restore after the trash brings the cascade back with the restored quiz as its deepest level; a trash after the restore puts the restored quiz in the Trash with the cascade. The device that trashed it sees the cascade back on its next pull, with no notice, because nothing of its own was dropped. |
| The same trash, restore or purge sent from two devices | The first applies. The second is rejected (`trashed`, `not_trashed`, `not_cleared` or `not_found`) with no notice, because nothing that was the user's work was dropped. |
| A cascade purged on one device while the other, offline throughout, kept studying it | The tombstone wins. The other device's operations on it are rejected as `not_found` and dropped, the tombstone removes the cascade with its question keys and answers, and the notice reads "This cascade was deleted, on another device or by the Trash's retention period. 200 answers from this device weren't kept.", with no Restore hint, since there is nothing to restore. |
| The same quiz finished on one device and a run of it finished on another | Whichever arrives first wins. A `finish` after a `finish_segment` is rejected because the quiz is no longer the deepest level; a `finish_segment` after a `finish` is rejected because its attempt is out of date. The loser's dependent operations are dropped. |
| A quiz restored on one device while the other finishes the deepest level | Whichever arrives first wins. If the restore lands first, the `finish` is rejected because its quiz is no longer the deepest level. If the finish lands first, the restore still applies, on top of the new deepest level. |
| Two different quizzes restored on two devices | Both apply, in arrival order; the second becomes the deeper of the two. A `grade` needs only an active quiz, never the deepest one, so the first device's grades on its restored quiz are all kept, and its `finish` is rejected as `not_deepest` until the deeper level, the one restored second, is cleared. Nothing of its work is dropped, so no answers are counted; the notice is the `not_deepest` one below. |
| Quiz options changed on one device while the quiz was finished on another | Rejected if the quiz is no longer active, with no notice, since an option on a finished quiz has nothing left to apply to; the rebase drops the overlay row. Otherwise the change applies to the reset quiz. |
| Quiz or cascade options changed on two devices | **The later operation wins whole**, like preferences: unless the device had seen the row's current options (`seen_seq`), an operation whose `at` is older than the row's last change is rejected as `stale`, even when it carries a different field, and the rebase reverts that field on the losing device. One timestamp per row is enough for this, and the loser simply sets the option again. |
| A quiz created while the cascade's options were different | Nothing happens to it. Options are copied at creation and never revisited. |
| A `finish` applied with a different outcome than the device computed | Another device's grade changed the score, or an options change on the finished quiz changed its progression, so the server cleared where the device descended, or the reverse, or built the new quiz from a different set of misses. The result's `new_quiz_questions_hash` is what tells the device whether the server's new quiz holds the same questions as its own. The new quiz has the same id and seed on both sides, so the device's grades on it stand wherever the question exists in the server's copy; a grade for a question the server's copy lacks is rejected as `not found`. When the two sides disagree about the finished quiz itself (one cleared it, the other reset it), the device's rows for it are not promoted; the quiz is rebuilt from the pulled row like any quiz whose seed differs from the base's, or, when the server cleared it, the base's rows and positions stay as they are, as for any cleared quiz. The device rebases the cascade and, when the outcome kind (cleared, replaced, descended, completed, reshuffled) or the level structure differs from what it showed, tells the user: "Level 2 came out differently on the server because of changes made on another device." When only the new quiz's question count differs, nothing is said. **This path never opens the completion screen**, even when the server's outcome is `completed`: that screen reports `peak_depth` and `attempts_since_completion`, which the server's own `Completed` has already reset to 1 and 0, and the result carries neither, so the device has no honest numbers to show — the pulled row's would read "after 1 level and 0 attempts". The notice stands, and the Complete badge appears from the pulled cascade row like any other. The screen is shown only for a `Completed` this device computed and the server confirmed, where the device's own rule run reported both numbers before resetting them. Rejected grades, if any, are counted in the usual "weren't kept" notice, separately. |
| A quiz's segment size changed on one device while another device passed a boundary | The `finish_segment` is rejected if its segment end is no longer a multiple of the size the server has. The drill quiz it created locally and the grades on it are dropped, with the usual notice. |
| The cascade's options changed on one device while another, offline with the old options, finished a quiz or restored one | The server builds the new or restored quiz from the options it has, so the device's local copy of that quiz may carry different options. The rebase replaces it with the server's row, and any `finish_segment` the device sent against a segment size the server's copy does not have is rejected and dropped. |
| The cursor moved on two devices in the same attempt | A device that had seen the other's cursor (`seen_seq`) wins; otherwise the later `cursor_moved_at`. A device's own moves always apply in order. If the winning cursor is already past a boundary the loser then tries to pass, the loser's `finish_segment` is rejected and its drill quiz dropped, with the usual notice. |
| Preferences changed on two devices | The later operation wins whole, as for options: unless the device had seen the current preferences (`seen_seq`), an older operation is rejected as `stale` whatever fields it carries, and the losing device's view reverts to the server's row as soon as the rejected operation is dropped from the outbox. Bindings are a separate unit with their own timestamp. |

Rejected operations are never retried. After the rebase, the device shows what
the server has, and a notice appears only when the user's own work was dropped:
grades, and the finishes and quizzes built from them. Rejected cursor moves and
option changes are never counted, and a grade the server kept is never counted
either, whatever happened to the finish after it. The notice's first sentence
follows the rejected or replay-dropped operation and its reason. For a `finish` or
`finish_segment`: `stale_attempt` or `not_active` "Level N was finished on
another device."; `not_deepest` "Another level was added below Level N on another device.
Finish it first."; `trashed` "This cascade was moved to the Trash on another
device."; `not_found` "This cascade was deleted, on another device or by the
Trash's retention period."; `duplicate_segment` "This run of Level N was
finished on another device.", **and so does a `bad_segment` whose cause was
that the segment end is not past the cursor**, because that is the same
situation seen through a different check: a run that missed nothing creates no
drill quiz, so `quizzes_one_per_segment` has nothing to catch a second device's
`finish_segment` with and the cursor test catches it instead — telling that user
the size or place "was changed" would name a change nobody made; `bad_segment`
keeps the sentence below for its other causes (no segment size, not a multiple
of it, or past the question count) "Level N's
segment size or place was changed on another device."; `ungraded` "Level N has
answers the server did not get. Finish it again."; `invalid` and `error` as under
[Operations](#operations). For a `restore_quiz`: `not_cleared` "This quiz was
restored on another device."; `not_found` "This quiz was deleted, on another
device or by the Trash's retention period."; `invalid` and `error` as under
[Operations](#operations). For grades rejected with no
**rejected** `finish`, `finish_segment` or `restore_quiz` of this device behind
them, whether because this device sent none or because the one it sent was
applied, the
first sentence follows the grades' reason: `stale_attempt` or `not_active`
"Level N was finished on another device."; `stale` "Level N was also answered
on another device."; `error` "Level N's answers could not be saved on the
server."; `trashed` and `not_found` the cascade sentences above, the latter only
in the deletion case the paragraph below sets out.
The second sentence, "K answers
from this device weren't kept.", appears only when K is greater than 0.
**`not_found` is three situations, and only one of them is a deletion.** The
reason covers a target the server never had as well as one it purged (see
[Operations](#operations)), and one response routinely holds both, since a
rejection does not stop the batch. So the device decides the sentence from what
it holds, not from the reason alone:
- The cascade is **gone** from the base, or a tombstone for it arrived in the
  same pull: the sentences above, the deletion ones.
- The missing quiz is one an **earlier rejected operation in the same response**
  was to create — the ordinary offline chain, where a `finish` rejected
  `not_deepest` or `stale_attempt` means the level behind it never existed on
  the server. Every such rejection is a **dependent** of that one: it takes no
  sentence of its own and its grades are counted in that operation's notice, so
  a device that finished two levels on a plane sees one notice naming the first
  level and one count, not a second notice saying the cascade was deleted.
- The cascade and the quiz are there but the **question** is not, which is the
  [outcome-mismatch](#conflicts) case the server's own miss set produces: the
  came-out-differently sentence stands and those grades are counted in the usual
  "weren't kept" line, with no deletion sentence.
Monitoring therefore sees `not_found` rejections after a `not_deepest` or
`stale_attempt` as a matter of course, and they are expected rather than a sign
of divergence (see [Monitoring](#deployment-and-operations)).

### Authentication while offline

- **Signed-in state.** The app keeps the signed-in account in the `signed_in`
  pointer of the unscoped database described under
  [On the device](#on-the-device), beside that account's row in its `accounts`
  table, reads the pointer
  before anything else at startup, and treats the user as signed in until a sync
  is answered with `401`. Nothing on that path needs a connection, so a reload or
  a browser restart on a plane opens the right per-user database and lands on the
  cascade the user was studying rather than on the login page.
- **A tab is bound to one account, and so is every request it sends.** The
  session cookie is one per origin, shared by every tab, while each tab runs as
  the account whose database it opened, and the sync leader is elected per
  account, so two tabs running as two accounts each sync. Without a binding, a
  login for Bob in one tab would let Alice's tab push its outbox **as Bob**,
  where every operation is rejected `not_found` and dropped for good, and pull
  Bob's rows into Alice's database, a full pull deleting her base on the way.
  So every authenticated request the app sends carries the user id the tab runs
  as, in an `X-Wordfall-User` header read from `meta`, and the server refuses a
  request whose header differs from the session's user with `401` **before
  anything is applied or read** (see [Security](#authentication)). The ordinary
  `401` rule then does the right thing: the tab keeps its data and its outbox and
  shows **Log in to sync**. At startup, once online, the app compares
  `GET /api/auth/me`'s `user_id` with the pointer and treats a difference the same
  way. And because the first request after a switch may be minutes away, every
  tab listens on a `BroadcastChannel` for changes to the `signed_in` pointer: a
  tab whose account is no longer the signed-in one stops syncing and downloading
  and shows **signed out in another tab**, keeping its data, and does not reload
  on its own. It **resumes** when the pointer names its own account again — the
  user logged out and back in elsewhere, say to clear **Log in to sync** — since
  the cookie is then that account's again and the account-binding check lets its
  requests through.
- **Expired session.** A `401` during sync, from an expired session or "Sign out
  everywhere", keeps all local data and the outbox, and shows **Log in to
  sync**. Studying continues. Once the user logs in again, the outbox is pushed.
  This holds even when the session was revoked on purpose: a device that is out
  of the user's hands keeps its downloaded word lists, which is accepted,
  because wiping on `401` would also destroy the unsynced work of a user who
  pressed Sign out everywhere while their phone was offline.
- **A different user.** If a different user logs in on the device, the previous
  user's local data stays, untouched, until that user logs in again.
- **A deleted account** is the one case where a `401` is not the whole story.
  The device that ran `DELETE /api/account` removes its own copy at once (see
  [Authentication](#authentication)), because keeping data for an account that
  can never sign in again would leave it on the device for good. Any other
  device finds out on its next sync, is answered `401`, and follows the rule
  above — its data and outbox stay, it shows **Log in to sync**, and only the
  user's own removal action or the browser clearing site data reclaims the
  space.
- **Logging out.** The app tries to sync first, then **always completes
  locally**: it clears the `signed_in` pointer whether or not
  `POST /api/auth/logout` succeeded, so the next startup shows the login page
  even on a plane, and that request is queued and retried at the next
  connection on which no account is signed in (and after any `429` backoff) so
  the session cookie is cleared server-side too. **The queue is the flag on that account's `accounts` row**, in the
  unscoped database, not the outbox: the outbox lives in the per-user database,
  which the removal action in this same dialog deletes, and a logout that
  vanished with it would leave the cookie alive for its whole TTL on a device the
  user has walked away from. The retry runs **only while no account is signed
  in** — at a startup that finds no pointer, or straight after a logout — and
  the flag is cleared on a `2xx` or on **any `4xx` but `429`**, and left for
  later only on a `429`, a `5xx` or a network error. The test is whether the
  request could ever be accepted, not which status came back: a `401` means the
  session is already gone, and a `403` means this request will never be accepted
  as it stands. Naming the class rather than a list of statuses matters because
  what the retry carries is not up to the app. **The state it waits for is that
  no account is signed in, which is not the same as no cookie**: the session
  cookie is `httpOnly`, only the server's own logout response clears it, so the
  browser still holds it and sends it with the retry. The request is therefore an
  ordinary cookie-authenticated write, and the app **sends the double-submit CSRF
  token** with it, read from the CSRF cookie, which carries the session cookie's
  TTL and so is there whenever the session cookie is; that path is a `2xx`, and
  it is what actually clears the cookie. Only when both cookies have expired,
  which is what a device offline past their TTL meets, does the request arrive
  with no session cookie, and then the endpoint
  [needs neither a session nor a CSRF token](#auth-and-account) and answers
  `2xx` as well, because there is nothing left to clear. The one `403` left is a
  session cookie with the CSRF cookie gone — a browser that cleared part of its
  site data — and the class rule above clears the flag for it rather than
  firing the same doomed request at every startup for the life of the
  installation. It runs only while no account is signed in
  because the endpoint clears whatever session cookie the browser holds, and the
  browser holds one: a login for another account replaces the cookie this
  logout was for, so firing the queued request afterwards would sign **that**
  account out of a session it never left. That is why a login clears every
  pending flag (see [On the device](#on-the-device)): once a new session cookie
  has been set, the cookie the queued logout meant to clear is already gone, and
  the request has nothing left to do. Until it goes through, the cookie the browser still holds grants nothing
  through the app, which never acts on a session without a signed-in pointer,
  and it is the same window a copied stateless token already has (see
  [the logout endpoint](#auth-and-account)). That user's
  per-user database is **kept**, exactly as it is when a different user logs in,
  so logging back in here finds its cascades, keys and cards already local and
  pushes anything the outbox still holds. When operations are unsent the dialog
  says how many and that they will sync the next time that user logs in on this
  device. The dialog also offers **Remove this account's data from this
  device**, which deletes the per-user database and, when the outbox is not
  empty, says how many changes that discards. It then empties that account's
  `accounts` row — its totals and its last-signed-in time — and deletes the row
  itself **unless the row still holds an unacknowledged logout**, in which case
  a row carrying that account's id, its username and the flag alone survives
  until the request goes through and is deleted when the flag clears. An emptied
  row is never listed on the Account page's storage line, since there is no
  data behind it to remove. Because `ROW_STORAGE_BUDGET` and
  the answer limit are per user, a device several accounts have used can hold
  several budgets; the Account page's storage line lists every account in the
  `accounts` table that still has data here, with the totals its row holds, and
  offers the same action for each.

---

## Schema

There is a single migration, `backend/migrations/0001_initial.sql`, run by SQLx
at startup (SQLx creates its own `_sqlx_migrations` table). It is edited in
place until the first deployment, after which migrations are append-only.
**Two tasks starting together are safe**: SQLx takes a Postgres advisory lock
around the run, so a rolling deploy's second task waits and then finds every
migration applied and proceeds to bind. Nothing else in the plan serialises
startup, so this is the reason a hand-rolled runner is not used. The
full schema:

```sql
-- =========================================================================
-- Wordfall schema
-- =========================================================================

CREATE EXTENSION IF NOT EXISTS citext;

-- -------------------------------------------------------------------------
-- Enums
-- -------------------------------------------------------------------------

CREATE TYPE quiz_type AS ENUM ('anagram', 'definition', 'leave_value');

CREATE TYPE grade AS ENUM ('correct', 'missed');

CREATE TYPE anagram_answer_mode AS ENUM ('flashcard', 'typed');

CREATE TYPE input_action AS ENUM ('show_next', 'toggle_grade', 'previous');

CREATE TYPE input_kind AS ENUM ('mouse_button', 'wheel', 'key');

-- 'cleared' means finished and in the Trash, whether or not the score met the
-- clear threshold (see Progression).
CREATE TYPE quiz_status AS ENUM ('active', 'cleared');

-- What a finish below the clear threshold does.
CREATE TYPE quiz_progression AS ENUM ('ladder', 'drill');

-- How a quiz came to exist.
CREATE TYPE quiz_origin AS ENUM (
    'source',             -- created from the filter search
    'clear_replacement',  -- the misses of a cleared quiz, at the same level
    'drill_replacement',  -- Drill progression: the misses of a quiz that was not cleared,
                          -- at the same level
    'descent',            -- Ladder progression, or the Source quiz at any score: the
                          -- misses of a quiz that was not cleared, one level down
    'segment'             -- the misses of one run of a quiz, one level down
);

CREATE TYPE finish_outcome AS ENUM (
    'cleared',     -- score met the threshold; quiz to the Trash (never the Source quiz)
    'replaced',    -- Drill: score did not meet the threshold; quiz to the Trash anyway
    'descended',   -- Ladder, or the Source quiz at any score: quiz reset, misses one level down
    'completed',   -- Source quiz, no misses: quiz reset in place; cascade complete
    'reshuffled'   -- nothing correct; quiz reset in place
);

CREATE TYPE sync_op_type AS ENUM (
    'grade', 'move_cursor', 'finish', 'finish_segment', 'restore_quiz',
    'trash_cascade', 'restore_cascade', 'purge_quiz', 'purge_cascade',
    'set_preferences', 'set_bindings', 'set_cascade_options', 'set_quiz_options'
);

CREATE TYPE sync_op_status AS ENUM ('applied', 'rejected');

CREATE TYPE sync_entity AS ENUM ('cascade', 'quiz');

CREATE TYPE condition_type AS ENUM (
    'anagram_match',
    'pattern_match',
    'subanagram_match',
    'length',
    'in_lexicon',
    'in_word_list',
    'num_vowels',
    'includes_letters',
    'probability_order',
    'limit_by_probability_order',
    'playability_order',
    'limit_by_playability_order',
    'num_unique_letters',
    'point_value',
    'takes_prefix',
    'takes_suffix',
    'part_of_speech',
    'definition',
    'consists_of',
    'num_anagrams',
    'front_inner_hook',
    'back_inner_hook',
    'leave_value'
);

CREATE TYPE part_of_speech AS ENUM (
    'adjective',
    'adverb',
    'conjunction',
    'definite_article',
    'indefinite_article',
    'interjection',
    'noun',
    'preposition',
    'pronoun',
    'verb'
);

-- -------------------------------------------------------------------------
-- Accounts
-- -------------------------------------------------------------------------

CREATE TABLE users (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username            CITEXT NOT NULL UNIQUE
                            CHECK (username ~ '^[A-Za-z0-9_]{3,32}$'),
    email               CITEXT NOT NULL UNIQUE
                            CHECK (length(email) BETWEEN 3 AND 254),
    password_hash       TEXT NOT NULL,                  -- Argon2 PHC string
    email_confirmed_at  TIMESTAMPTZ,
    is_admin            BOOLEAN NOT NULL DEFAULT false, -- set only via SQL
    session_generation  INTEGER NOT NULL DEFAULT 0,     -- bumped to revoke all sessions
    sync_seq            BIGINT NOT NULL DEFAULT 0,      -- last sync sequence value issued
    sync_floor_seq      BIGINT NOT NULL DEFAULT 0,      -- tombstones at or below this were pruned
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (sync_floor_seq <= sync_seq)
);

CREATE TABLE email_confirmations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash   BYTEA NOT NULL UNIQUE CHECK (octet_length(code_hash) = 32), -- SHA-256
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,               -- created_at + 24 hours
    used_at     TIMESTAMPTZ
);
CREATE INDEX email_confirmations_user_id ON email_confirmations (user_id);

CREATE TABLE password_reset_tokens (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32), -- SHA-256
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,               -- created_at + 30 minutes
    used_at     TIMESTAMPTZ
);
CREATE INDEX password_reset_tokens_user_id_unused
    ON password_reset_tokens (user_id) WHERE used_at IS NULL;

-- One row per user, inserted in the same transaction as the user.
CREATE TABLE user_preferences (
    user_id                   UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    default_clear_threshold   SMALLINT NOT NULL DEFAULT 80
                                  CHECK (default_clear_threshold BETWEEN 1 AND 100),
    leave_value_decimals      SMALLINT NOT NULL DEFAULT 1
                                  CHECK (leave_value_decimals BETWEEN 0 AND 3),
    anagram_show_definitions  BOOLEAN NOT NULL DEFAULT false,
    anagram_show_hooks        BOOLEAN NOT NULL DEFAULT false,
    anagram_answer_mode       anagram_answer_mode NOT NULL DEFAULT 'flashcard',
    -- Defaults for new cascades only; changing one never touches an existing cascade.
    default_segment_size      INTEGER NOT NULL DEFAULT 0
                                  CHECK (default_segment_size = 0
                                         OR default_segment_size BETWEEN 5 AND 300000),
    default_progression       quiz_progression NOT NULL DEFAULT 'ladder',
    default_require_alphabetical BOOLEAN NOT NULL DEFAULT false,
    changed_at                TIMESTAMPTZ NOT NULL DEFAULT now(), -- device time of the latest
                                  -- set_preferences; an older operation is rejected whole
    changed_by_device_id      UUID,                               -- NULL until the first change;
                                  -- that device's own next change always applies
    bindings_changed_at       TIMESTAMPTZ NOT NULL DEFAULT now(), -- device time of latest binding change
    bindings_device_id        UUID,                               -- likewise for bindings
    updated_seq               BIGINT NOT NULL                     -- the sequence registration took, then
                                  -- bumped by every preferences or bindings change,
                                  -- so every device's first pull carries the row
);

-- Desktop controls. The defaults are inserted with the user; every action keeps
-- at least one binding.
CREATE TABLE user_input_bindings (
    user_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    action   input_action NOT NULL,
    slot     SMALLINT NOT NULL CHECK (slot BETWEEN 0 AND 2),
    kind     input_kind NOT NULL,
    code     TEXT NOT NULL,
    ctrl     BOOLEAN NOT NULL DEFAULT false,
    shift    BOOLEAN NOT NULL DEFAULT false,
    alt      BOOLEAN NOT NULL DEFAULT false,
    meta     BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (user_id, action, slot),
    UNIQUE (user_id, kind, code, ctrl, shift, alt, meta), -- one action per stroke
    CHECK (CASE kind
        WHEN 'mouse_button' THEN code IN ('left', 'middle', 'right', 'back', 'forward')
        WHEN 'wheel'        THEN code IN ('up', 'down')
        WHEN 'key'          THEN code ~ '^[A-Za-z0-9]{1,32}$'  -- KeyboardEvent.code
                                 AND code <> 'Escape'
    END)
);

-- -------------------------------------------------------------------------
-- Catalog (uploaded by admins; immutable once uploaded)
-- -------------------------------------------------------------------------

CREATE TABLE letter_distributions (
    id           SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name         TEXT NOT NULL UNIQUE CHECK (name ~ '^[A-Za-z0-9 _-]{1,32}$'),
    uploaded_by  UUID REFERENCES users (id) ON DELETE SET NULL,
    uploaded_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per line of the uploaded MAGPIE letter distribution file.
CREATE TABLE letter_distribution_tiles (
    letter_distribution_id  SMALLINT NOT NULL
                                REFERENCES letter_distributions (id) ON DELETE CASCADE,
    position                SMALLINT NOT NULL CHECK (position >= 0),
                                -- line number from 0; the tile order; 0 is the blank
    letter                  TEXT NOT NULL CHECK (octet_length(letter) BETWEEN 1 AND 8),
    blank_letter            TEXT NOT NULL CHECK (octet_length(blank_letter) BETWEEN 1 AND 8),
    count                   SMALLINT NOT NULL CHECK (count >= 0),  -- tiles of this kind in the bag
    value                   SMALLINT NOT NULL CHECK (value >= 0),  -- points
    is_vowel                BOOLEAN NOT NULL,
    fullwidth_letter        TEXT,
    fullwidth_blank_letter  TEXT,
    PRIMARY KEY (letter_distribution_id, position),
    UNIQUE (letter_distribution_id, letter),
    UNIQUE (letter_distribution_id, blank_letter),
    CHECK ((position = 0) = (letter = '?')),
    CHECK (position <> 0 OR (blank_letter = '?' AND value = 0 AND NOT is_vowel)),
    CHECK (position = 0 OR (letter       !~ '[\[\],?*.[:space:]]'
                        AND blank_letter !~ '[\[\],?*.[:space:]]')),
    CHECK ((fullwidth_letter IS NULL) = (fullwidth_blank_letter IS NULL))
);

CREATE TABLE lexicons (
    id                      SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name                    TEXT NOT NULL UNIQUE CHECK (name ~ '^[A-Za-z0-9_-]{1,32}$'),
    letter_distribution_id  SMALLINT NOT NULL REFERENCES letter_distributions (id),
    word_count              INTEGER NOT NULL CHECK (word_count > 0),
    uploaded_by             UUID REFERENCES users (id) ON DELETE SET NULL,
    uploaded_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX lexicons_letter_distribution_id ON lexicons (letter_distribution_id);

CREATE TABLE lexicon_words (
    lexicon_id   SMALLINT NOT NULL REFERENCES lexicons (id) ON DELETE CASCADE,
    word         TEXT NOT NULL CHECK (char_length(word) BETWEEN 1 AND 300
                                      AND position('?' IN word) = 0),
                     -- MAGPIE notation; 1–15 tiles, checked on upload. 15 bracketed
                     -- 8-byte tiles need 150 characters; 300 leaves margin.
    playability  DOUBLE PRECISION NOT NULL
                     CHECK (playability NOT IN ('NaN'::float8, 'Infinity'::float8, '-Infinity'::float8)),
    definition   TEXT NOT NULL CHECK (char_length(definition) BETWEEN 1 AND 10000),
    PRIMARY KEY (lexicon_id, word)
);

-- At most one per lexicon. Leaves use the lexicon's letter distribution.
CREATE TABLE leave_sets (
    id           INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    lexicon_id   SMALLINT NOT NULL UNIQUE REFERENCES lexicons (id),
    leave_count  INTEGER NOT NULL CHECK (leave_count > 0),
    uploaded_by  UUID REFERENCES users (id) ON DELETE SET NULL,
    uploaded_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, lexicon_id)   -- target of cascades' composite foreign key
);

CREATE TABLE leave_values (
    leave_set_id  INTEGER NOT NULL REFERENCES leave_sets (id) ON DELETE CASCADE,
    leave         TEXT NOT NULL CHECK (char_length(leave) BETWEEN 1 AND 120),
                      -- MAGPIE notation; 1–6 tiles in distribution order, blanks first.
                      -- 6 bracketed 8-byte tiles need 60 characters; 120 leaves margin.
    value         DOUBLE PRECISION NOT NULL
                      CHECK (value NOT IN ('NaN'::float8, 'Infinity'::float8, '-Infinity'::float8)),
    PRIMARY KEY (leave_set_id, leave)
);

-- -------------------------------------------------------------------------
-- Filter specifications (shared by saved searches and cascades)
-- -------------------------------------------------------------------------

CREATE TABLE search_specs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    quiz_type   quiz_type NOT NULL,   -- the type this spec's In Word List entries were
                            -- canonicalised under: a leave is sorted into tile order and
                            -- a word is not, so loading the spec under a type on the
                            -- other side of Leave Value flags that row rather than
                            -- silently reading leaves as words. Start over copies it
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX search_specs_user_id ON search_specs (user_id);

CREATE TYPE group_op AS ENUM ('and', 'or');

-- AND / OR groups of a spec. Group 0 is the top of every spec and has no
-- parent; every other group and every condition belongs to exactly one group.
-- Children of a group, conditions and groups alike, are ordered by
-- order_in_group, which the application keeps unique per group.
CREATE TABLE search_groups (
    spec_id         UUID NOT NULL REFERENCES search_specs (id) ON DELETE CASCADE,
    id              SMALLINT NOT NULL CHECK (id BETWEEN 0 AND 99),
    parent_id       SMALLINT,                       -- NULL only for group 0
    op              group_op NOT NULL,
    order_in_group  SMALLINT NOT NULL CHECK (order_in_group BETWEEN 0 AND 99),
    PRIMARY KEY (spec_id, id),
    FOREIGN KEY (spec_id, parent_id) REFERENCES search_groups (spec_id, id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CHECK ((id = 0) = (parent_id IS NULL)),
    CHECK (parent_id IS NULL OR parent_id <> id)
);

CREATE TABLE search_conditions (
    spec_id               UUID NOT NULL REFERENCES search_specs (id) ON DELETE CASCADE,
    position              SMALLINT NOT NULL CHECK (position BETWEEN 0 AND 99),
                              -- row number within the spec; the identity used by
                              -- search_condition_words
    group_id              SMALLINT NOT NULL,
    order_in_group        SMALLINT NOT NULL CHECK (order_in_group BETWEEN 0 AND 99),
    condition_type        condition_type NOT NULL,
    negated               BOOLEAN NOT NULL DEFAULT false,

    -- Parameters. Which ones are set depends on condition_type (see CHECK below).
    text_value            TEXT CHECK (length(text_value) BETWEEN 1 AND 500),
                              -- pattern, tiles, prefix or suffix in canonical form
                              -- (see Tiles): MAGPIE notation, or space-separated
                              -- pattern tokens; or definition text as typed; or,
                              -- for In Lexicon in a saved search, the lexicon's
                              -- name, which the canonical-form check never sees. A pattern is what
                              -- needs the room, not a prefix: a 14-tile prefix of
                              -- 8-byte tiles is 140 characters, but a tile set has no
                              -- size cap, so 15 positions each holding a set of the
                              -- distribution's consonants run past 350 characters
                              -- ("8-letter words with no vowels" writes one per
                              -- position). The application checks the canonical
                              -- form's length before insert and reports it as a
                              -- field error naming the limit
    part_of_speech_value  part_of_speech,
    other_lexicon_id      SMALLINT REFERENCES lexicons (id),
                              -- In Lexicon in a cascade's spec, which pins the
                              -- lexicon. A saved search's In Lexicon row holds the
                              -- lexicon's name in text_value instead and pins
                              -- nothing (see Immutability and deletion)
    min_value             INTEGER CHECK (min_value >= 0),
    max_value             INTEGER CHECK (max_value >= 0),
    min_leave_value       DOUBLE PRECISION
                              CHECK (min_leave_value NOT IN ('NaN'::float8,
                                     'Infinity'::float8, '-Infinity'::float8)),
    max_leave_value       DOUBLE PRECISION
                              CHECK (max_leave_value NOT IN ('NaN'::float8,
                                     'Infinity'::float8, '-Infinity'::float8)),
                              -- as for every stored value and playability: Postgres
                              -- treats NaN as equal to itself and above everything,
                              -- so a NaN pair would pass min <= max below and then
                              -- match no leave at all
    lax                   BOOLEAN,

    PRIMARY KEY (spec_id, position),
    FOREIGN KEY (spec_id, group_id) REFERENCES search_groups (spec_id, id) ON DELETE CASCADE,

    CHECK (min_value IS NULL OR max_value IS NULL OR min_value <= max_value),
    CHECK (min_leave_value IS NULL OR max_leave_value IS NULL
           OR min_leave_value <= max_leave_value),

    -- Not is only allowed where Zyzzyva allows it.
    CHECK (NOT negated OR condition_type IN (
        'anagram_match', 'pattern_match', 'subanagram_match', 'in_lexicon',
        'in_word_list', 'includes_letters', 'takes_prefix', 'takes_suffix',
        'part_of_speech', 'definition', 'front_inner_hook', 'back_inner_hook')),

    -- Exactly the parameters each type uses are present.
    CHECK (CASE
        WHEN condition_type IN ('anagram_match', 'pattern_match', 'subanagram_match',
                                'includes_letters', 'takes_prefix', 'takes_suffix',
                                'definition')
            THEN text_value IS NOT NULL
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 1
        WHEN condition_type IN ('length', 'num_vowels', 'num_unique_letters',
                                'point_value', 'num_anagrams')
            THEN num_nonnulls(min_value, max_value) = 2
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 2
        WHEN condition_type IN ('probability_order', 'limit_by_probability_order',
                                'playability_order', 'limit_by_playability_order')
            THEN num_nonnulls(min_value, max_value, lax) = 3
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 3
        WHEN condition_type = 'consists_of'
            THEN num_nonnulls(text_value, min_value, max_value) = 3
             AND max_value <= 100
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 3
        WHEN condition_type = 'part_of_speech'
            THEN part_of_speech_value IS NOT NULL
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 1
        WHEN condition_type = 'in_lexicon'    -- the id in a cascade's spec, the
                                               -- name in a saved search's
            THEN num_nonnulls(text_value, other_lexicon_id) = 1
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 1
        WHEN condition_type IN ('in_word_list',   -- entries live in search_condition_words
                                'front_inner_hook', 'back_inner_hook')  -- no parameters
            THEN num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, lax) = 0
        WHEN condition_type = 'leave_value'    -- either bound may be open, not both
            THEN num_nonnulls(min_leave_value, max_leave_value) >= 1
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, lax) = 0
    END)
);
CREATE INDEX search_conditions_other_lexicon_id
    ON search_conditions (other_lexicon_id) WHERE other_lexicon_id IS NOT NULL;

-- In Word List entries in canonical MAGPIE notation (see Tiles). Converted to tiles at search time.
CREATE TABLE search_condition_words (
    spec_id   UUID NOT NULL,
    position  SMALLINT NOT NULL,
    entry     TEXT NOT NULL CHECK (char_length(entry) BETWEEN 1 AND 300),
    PRIMARY KEY (spec_id, position, entry),
    FOREIGN KEY (spec_id, position)
        REFERENCES search_conditions (spec_id, position) ON DELETE CASCADE
);

CREATE TABLE saved_searches (
    id          UUID PRIMARY KEY,           -- device-generated, so a save can be retried
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    spec_id     UUID NOT NULL UNIQUE REFERENCES search_specs (id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, name)
);

-- -------------------------------------------------------------------------
-- Cascades and quizzes
-- -------------------------------------------------------------------------

CREATE TABLE cascades (
    id                UUID PRIMARY KEY,           -- device-generated, so creation can be retried
    user_id           UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name              TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    quiz_type         quiz_type NOT NULL,
    lexicon_id        SMALLINT NOT NULL REFERENCES lexicons (id),
    leave_set_id      INTEGER,                    -- Leave Value cascades only
    spec_id           UUID NOT NULL REFERENCES search_specs (id),
                          -- a private copy, never a saved search's or another
                          -- cascade's spec; Start over copies it again
    clear_threshold   SMALLINT NOT NULL CHECK (clear_threshold BETWEEN 1 AND 100),

    -- Quiz options: what every quiz created for this cascade starts with.
    segment_size         INTEGER NOT NULL DEFAULT 0
                             CHECK (segment_size = 0 OR segment_size BETWEEN 5 AND 300000),
                                 -- 0 = no segments; the minimum of 5 rules out the
                                 -- one-question run. See Quiz options for the
                                 -- ceil(question_count / S) bound on chain quizzes
    progression          quiz_progression NOT NULL DEFAULT 'ladder',
    require_alphabetical BOOLEAN NOT NULL DEFAULT false,
    options_changed_at   TIMESTAMPTZ NOT NULL,   -- device time of creation or of the latest
                             -- set_cascade_options; an older operation is rejected whole
    options_seq          BIGINT NOT NULL,            -- sync sequence of that change, for seen_seq
    options_device_id    UUID NOT NULL,              -- the device that made it (the creator at first);
                                                     -- its own next change always applies

    question_count    INTEGER NOT NULL CHECK (question_count BETWEEN 1 AND 300000),
    depth             INTEGER NOT NULL CHECK (depth >= 1),
                          -- number of active levels; Level 1 is always the Source quiz
    peak_depth        INTEGER NOT NULL DEFAULT 1 CHECK (peak_depth >= depth),
                          -- deepest level reached since the last completion (or creation)
    attempts_since_completion INTEGER NOT NULL DEFAULT 0
                          CHECK (attempts_since_completion >= 0),
                          -- attempts finished on any level since the last completion;
                          -- the completion screen shows both, then both reset
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_activity_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at      TIMESTAMPTZ,                -- latest finish of the Source quiz with no misses
    trashed_at        TIMESTAMPTZ,                -- set only by the user
    updated_seq       BIGINT NOT NULL,

    -- The leave value set must belong to the cascade's lexicon.
    FOREIGN KEY (leave_set_id, lexicon_id) REFERENCES leave_sets (id, lexicon_id),

    CHECK ((quiz_type = 'leave_value') = (leave_set_id IS NOT NULL))
);
CREATE INDEX cascades_user_activity ON cascades (user_id, last_activity_at DESC);
CREATE INDEX cascades_user_seq ON cascades (user_id, updated_seq);
CREATE INDEX cascades_trashed_at ON cascades (trashed_at) WHERE trashed_at IS NOT NULL;
CREATE INDEX cascades_spec_id ON cascades (spec_id);
CREATE INDEX cascades_lexicon_id ON cascades (lexicon_id);
CREATE INDEX cascades_leave_set_id ON cascades (leave_set_id) WHERE leave_set_id IS NOT NULL;

-- The cascade's questions, in search order. Immutable.
CREATE TABLE cascade_questions (
    cascade_id    UUID NOT NULL REFERENCES cascades (id) ON DELETE CASCADE,
    idx           INTEGER NOT NULL CHECK (idx BETWEEN 0 AND 299999),
    question_key  TEXT NOT NULL CHECK (char_length(question_key) BETWEEN 1 AND 300),
                      -- alphagram, word or canonical leave, in MAGPIE notation
    PRIMARY KEY (cascade_id, idx)
);

CREATE TABLE quizzes (
    id                UUID PRIMARY KEY,           -- device-generated, the Source quiz included
    cascade_id        UUID NOT NULL REFERENCES cascades (id) ON DELETE CASCADE,
    user_id           UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    level             INTEGER NOT NULL CHECK (level >= 1),
                          -- for a cleared quiz, the level it was cleared from;
                          -- 1 only ever for the Source quiz
    origin            quiz_origin NOT NULL,
    origin_quiz_id    UUID REFERENCES quizzes (id) ON DELETE SET NULL,
    origin_attempt    INTEGER CHECK (origin_attempt >= 1),   -- the attempt it came out of
    origin_segment_end INTEGER CHECK (origin_segment_end >= 1),
                          -- segment quizzes: the run boundary it came from, in the parent's
                          -- positions. Recorded as a position, not a run number, so changing
                          -- the segment size mid-attempt cannot re-descend a finished run.
    status            quiz_status NOT NULL DEFAULT 'active',
    segment_chain     BOOLEAN NOT NULL DEFAULT false,
                          -- a run's drill quiz and every replacement of it: Drill for
                          -- life and never segmented (see Segments)

    -- Quiz options, copied from the cascade at creation and editable afterwards.
    segment_size         INTEGER NOT NULL DEFAULT 0
                             CHECK (segment_size = 0 OR segment_size BETWEEN 5 AND 300000),
    progression          quiz_progression NOT NULL DEFAULT 'ladder',
    require_alphabetical BOOLEAN NOT NULL DEFAULT false,
    options_changed_at   TIMESTAMPTZ NOT NULL,   -- device time of creation or of the latest
                             -- set_quiz_options; an older operation is rejected whole
    options_seq          BIGINT NOT NULL,            -- sync sequence of that change, for seen_seq
    options_device_id    UUID NOT NULL,              -- the device that made it; its own next
                                                     -- change always applies

    attempt           INTEGER NOT NULL DEFAULT 1 CHECK (attempt >= 1),
    shuffle_seed      BIGINT NOT NULL,            -- the u64 seed of the current attempt's order,
                                                  -- stored as its two's-complement i64; identifies
                                                  -- the attempt in sync operations
    questions_hash    BIGINT NOT NULL,            -- FNV-1a 64 of the ascending question indexes
                                                  -- (see Deterministic shuffles); fixed for life.
                                                  -- Stored as its two's-complement i64, like
                                                  -- shuffle_seed above: a hash with its top bit set
                                                  -- reads back negative, and the wire form is the
                                                  -- unsigned decimal either way (see API)
    question_count    INTEGER NOT NULL CHECK (question_count BETWEEN 1 AND 300000),
    correct_count     INTEGER NOT NULL DEFAULT 0 CHECK (correct_count >= 0),
    missed_count      INTEGER NOT NULL DEFAULT 0 CHECK (missed_count >= 0),
    cursor            INTEGER NOT NULL DEFAULT 0,
    cursor_moved_at   TIMESTAMPTZ,                -- device time, for latest-wins between devices
    cursor_device_id  UUID,                       -- the device that last moved it
    run_start         INTEGER NOT NULL DEFAULT 0, -- the last run boundary passed in this attempt
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_seq       BIGINT NOT NULL,            -- the sync sequence that created it;
                                                  -- informational. The rebase decides new
                                                  -- versus reset from its own base, never from this
    last_activity_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    cleared_at        TIMESTAMPTZ,
    updated_seq       BIGINT NOT NULL,            -- also bumped when any of its questions change

    CHECK ((status = 'cleared') = (cleared_at IS NOT NULL)),
    CHECK ((origin = 'source') = (level = 1)),             -- Level 1 is the Source quiz
    CHECK (origin <> 'source' OR status = 'active'),      -- and it is never cleared
    CHECK (origin <> 'source' OR (origin_quiz_id IS NULL AND origin_attempt IS NULL)),
    CHECK ((origin = 'segment') = (origin_segment_end IS NOT NULL)),
    CHECK (origin <> 'segment' OR segment_chain),            -- a run's misses start a chain
    CHECK (NOT segment_chain OR (progression = 'drill' AND segment_size = 0)),
    CHECK (correct_count + missed_count <= question_count),
    CHECK (cursor >= 0 AND cursor < question_count),      -- the card being shown
    CHECK (run_start BETWEEN 0 AND cursor),                -- Previous never crosses a boundary
    CHECK ((cursor_moved_at IS NULL) = (cursor_device_id IS NULL))
);
CREATE UNIQUE INDEX quizzes_one_active_per_level
    ON quizzes (cascade_id, level) WHERE status = 'active';
CREATE UNIQUE INDEX quizzes_one_source ON quizzes (cascade_id) WHERE origin = 'source';
CREATE INDEX quizzes_cascade_id ON quizzes (cascade_id);   -- purges and the FK cascade
CREATE INDEX quizzes_user_seq ON quizzes (user_id, updated_seq);
CREATE INDEX quizzes_cleared_at ON quizzes (cleared_at) WHERE status = 'cleared';
CREATE INDEX quizzes_origin_quiz_id ON quizzes (origin_quiz_id) WHERE origin_quiz_id IS NOT NULL;
-- One descent per run per attempt, so a repeated or racing finish_segment cannot make two.
CREATE UNIQUE INDEX quizzes_one_per_segment
    ON quizzes (origin_quiz_id, origin_attempt, origin_segment_end)
    WHERE origin = 'segment';

CREATE TABLE quiz_questions (
    quiz_id       UUID NOT NULL REFERENCES quizzes (id) ON DELETE CASCADE,
    question_idx  INTEGER NOT NULL CHECK (question_idx BETWEEN 0 AND 299999),
                      -- index into cascade_questions
    position      INTEGER NOT NULL CHECK (position BETWEEN 0 AND 299999),
    grade         grade,
    graded_at     TIMESTAMPTZ,                    -- device time, for latest-wins between devices
    graded_by_device_id UUID,                     -- a device's own regrade always applies
    updated_seq   BIGINT NOT NULL,
    PRIMARY KEY (quiz_id, question_idx),
    CONSTRAINT quiz_questions_position_unique
        UNIQUE (quiz_id, position) DEFERRABLE INITIALLY DEFERRED,
    CHECK ((grade IS NULL) = (graded_at IS NULL)),
    CHECK ((grade IS NULL) = (graded_by_device_id IS NULL))
);
CREATE INDEX quiz_questions_quiz_seq ON quiz_questions (quiz_id, updated_seq);

-- One row per finished attempt. Immutable.
CREATE TABLE quiz_attempts (
    quiz_id         UUID NOT NULL REFERENCES quizzes (id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
                        -- denormalised exactly as quizzes.user_id is, so a pull can
                        -- find this user's changed attempt rows through one index
                        -- range scan. Attempt rows are not narrowed by
                        -- question_rows_for and travel for cleared quizzes too, so
                        -- without it the candidate set is every quiz the user owns:
                        -- 60,000 chain quizzes would mean 60,000 seeks per sync
    attempt         INTEGER NOT NULL CHECK (attempt >= 1),
    question_count  INTEGER NOT NULL CHECK (question_count BETWEEN 1 AND 300000),
    correct_count   INTEGER NOT NULL CHECK (correct_count >= 0),
    missed_count    INTEGER NOT NULL CHECK (missed_count >= 0),
    outcome         finish_outcome NOT NULL,
    shuffle_seed    BIGINT NOT NULL,              -- the order this attempt was played in, as its
                                                 -- two's-complement i64, like quizzes.shuffle_seed
    finished_at     TIMESTAMPTZ NOT NULL,         -- device time
    updated_seq     BIGINT NOT NULL,
    PRIMARY KEY (quiz_id, attempt),
    CHECK (correct_count + missed_count = question_count)
);
CREATE INDEX quiz_attempts_user_seq ON quiz_attempts (user_id, updated_seq);

-- -------------------------------------------------------------------------
-- Sync bookkeeping
-- -------------------------------------------------------------------------

-- Operations received, so a repeated operation is recognised. A record whose operation
-- returns no result fields is deleted once its device has shown it received the result
-- (below sync_devices.acked_below); finish, finish_segment and restore_quiz records are
-- kept for SYNC_RETENTION_DAYS. See The sync cycle.
-- Looked up by (user_id, id): an id another user has already used is rejected, never replayed.
CREATE TABLE sync_operations (
    id           UUID PRIMARY KEY,                -- device-generated operation id
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id    UUID NOT NULL,
    device_seq   BIGINT NOT NULL CHECK (device_seq >= 1),
    op_type      sync_op_type NOT NULL,
    status       sync_op_status NOT NULL,
    reason       TEXT,
    -- The result fields of an applied finish, finish_segment or restore_quiz,
    -- returned unchanged when the operation is sent again.
    outcome      TEXT,
    new_quiz_question_count  INTEGER CHECK (new_quiz_question_count >= 1),
    new_quiz_questions_hash  BIGINT,           -- two's-complement i64, like quizzes.questions_hash
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, device_id, device_seq),
    CHECK ((status = 'rejected') = (reason IS NOT NULL)),
    CHECK (status = 'applied' OR num_nonnulls(outcome, new_quiz_question_count,
                                              new_quiz_questions_hash) = 0),
    CHECK ((new_quiz_question_count IS NULL) = (new_quiz_questions_hash IS NULL))
);
CREATE INDEX sync_operations_received_at ON sync_operations (received_at);

-- Per device, the device_seq below which every operation is known to have been
-- acknowledged: raised to the lowest device_seq of each request that carries
-- operations. An operation below it with no record is answered applied and not
-- applied again.
CREATE TABLE sync_devices (
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id    UUID NOT NULL,
    acked_below  BIGINT NOT NULL DEFAULT 1 CHECK (acked_below >= 1),
    PRIMARY KEY (user_id, device_id)
);

-- Records of purged cascades and quizzes, so other devices remove them. Pruned after 90 days.
CREATE TABLE sync_tombstones (
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    entity      sync_entity NOT NULL,
    entity_id   UUID NOT NULL,
    seq         BIGINT NOT NULL,
    deleted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, entity, entity_id)
);
CREATE INDEX sync_tombstones_user_seq ON sync_tombstones (user_id, seq);
CREATE INDEX sync_tombstones_deleted_at ON sync_tombstones (deleted_at);

-- Export download tokens that have been redeemed, so a token is used once
-- whichever task receives it. The token itself is a signed PASETO; only its
-- spent jti is stored. Pruned by the purge task once expired.
CREATE TABLE export_tokens_spent (
    jti         UUID PRIMARY KEY,
    expires_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX export_tokens_spent_expires_at ON export_tokens_spent (expires_at);

-- -------------------------------------------------------------------------
-- Catalog load status per backend instance (see Loading changes into
-- running servers). An instance writes a row when it has built an item's
-- index and refreshes heartbeat_at every 60 seconds; rows older than
-- 3 minutes belong to a dead instance and are ignored, then pruned.
-- -------------------------------------------------------------------------

CREATE TYPE catalog_item_kind AS ENUM ('letter_distribution', 'lexicon', 'leave_set');

CREATE TABLE catalog_instance_status (
    instance_id   UUID NOT NULL,                 -- minted at startup
    item_kind     catalog_item_kind NOT NULL,
    item_id       INTEGER NOT NULL,
    loaded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    heartbeat_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (instance_id, item_kind, item_id)
);
CREATE INDEX catalog_instance_status_heartbeat ON catalog_instance_status (heartbeat_at);
```

Notes on the schema:

- **No JSONB.** Filter parameters are typed columns, and a per-type `CHECK`
  guarantees each stored condition has exactly the parameters its type uses.
  The AND / OR tree is `search_groups` rows, with group 0 at the top; group
  depth, the 100-row total and non-empty groups are checked by the application.
  The Rust `ConditionKind` enum is loaded from these rows and written back to
  them. A round-trip test (see [Testing](#testing)) covers all 23 types and the
  group tree. Sync
  operation payloads are not stored at all; only each operation's id, type and
  result are kept, the result including the outcome, count and hash a finish
  or restore reported, so a repeat gets the same answer.
- **The cascade stack is enforced by the database where it can be.** At most one
  active quiz per level, exactly one Source quiz, which is the only quiz that
  can be at Level 1 and is always active, at most one quiz per (parent quiz,
  attempt, run boundary), a segment-chain quiz always on Drill progression with
  no segments, and
  `depth` at least 1.
- **Which clock stamps a row.** `cleared_at`, `trashed_at` and `completed_at`
  are set to the server's `now()` when the operation that causes them is
  applied, never from the operation's `at`, so the purge clocks and the
  Complete badge count from when the server learned of the change: a finish
  played offline three weeks ago still gets a full retention period. Both
  `last_activity_at` columns take `greatest(existing, clamped at)` instead, so
  late-synced old work never lowers them. They
  are stamped by `grade`, `finish`, `finish_segment`, `restore_quiz` and
  `restore_cascade` (the cascade's by all five, the quiz's by the first three
  and by `restore_quiz` on the restored quiz) and never by cursor moves,
  option changes, trashing or purging, so
  the Cascades page ordering reflects when the studying happened, not when it
  synced (the download window is judged on each device from its own last-open
  times). Only `graded_at`, `cursor_moved_at`,
  the options and preferences timestamps and `finished_at` carry device time
  for their own comparisons.
- **Quiz options are copied, never referenced.** `cascades` holds what new
  quizzes start with and `quizzes` holds what each quiz actually uses, so
  changing a cascade's options can never rewrite the rules a quiz in progress is
  being played under. Both sets of columns are plain typed columns with the same
  names, so the copy is one `INSERT … SELECT`.
- **Tiles are validated by the application.** A `CHECK` cannot parse MAGPIE
  notation against a distribution, so the upload validator guarantees:
  - every word parses into 1–15 non-blank tiles of its lexicon's distribution
  - every leave parses into 1–6 tiles of its lexicon's distribution, within bag
    counts, written in canonical order, and its value's absolute value is at
    most 1,000,000, so the shared rounding never leaves 64-bit range
  - a distribution's positions run 0..n−1 with no gaps, and no `letter` or
    `blank_letter` is longer than 8 bytes (the schema checks the bytes; the
    upload reports the line), and no `blank_letter` equals another tile's
    `letter`, so upper-cased typed text maps to one tile
- **Catalog deletion is refused while in use.** References to
  `letter_distributions`, `lexicons` and `leave_sets` from other catalog rows,
  cascades and search conditions have no `ON DELETE` action, so deleting a
  referenced item fails. Deleting an unreferenced lexicon or leave value set
  cascades to its words or values. The admin API checks references first so it
  can explain the refusal.
- **Deleting an account** is a single `DELETE FROM users`. Everything the user
  owns cascades from `users`. References between user-owned rows
  (`saved_searches.spec_id`, `cascades.spec_id`) use the default `NO ACTION`,
  which Postgres checks at the end of the statement. Cascaded deletes of both
  sides therefore succeed, while deleting a spec that is still in use fails.
  Catalog items the user uploaded stay, with `uploaded_by` set to `NULL`.
- **Purging a cascade** deletes its question index, quizzes, questions and
  attempts by cascade. In the same transaction, the application deletes the
  cascade's spec, which nothing else ever references because saved searches and
  Start over each take their own copy, writes tombstones for the cascade and
  each quiz, and bumps the user's sync sequence.
- **The 300,000 ceiling** lives in `cascades.question_count`,
  `quizzes.question_count`, `quiz_attempts.question_count`,
  `cascade_questions.idx` and `quiz_questions.position`,
  so no configuration can exceed it.
- **Invariants left to the application** because a `CHECK` cannot see other
  rows. Integration tests and the shared rule vectors cover each one.
  - `question_idx < cascades.question_count`, and `position < quizzes.question_count`
  - `quizzes.user_id` and `quiz_attempts.user_id` equal their cascade's and
    quiz's `user_id`, and the spec belongs to the same user
  - `cascades.depth` equals the number of active quizzes, and the active levels
    are exactly `1..depth`. **Nothing bounds `depth`**: under Ladder a finish
    with any miss adds a level, so a user who keeps missing the same question
    keeps adding one, each costing a quiz row and its question rows. That is
    left uncapped on purpose — refusing a descent would be the one place a
    finish could fail for a reason the user cannot act on, and every level costs
    a full graded attempt — so the display collapses instead of the rule
    refusing (see [CascadeLadder](#frontend))
  - the counters on `quizzes` match its questions' grades
  - `search_condition_words` rows belong only to `in_word_list` conditions,
    their entries are canonical for their spec's `quiz_type`, and one spec holds
    at most 300,000 of them across all its rows
  - every `search_groups` row other than group 0 and every condition has a
    parent that exists, groups nest at most 4 deep, no group is empty, and
    `order_in_group` is unique among a group's children
  - `question_key` exists in the cascade's lexicon or leave value set
  - an `in_lexicon` condition uses `other_lexicon_id` in a cascade's spec and
    `text_value` (the lexicon's name) in a saved search's spec, never the other
    way round; a save converts the id to the name, and creation resolves the
    name back to an id, refusing a name that no longer exists
  - an `in_lexicon` condition's `other_lexicon_id` shares the letter
    distribution of the lexicon the spec is searched against
  - `word_count` and `leave_count` match their rows
  - a user has at most `MAX_CASCADES_PER_USER` cascades, counting the Trash,
    and at most `MAX_SAVED_SEARCHES_PER_USER` saved searches
  - every input action has at least one binding
  - a segment quiz's `origin_segment_end` is a multiple of the parent's segment
    size at the time, is less than the parent's `question_count`, and its
    questions are exactly the questions its parent had graded missed in
    positions from the parent's `run_start` at the time up to, but not
    including, that boundary
  - `segment_chain` is set on every quiz with origin `segment`, on every
    replacement of a `segment_chain` quiz, and on nothing else
  - a quiz waiting because of a segment descent has its cursor and `run_start`
    at that boundary
  - `peak_depth` is the greatest `depth` the cascade has had since its last
    `completed_at`, and `attempts_since_completion` the number of attempts
    finished since then, on any level, **whether or not the quizzes that held
    them still exist**. Neither is derivable from the rows that survive: the
    purge task deletes a cleared quiz's `quiz_attempts` rows a retention
    period after it was cleared, and nothing records the depths a cascade
    passed through. Both are maintained only by the
    [rule functions](#rule-implementation) and never recomputed, so the
    invariant test checks them against the operation history and never against
    the surviving `quiz_attempts` rows
  - `shuffle_seed` reproduces the quiz's positions through the shared shuffle
  - a quiz's options were copied from its cascade when it was created, which
    only the creating code can guarantee, so the rule vectors check it

### Purge task

Each backend instance runs an hourly background task. It first takes
`pg_try_advisory_lock`, so only one instance purges at a time. It works one
user per transaction, and within a user it locks the user row first (the
`sync_seq` bump) and only then touches that user's cascades and quizzes, the
same order a sync takes, so a purge and a sync for one user never deadlock.
Purging a cascade is one `DELETE` per table by cascade id, and the
[scale test](#scale-tests) holds a 300,000-question purge to a budget.

**A run is bounded per user.** One transaction purges at most
`PURGE_MAX_QUIZZES_PER_USER_PER_RUN` quizzes (5,000 by default), oldest
`cleared_at` first, and leaves the rest for the next run. The bound is there
because the work is not spread out in time the way the retention period
suggests: one segmented attempt can clear 60,000 chain quizzes within hours
(see [Quiz options](#quiz-options)), so they all age out in the same hour, and
one transaction deleting them all would hold the user row — which every sync
for that user waits on — long enough for those syncs to reach the statement
timeout and be answered `503`. Capped, a Trash that large drains over a few
hourly runs while the user keeps syncing, and the Trash page's purge dates stay
true to within those runs. The cap counts the cleared quizzes a run purges **on
their own**, each of which costs its own delete and tombstone. A **trashed
cascade is still purged whole**, since half a cascade is not a state the Trash
has, and it is cheap for its size — bulk statements by cascade id, which the
[scale test](#scale-tests) budgets at 300,000 questions — but a run that has
reached its cap starts no further cascade and leaves it for the next. Then
it:

- **purges** cleared quizzes whose `cleared_at` is older than
  `TRASH_RETENTION_DAYS` and whose cascade is not trashed, and trashed cascades
  whose `trashed_at` is older than that together with every quiz in them,
  writing tombstones and bumping each affected user's sync sequence
- **prunes** `sync_operations` and `sync_tombstones` older than
  `SYNC_RETENTION_DAYS`, raising each affected user's `sync_floor_seq` to the
  highest tombstone sequence it removed
- **deletes** accounts that were never confirmed and whose last confirmation
  code expired more than 7 days ago, so an abandoned registration does not hold
  an email address forever
- **prunes** `export_tokens_spent` rows past their `expires_at`
- **prunes** `catalog_instance_status` rows whose heartbeat is older than
  3 minutes, left by instances that have stopped

---

## Authentication

The flows are the conventional ones, built directly on Axum:

- **Register** (`/register`): username, email and password. The server
  validates all fields and returns `400` with **every** field error at once.
  Passwords are checked for strength with `zxcvbn` (score ≥ 3) and length.
  A taken username is reported as a field error, since usernames are not secret,
  and that check runs before anything below, so a `400` never also sends an
  email. An email already in use gets the same response as a successful
  registration.
  The password is hashed on every path, whether or not the hash is stored, so
  the response takes the same time whichever branch runs.
  If that email belongs to a **confirmed** account, its owner is emailed a
  notice instead. If it belongs to an account that was **never confirmed**: while
  its code is still valid, a fresh code is sent to the same address and both
  stay valid until they expire, so a stranger who knows the address cannot keep
  the real registrant from confirming; once the code has expired, the old
  account row is deleted (its dependents go by cascade) and the new
  registration creates a fresh one through the ordinary path, with a new id,
  username and password, and a fresh code is sent. A username held by an
  unconfirmed account is taken while that account's latest code is valid and
  free once it has expired, in which case the old row is deleted as above. So an expired code never
  strands an address, and either way the form cannot be used to find out which
  emails have accounts. The password is hashed with Argon2. A confirmation code
  (32 random bytes) is emailed, and only its SHA-256 is stored, with a 24-hour
  expiry. At most three confirmation, notice or reset emails are sent to one
  address in 24 hours per requesting IP, and twenty per address in all; further
  requests get the same response and send nothing, and the per-IP scope means
  a stranger cannot use the cap to block the address's owner. A
  registration that replaces an expired unconfirmed account still replaces it
  when the cap has been reached; its code goes out with the next registration
  after the window. The
  `user_preferences` row and the default `user_input_bindings` are
  created with the user, and registration takes the account's first sync
  sequence and stamps the preferences row with it, so a new device's first pull
  carries the preferences even when they were never changed.
- **Confirm email** (`/confirm-email?code=…`): the page submits the code; on
  success the user is sent to `/login`. Login is refused with `403` until the
  email is confirmed.
- **Login**: by **username** and password, never by email, so the `403` for an
  unconfirmed account reveals nothing that the public username does not. Rate
  limited on **failed** attempts, per IP and per username
  (`LOGIN_FAILURES_PER_IP_PER_MINUTE` and
  `LOGIN_FAILURES_PER_USERNAME_PER_MINUTE`, 10 each): a request is refused with
  `429` **before** any Argon2 verify runs when either bucket is empty, so no
  guess ever costs a hash, and a token is taken from both only when the verify
  fails. A successful login spends nothing, because guessing is what the limits
  exist to stop, and a club or tournament room logging in together from one
  address must not lock itself out. The per-username limit lets anyone who
  knows a username keep its owner out for a minute at a time by failing on
  purpose; that is accepted over unlimited guessing against one account. The
  other auth endpoints that need no session — register, confirm-email,
  reset-password and its confirm — share one per-IP bucket,
  `AUTH_RATE_PER_IP_PER_MINUTE` (30), on top of the email caps under Register.
  `GET /api/auth/me` needs a session and is not on a per-IP bucket, and neither
  is logout, whose queued retry already honours `429`. On success the server sets:
  - a PASETO **v4.local** session token (32-byte key from
    `SESSION_SIGNING_KEY`) in an `httpOnly`, `SameSite=Lax` cookie named
    `wordfall_session`, `Secure` in production, with a 30-day TTL. `Lax`
    rather than `Strict` so that a cascade link opened from an email or a chat
    arrives signed in instead of triggering a spurious "Log in to sync"; CSRF
    protection comes from the double-submit token, not from SameSite
  - a CSRF cookie, readable by scripts (not `httpOnly`), for double-submit on
    every state-changing request. It carries the **same TTL as the session
    cookie**, never a browser-session lifetime, and is re-set on every
    response that accepts or renews a session, `GET /api/auth/me` included,
    which the app calls at startup. Otherwise a browser restart would drop it
    while the 30-day session cookie survived, and every sync would fail CSRF
    with no `401` to show **Log in to sync**

  The token carries the user id and the account's `session_generation`. Every
  request re-reads the user row, including `is_admin`, so a deleted account, a
  bumped generation, or a revoked admin flag takes effect immediately. The long
  TTL is deliberate: a device that has been offline for weeks can still sync as
  soon as it reconnects. The TTL slides: a sync made in the last seven days
  of a cookie's life is answered with a fresh cookie of the full TTL, so an
  active user is never signed out for age, and a device that stays offline
  past the TTL sees **Log in to sync** with its local data intact.
- **Password reset**: always answers with the same message, and the email is
  queued after the response so both branches take the same time. If the email
  belongs to a confirmed account, a 30-minute single-use token is emailed.
  Completing a reset spends every other outstanding reset token and increments
  `session_generation`, which signs out every session.
- **Change password**, **Sign out everywhere** and **delete account** (each
  with password re-entry) are on `/account`. Changing the password also bumps
  `session_generation` and issues a new session to the current device, and
  **Sign out everywhere does the same**: it signs out every other session and
  re-issues this one, since the user has just re-entered the password here and
  "everywhere" means everywhere else. A device that wants to be signed out as
  well uses Log out.
  **Deleting the account clears this device's copy of it.** On a `2xx` the app
  runs the same **Remove this account's data from this device** path the logout
  dialog offers (see
  [Authentication while offline](#authentication-while-offline)): the per-user
  database and the `accounts` row go, with no queued logout, since the session
  is already void, and the app lands on the landing page. Without that step the
  `401`-keeps-everything rule would apply to an account that can never log in
  again, leaving its question keys and downloaded answers on the device for good
  and listing a dead account on the next user's storage line. Another device
  holding that account's data learns of the deletion only when a sync answers
  `401`, where the ordinary rule applies: it keeps its data and shows
  **Log in to sync**, and its Account page still offers the removal.

Security generally:

- CSRF double-submit on every cookie-authenticated `POST`, `PUT`, `PATCH` and
  `DELETE`, including `/api/sync`. The one exception is
  `POST /api/auth/logout` **when the request carries no session cookie**, which
  is not a cookie-authenticated request: it has no session to act on and nothing
  to forge, and refusing it would strand the
  [queued logout](#authentication-while-offline) made by a device whose cookies
  have both expired. With a session cookie present it is checked like any other
  write, which is the ordinary case for that queued logout, since the cookie it
  exists to clear is still in the browser; the app sends the token from the CSRF
  cookie, whose TTL is the session cookie's.
- `/api/admin/*` requires `is_admin`, and non-admins get `404`, so the admin
  surface is not advertised.
- `governor` rate limits on auth endpoints, and per-user limits on search,
  preview, cascade creation, saving and loading a saved search, sync, export, admin uploads, and
  the card, question-list and grades pages, which are the expensive calls. Each
  has a named refill rate in [Configuration](#configuration):
  `DOWNLOAD_RATE_PER_MINUTE` for the one shared bucket over the card,
  question-list and grades pages, `SEARCH_RATE_PER_MINUTE` for search, preview,
  cascade creation, saving a search and loading one — `GET /api/searches/:id`
  reads up to 300,000 word-list rows and returns megabytes, so it is as
  expensive as the preview beside it — `EXPORT_RATE_PER_MINUTE`,
  `ADMIN_UPLOAD_RATE_PER_MINUTE` — a 100 MB body with a 120-second synchronous
  validation behind it is the most expensive call in the plan, admins are few,
  and ten a minute is far more than any real cataloguing session needs — and
  `SYNC_RATE_PER_MINUTE`. The download default of 120 is what this plan's
  download claims rest on: reopening one 300,000-question cascade with two
  active quizzes costs at most about 51 requests (six grades pages per quiz, six
  index-list pages for the non-source quiz, whose list is the only one ever
  fetched, three keys pages and thirty answer pages), so one such reopen
  fits inside a minute; a first sync
  with several kept cascades fills over several minutes behind the download
  progress; and the keys-only pass costs a tenth of the requests, since its
  pages hold ten times as many rows, so ten cascades' keys fit in the bucket
  that one cascade's answers would have filled. In bytes the pass is about a
  quarter of cards without definitions, not a tenth, so it is the request
  count and not the bandwidth that this default is sized against. The buckets are in memory, so every limit in this plan holds per
  backend instance and the deployment's task count multiplies it: with two
  tasks the login limit is effectively twenty failures a minute per username. Until a
  shared limiter is needed, the Terraform module caps the service at two
  tasks and the runbook states the multiplier; moving the buckets to a shared
  store later changes no protocol.
- **The two endpoints that need no session are limited per IP**, by
  `CATALOG_RATE_PER_MINUTE`: `GET /api/lexicons` and
  `GET /api/letter-distributions/:name`, sized for a room of devices behind one
  address rather than for one device (see [Configuration](#configuration)). Every other limit above is per user and
  therefore needs a session, and these two read the database on every call —
  the distribution deliberately straight from `letter_distribution_tiles` and
  never from an in-memory index, so nothing caches it — which would otherwise
  leave an unauthenticated client free to spend the same connection pool every
  signed-in user's sync and search share. They key on the same
  `X-Forwarded-For` entry the login limits do.
- `429` responses carry `Retry-After`, and **every client of a limited endpoint
  honours it**, not only the sync engine: the download manager pauses that
  cascade's fetches until then, keeping its progress indicator as it was, and
  the builder's preview treats a `429` exactly as it treats a `503`. This is
  not an edge case on the download path — the bucket above is deliberately
  sized so that a first sync with several kept cascades fills over several
  minutes, so a `429` is the ordinary signal to wait, and a client that read it
  as a failed page would freeze a download or spin against the limit. **Create
  Cascade, Start over and Save Search… honour it too**, retrying after
  `Retry-After` with the same body exactly as they retry a `5xx`, which their
  device-minted ids make safe; they share the search bucket with the builder's
  preview, and the preview holds two slots back for them (see
  [Creating a cascade](#creating-a-cascade)).
- `TRUSTED_PROXY_HOPS` controls which `X-Forwarded-For` entry per-IP limits key
  on (1 behind the ALB and behind the compose Nginx).
- **Every authenticated request names its account.** The app sends the user
  id it is running as in `X-Wordfall-User`, and the server answers `401` without
  applying or reading anything when that id differs from the session's user or
  is missing, so a cookie replaced by a login in another tab, or planted by a
  login CSRF, can never make one account's device act on another's data (see
  [Authentication while offline](#authentication-while-offline)). Logout and
  `GET /api/auth/me` are exempt, and the export download is authorised by the
  token its header-checked token request issued (see [API](#api)).
- Every cascade, quiz, saved-search, preference and sync query filters by
  `user_id`. Another user's resource returns `404`, not `403`, and in a sync
  operation it is rejected as "not found".

---

## API

All endpoints are JSON under `/api`, except the multipart admin uploads, and a
JSON endpoint refuses a request whose `Content-Type` is not `application/json`
with `415`, which is what keeps a cross-site form, which cannot send that type
without a preflight, from posting to login or registration, the two writes the
CSRF token does not cover. Every request that needs a session also carries
`X-Wordfall-User`, the account the calling tab runs as, and a mismatch with the
session is a `401` (see [Security](#authentication)). Three requests are handled
differently. The export download, a navigation that cannot set a header, is
authorised by the single-use token the header-checked
`POST /api/cascades/:id/export-token` issued. Logout carries no header and is
**exempt**, since it can only clear the cookie it receives and never reads or
writes an account's data, and the queued logout runs, by design, while no tab is
signed in as anyone. And `GET /api/auth/me` is **exempt**: it changes nothing and
reports whose cookie the browser holds, which is exactly what the startup
comparison needs to read and what lets a client holding only the session cookie
recover its CSRF token. Every
endpoint except `auth/*`, `/health`, `GET /api/lexicons` and
`GET /api/letter-distributions/:name` requires a session. Those two are limited
per IP by `CATALOG_RATE_PER_MINUTE` instead of per user (see
[Authentication](#authentication)), since a per-user bucket needs a session.

JSON request bodies are accepted up to `API_MAX_BODY_BYTES` (default 16 MB),
enough for a filter tree holding a 300,000-entry In Word List; Axum's default of
2 MB is not. Admin uploads have their own limit.

### Auth and account

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/api/auth/register` | Create an account; sends the confirmation email |
| `POST` | `/api/auth/confirm-email` | Confirm with the emailed code |
| `POST` | `/api/auth/login` | Start a session |
| `POST` | `/api/auth/logout` | Clear the session cookie **and the CSRF cookie**, so no token outlives the session it was issued with; login re-sets both. The token is stateless, so a copy of it stays valid until its TTL; a user who suspects a copy uses **Sign out everywhere**. This is the one endpoint that needs **neither a session nor a CSRF token** *when no session cookie arrives with it*: there is then nothing to clear and nothing to forge, and it answers `2xx`, so a device offline past both cookies' TTL is not left retrying forever. With a session cookie it is CSRF-checked like any other write, which is the ordinary state of the [queued logout](#authentication-while-offline). It is **exempt from the account-binding `X-Wordfall-User` check**: the queued logout is sent while no account is signed in, so it could never satisfy it, and a `401` would clear its flag with the cookie still alive. The queued logout runs when no account is **signed in**, not when no cookie is held, and the `httpOnly` session cookie it exists to clear is still in the browser, so the app sends the double-submit token with it. Either way the answer is a `2xx`, never a `401`-or-`403` guess |
| `POST` | `/api/auth/reset-password` | Request a reset email |
| `POST` | `/api/auth/reset-password/confirm` | Set a new password with a reset token |
| `GET` | `/api/auth/me` | `{ user_id, username, is_admin, trash_retention_days, max_quiz_questions }` or `401`. Exempt from the account-binding `X-Wordfall-User` check, since it only reports whose session cookie the browser holds, which the startup comparison reads. The retention period and the cap go into the `meta` store, so the Trash page can show purge dates offline and the option forms validate segment size against the server's cap |
| `POST` | `/api/account/sign-out-everywhere` | Body `{ password }`. Bump `session_generation` and re-issue this device's session and CSRF cookies, as a password change does, so every other session is refused from its next request while this one keeps working |
| `POST` | `/api/account/password` | Change the password (current and new); bumps `session_generation` and re-issues this session |
| `DELETE` | `/api/account` | Delete the account (requires password). On a `2xx` the device removes its own local copy of that account, as [Authentication](#authentication) describes |

Preferences are read and written through sync, not a separate endpoint.

### Catalog and search

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/lexicons` | `[{ name, letter_distribution, word_count, leave_count, max_num_anagrams, max_order_rank, max_leave_num_anagrams, max_leave_order_rank }]`, where every `leave_` figure is `null` for a lexicon without leave values. The four maxima are the range [ceilings](#filter-reference) the client cannot derive for itself, all taken from the [index](#derived-attributes): `max_num_anagrams` is the largest `num_anagrams` over the words, and `max_order_rank` the size of the largest length bucket, which is the highest rank Probability Order and Playability Order can reach, with the two `leave_` figures the same over the leaves, by size. The other ceilings need no endpoint: the quiz type gives 15 or 6, `GET /api/letter-distributions/:name` gives the highest tile value behind Point Value's, and `word_count` and `leave_count` are the two limit filters', which rank a whole group's survivors together. Only items loaded by every live instance, per `catalog_instance_status`, are listed, so no listed item can fail on another instance. |
| `GET` | `/api/letter-distributions/:name` | `{ name, tiles: [{ letter, blank_letter, count, value, is_vowel }] }` in tile order (blank first), for parsing, display and the tile palette. Read straight from `letter_distribution_tiles`, never from an in-memory index, so it needs no `catalog_instance_status` gate and a device can fetch a distribution from an instance that is still indexing the lexicons built on it |
| `POST` | `/api/search/preview` | Body `{ lexicon, quiz_type, filters }` → `{ count, sample[], over_cap }`, or `400` with `{ errors: [{ path, field, message }] }`, where `path` is the row's child indexes from the top group |
| `GET` | `/api/searches` | `[{ id, name, quiz_type, word_list_entries, created_at, updated_at }]`: the user's saved searches **without their trees**, with `word_list_entries` the tree's total In Word List entries, so the list **Load Search…** shows stays small however many 300,000-entry lists a user keeps (up to `MAX_SAVED_SEARCHES_PER_USER` of them) |
| `GET` | `/api/searches/:id` | One saved search's whole tree, `{ id, name, quiz_type, filters }`, in the same shape `POST` accepts, In Word List entries included: what Load Search… fetches for the search the user picks, and rebuilds the form from. The `quiz_type` is what lets the builder flag an In Word List row on load. `404` for another user's or a deleted search |
| `POST` | `/api/searches` | Save `{ id, name, quiz_type, filters, overwrite? }`, where `id` is a device-generated UUID minted when Save is pressed and reused for any retry of that press, exactly as for [cascade creation](#cascades-and-sync): a request whose `id` the server already holds for this user returns that saved search unchanged, so a save whose response was lost is not refused on its retry as `name_taken` or `saved_search_limit` by its own first attempt, and an `id` held by another user is refused with `409`. `name` is required and 1–100 Unicode scalar values, otherwise a `400` field error. `quiz_type` is what the entries were canonicalised under and is stored on the spec. **The tree is validated as it is for a preview**, for everything that does not need a lexicon — the [group](#groups) rules, each type's parameters, negation, applicability to `quiz_type`, ranges against that type's floor and its **type-fixed** ceiling (Length, Number of Vowels and Number of Unique Letters at 15 or 6, Consists of at 100), canonical form and length, and the tree's [300,000-entry In Word List total](#filter-reference) — and a failure is `400` with the same `{ errors: [{ path, field, message }] }` shape, never a stored spec. What a save cannot check is whatever depends on the lexicon, since a saved search carries none and can be loaded against any: tile membership in a distribution, and the three ceilings a target sets — Point Value's (15 × the highest tile value), the order filters' (the target's largest bucket) and Number of Anagrams' (the target's largest count). Those rows are flagged on load and refused at creation. The builder also refuses to save while any row is **flagged** (see [Filter applicability](#filter-applicability-by-quiz-type)), naming the row, because a flagged In Word List row saved under the current type would record leave-canonical entries as `anagram`, or the reverse, and nothing afterwards could tell. A name the user already has answers `409` with `{ error: "name_taken" }` unless `overwrite` is `true`, which the UI sends after confirming; the overwrite deletes the replaced saved search and its spec and stores the new one under the request's `id`, so a retried overwrite is recognised by that `id` like any other save. `409` with `{ error: "saved_search_limit", limit, count }` at `MAX_SAVED_SEARCHES_PER_USER`, checked inside the transaction after locking the user's row, as the [cascade limit](#cascade-limit) is, so two simultaneous saves cannot both take the last slot; an overwrite is always allowed, since it takes no new slot |
| `DELETE` | `/api/searches/:id` | Delete a saved search and its spec |

`filters` is the top group. A group is `{ "op": "and" | "or", "children": [] }`
and a child is either a group or a condition, told apart by `op` versus `type`:

```json
{ "op": "and", "children": [
    { "type": "length", "negated": false, "min": 7, "max": 7 },
    { "op": "or", "children": [
        { "type": "includes_letters", "negated": false, "tiles": "Q" },
        { "type": "includes_letters", "negated": false, "tiles": "Z" } ] } ] }
```

Two more conditions:

```json
{ "type": "probability_order", "negated": false,
  "min": 1, "max": 1000, "lax": true }
```

```json
{ "type": "leave_value", "negated": false, "min": 10.0, "max": null }
```

`type` is one of `anagram_match`, `pattern_match`, `subanagram_match`, `length`,
`in_lexicon`, `in_word_list`, `num_vowels`, `includes_letters`,
`probability_order`, `limit_by_probability_order`, `playability_order`,
`limit_by_playability_order`, `num_unique_letters`, `point_value`,
`takes_prefix`, `takes_suffix`, `part_of_speech`, `definition`, `consists_of`,
`num_anagrams`, `front_inner_hook`, `back_inner_hook` or `leave_value`. Only the
parameters that type uses are accepted (the inner hook filters take none); any
extra field is a `400`. Every condition also carries `negated`, a boolean, which
is `true` only for the types the [Filter reference](#filter-reference) marks
negatable. The parameter fields, by type:

| `type` | Fields | Value form |
|---|---|---|
| `anagram_match`, `pattern_match`, `subanagram_match` | `pattern` | pattern tokens in canonical form, single spaces between them (`. W * M . S`) |
| `length`, `num_vowels`, `num_unique_letters`, `point_value`, `num_anagrams` | `min`, `max` | integers |
| `probability_order`, `limit_by_probability_order`, `playability_order`, `limit_by_playability_order` | `min`, `max`, `lax` | integers, and a boolean |
| `consists_of` | `tiles`, `min`, `max` | MAGPIE notation, and integer percentages 0–100 |
| `includes_letters`, `takes_prefix`, `takes_suffix` | `tiles` | MAGPIE notation |
| `in_lexicon` | `lexicon` | the lexicon's **name**, as in the preview and creation bodies; the server resolves it to an id for a cascade's spec and stores the name for a saved search's |
| `in_word_list` | `entries` | an array of entries, each in MAGPIE notation, canonical for the request's `quiz_type` |
| `part_of_speech` | `part_of_speech` | one of `adjective`, `adverb`, `conjunction`, `definite_article`, `indefinite_article`, `interjection`, `noun`, `preposition`, `pronoun`, `verb` |
| `definition` | `text` | free text, 1–500 characters, matched case-insensitively |
| `front_inner_hook`, `back_inner_hook` | — | — |
| `leave_value` | `min`, `max` | numbers, either one `null` for an open bound, not both |

These are the wire names on both sides; the Rust `ConditionKind` may name its
fields as it likes (`min_pct`, `max_pct`), but serialises to this table. The
[contract fixture](#contract-fixtures) that keeps `lib/filters.ts` and the
backend's condition schema in step is generated from this table, so the first
person to write it copies names rather than choosing them.

### Cascades and sync

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/api/cascades` | Body `{ id, source_quiz_id, device_id, at, name, lexicon, quiz_type, clear_threshold, segment_size, progression, require_alphabetical, filters }`. `name` is required, 1–200 Unicode scalar values; the device builds the default summary itself (see [Creating a cascade](#creating-a-cascade)). `device_id` is recorded as `options_device_id` on the cascade and the Source quiz, so the creating device's option changes never compete on timestamps. `id` and `source_quiz_id` are device-generated UUIDs minted when the button is pressed and reused for any retry of that press, never regenerated on retry; a request whose `id` the server already holds for this user returns the existing cascade and Source quiz unchanged, so a lost response never makes a duplicate or takes a second slot, and an `id` or `source_quiz_id` held by another user is refused with `409` (`invalid`), never that user's row. `at` is the device's timestamp, used for `options_changed_at` on the cascade and the Source quiz exactly as operations' `at` is, so the creating device's first option change is never `stale` against a server clock. Runs the search **before opening any transaction**, then in one short transaction locks the user's row, re-checks the [cascade limit](#cascade-limit), stores the question index and shuffles the Source quiz with a server-generated seed, and returns `{ cascade, source_quiz, sync_seq }`. A cheap limit check before the search refuses an over-limit request without spending one, and the lock therefore never covers the search's CPU or its wait for a `SEARCH_CONCURRENCY` permit, so a creation can never block the [purge task](#purge-task) on the same user's row for the length of a search. `source_quiz` carries its `shuffle_seed` rather than its positions: the device derives the Source quiz's order with the shared [shuffle](#deterministic-shuffles) over `idx` 0…count−1, and writes the cascade, the Source quiz and its question rows straight into its base stores from this response, so its first pull finds nothing to rebuild. `sync_seq` is informational; the device does **not** advance its sync cursor from this response, so its next sync pulls the rows the creation stamped. The cascade's private spec is stored with this request's `quiz_type`, which is what an In Word List row's entries were canonicalised under and what Start over copies. `422` if the search is empty or over the cap. `409` with `{ error: "cascade_limit", limit, count }` at the [cascade limit](#cascade-limit). A `5xx`, such as a deadlock or timeout inside the creation transaction, is retried by the device with the same body, which the device-minted ids make safe, and so is a `429`, after `Retry-After`, since this endpoint shares `SEARCH_RATE_PER_MINUTE` with the builder's preview. The threshold and the three [quiz options](#quiz-options) are required: the device fills them from its preferences view and the server never reads the preferences row to supply them, so an omitted one is a `400`. The Source quiz is created with a copy of the options. |
| `POST` | `/api/cascades/:id/start-over` | Body `{ id, source_quiz_id, device_id, at }`, minted per press exactly as for creation and subject to the same repeat, timestamp and `options_device_id` rules. New cascade with a copy of the spec and the same threshold and options; returns the same shape. Subject to the cascade limit. |
| `GET` | `/api/cascades/:id/cards?from=<idx>&limit=<n>&hooks=<0\|1>&definitions=<0\|1>&keys=<0\|1>` | Answer cards `[{ idx, key, answer }]` for question indexes `from`…`from+limit−1` (`limit` ≤ 10,000). The device splits each row between its `questions` store (the `key`) and its `cards` store (the rest), so eviction can take the answer and leave the question (see [On the device](#on-the-device)). With `keys=1` the response is `{ from, keys: [...] }` — the keys alone, in idx order, since the range makes every index implied — and **`limit` ≤ 100,000**, which at about 10 bytes a key is roughly the size of one 10,000-card page: a 300,000-question cascade's keys are three requests where its answers are thirty, a tenth of the cost in the shared rate bucket, and about a quarter of the bytes of cards without definitions (see [Downloads](#on-the-device)). It is what the download manager fetches first, so a cascade becomes playable and self-gradable before its answers arrive; `hooks` and `definitions` are then a `400`. A Leave Value `answer` is a JSON number written as the shortest text that round-trips the stored `f64`, which `JSON.parse` reads back exactly. Card pages are served `Cache-Control: private, no-store`, because the device keeps every byte it fetches in IndexedDB and a second copy in the browser's HTTP cache would sit outside every budget the plan states; only the questions endpoint, whose index lists are small and refetched by the pending path, keeps immutable caching. Responses are compressed. |
| `GET` | `/api/cascades/:id/quizzes/:quiz_id/questions?from=<n>&limit=<n>` | The quiz's question indexes `[idx…]` in idx order, `limit` ≤ 50,000. A quiz's question set never changes, so it is served with the same immutable caching. Fetched when the device materialises a quiz; never part of a pull. `404` for another user's, a purged, or a Source quiz, whose indexes are implied. |
| `GET` | `/api/cascades/:id/quizzes/:quiz_id/grades?from=<idx>&limit=<n>` | The quiz's graded rows for the attempt its row names, which for a cleared quiz is the attempt it finished on, as the parallel arrays a pull uses, in idx order, with the quiz row's `attempt` and `shuffle_seed` so the device can tell whether they belong to the order it holds. `from` is an **index, not an offset**, as on the card pages: the response holds the graded rows whose `question_idx` is in `from … from+limit−1`, `limit` ≤ 50,000, so the device pages the index space by `from += limit` until `from` reaches the cascade's `question_count` and a sparse page — most of a quiz's questions may be ungraded — means nothing is left behind. The questions endpoint below reads `from` the other way, as an offset into a dense list, which is why the two are written `<idx>` and `<n>`. There is no `min_updated_seq` here, which only a pull's fast path needs; the device stamps these rows with the sequence of the sync it made before the fetch. Not cacheable. Fetched when the device materialises a quiz whose rows it dropped on leaving the download window; never part of a pull. If the device's quiz row names a different attempt or seed, the device discards the response, syncs and fetches again. `404` for another user's or a purged quiz. |
| `POST` | `/api/cascades/:id/export-token` | Body: the export's choices, as the query of the export row below. Checks the session, `X-Wordfall-User`, the cascade and quiz, and `EXPORT_RATE_PER_MINUTE`, answering `401`, `404` and `429` with `Retry-After` exactly as any other request does, which a navigation could not show the page; on success returns `{ url }`, the export URL with a token valid for **60 seconds and one use**, bound to this user and these choices: a PASETO under the HKDF-derived export key, which any task can verify and no session validator can decrypt, its use recorded in `export_tokens_spent` (see [Exporting words](#exporting-words)). The dialog navigates a hidden `<iframe>` to it at once, and mints a new one for any retry |
| `GET` | `/api/cascades/:id/export?scope=<cascade\|quiz>&quiz_id=<uuid>&which=<all\|correct\|missed\|ungraded>&format=<txt\|csv>&lines=<answers\|questions>&columns=<list>&order=<study\|alphabetical>&definitions=<0\|1>&hooks=<0\|1>&decimals=<0-3>&token=<token>` | The same file the device builds locally (see [Exporting words](#exporting-words)), streamed as `text/plain` or `text/csv` with a `Content-Disposition` filename. Used when the device doesn't have what the export needs. `token` is the single-use, 60-second token the row above issued for exactly these choices; it is the request's proof of account, since a download navigation cannot send a header, and an expired, reused or mismatched token answers `204 No Content`, which the dialog's hidden frame discards. `404` for another user's or a purged cascade, and for a `quiz_id` that is not this cascade's, has been purged, or is unknown. |
| `POST` | `/api/sync` | Body `{ device_id, app_version, cursor, page_token?, question_rows_for[], ops[] }`, where `app_version` is the frontend build's **monotonic integer build number**, injected into the SPA at build time and compared numerically against `MIN_APP_VERSION` (see [Configuration](#configuration)) — never a hash or a semver string, so "too old to sync" is one comparison and raising the floor is one number in the task definition; the build's git hash is logged beside it for support but decides nothing. `question_rows_for` lists every cascade whose question rows this device holds or wants: in its window and not [budget-dropped](#on-the-device), kept offline, **or whose base already holds question rows** (at most the cascade limit). The third case is what keeps a cascade the drop pass spared for its pending operations from going stale: the device holds its rows, so it must keep receiving their changes. A cascade the device holds no rows for and will not open is left out, and stays pending on the device. It is expected on every non-paged request, an omitted field means an empty list, ids that belong to another user or no longer exist are ignored, and the server omits `quiz_questions` rows for every other cascade. `cursor` is `null` for a full pull (a first sync or a resync), whose operations are pushed like any other's → `{ results: [{ op_id, status, reason?, outcome?, new_quiz_question_count?, new_quiz_questions_hash? }], changes: { cascades[], quizzes[], quiz_questions[], quiz_attempts[], preferences?, tombstones[] }, sync_seq, next_page_token?, resync_required? }`. `outcome` is set on an applied `finish` (a `finish_outcome`) or `finish_segment` (`drilled` or `continued`), and `new_quiz_question_count` and `new_quiz_questions_hash` whenever a quiz was created, reset or restored: the created quiz's when one was created, otherwise the reset or restored quiz's own; a repeated operation returns the recorded fields unchanged; pulled `quizzes[]` rows carry `questions_hash`; `preferences` includes `bindings[]`. |

On the wire, `quiz_questions` changes are grouped per quiz as parallel arrays
(`question_idx[]`, `grade[]`, `graded_at[]`), with no positions: the device
derives the order from the quiz row's `attempt` and `shuffle_seed`. Each group
also carries **`min_updated_seq`**, the lowest `updated_seq` among that group's
rows in this page, as decimal text. One number per quiz per page, rather than
one per row, is what lets the rebase's [fast path](#the-sync-cycle) tell a page
that only echoes this push's own work from one carrying another device's grade,
without adding ten bytes to each of the millions of rows a large first sync
moves. A quiz's rows can span a page break now that a page is bounded by its
total rows, so the device **folds the minimum across every page of the pull**
and the fast path tests that folded value, never the last page's alone.
Every pulled `cascades[]`, `quizzes[]` and `quiz_attempts[]` row carries its own
**`updated_seq`**, and every tombstone its **`seq`**, both as decimal text, for
the same test: the fast path reads that column on those rows exactly as it reads
`min_updated_seq` on the question groups, and the sequence the device stamps its
base rows with is the response's, not the row's, so nothing else on the device
could stand in for it.

An operation on the wire:

```json
{ "id": "0192f0c4-…", "device_seq": 118, "seen_seq": 5120,
  "at": "2026-09-15T14:03:22.418Z",
  "type": "finish",
  "quiz_id": "0192f0a1-…", "attempt": 2, "attempt_seed": "1180591620717411303",
  "shuffle_seed": "9241873301934422881", "new_quiz_id": "0192f0c4-…" }
```

A grade, whose `at` is the one timestamp it carries and becomes `graded_at`:

```json
{ "id": "0192f0c2-…", "device_seq": 117, "seen_seq": 5120,
  "at": "2026-09-15T14:03:19.004Z",
  "type": "grade",
  "quiz_id": "0192f0a1-…", "attempt": 2, "attempt_seed": "1180591620717411303",
  "question_idx": 4411, "grade": "missed" }
```

Seeds are sent as decimal strings, because JSON numbers lose precision above
2⁵³. So are `questions_hash` and `new_quiz_questions_hash`, and the seeds and
hashes on pulled `quizzes[]` and `quiz_attempts[]` rows: **every 64-bit value
on the wire is the decimal text of the unsigned value**, never a JSON number,
and the device stores it as text or `BigInt`, never as a `number`. Postgres has
no unsigned 64-bit type, so every one of these is stored as its
**two's-complement i64** (see [Schema](#schema)) and converted back on the way
out: a seed or hash with its top bit set — about half of them — reads back
negative, and a server that serialised that `i64` would send a value no device
could match. The hash
contract vector fixes the wire form, with one fixture above 2⁶³ for exactly
this. `attempt_seed` is the seed of the attempt
the operation was played under and `shuffle_seed` the seed for what the
operation creates.

A segment descent and an options change on the wire:

```json
{ "id": "0192f0c9-…", "device_seq": 119, "seen_seq": 5121,
  "at": "2026-09-15T14:41:07.902Z",
  "type": "finish_segment",
  "quiz_id": "0192f0a1-…", "attempt": 2, "attempt_seed": "1180591620717411303",
  "segment_end": 100,
  "shuffle_seed": "4519004437287701013", "new_quiz_id": "0192f0c9-…" }
```

```json
{ "id": "0192f0d1-…", "device_seq": 120, "seen_seq": 5121,
  "at": "2026-09-15T14:41:30.117Z",
  "type": "set_quiz_options",
  "quiz_id": "0192f0a1-…", "segment_size": 40 }
```

`set_cascade_options` and `set_quiz_options` carry only the fields that changed,
out of `segment_size`, `progression` and `require_alphabetical`; any other field
is a `400`.

### Admin

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/admin/catalog` | Every distribution, lexicon and leave value set, with sizes, uploader, upload time, reference counts and per-instance load status |
| `POST` | `/api/admin/letter-distributions` | Multipart `name`, `file`. `201` with the distribution, or `400` with `{ errors: [{ line, message }], total_errors }`. |
| `DELETE` | `/api/admin/letter-distributions/:id` | `409` with what still uses it |
| `POST` | `/api/admin/lexicons` | Multipart `name`, `letter_distribution`, `file` |
| `DELETE` | `/api/admin/lexicons/:id` | `409` with what still uses it |
| `POST` | `/api/admin/leave-sets` | Multipart `lexicon`, `file` |
| `DELETE` | `/api/admin/leave-sets/:id` | `409` with what still uses it |

`GET /health` checks the database connection and that every catalog item present
at startup is indexed.

---

## Frontend

SvelteKit static SPA. The routes:

| Route | Page |
|---|---|
| `/` | Landing page (logged out) or redirect to `/cascades` |
| `/register`, `/register/check-email`, `/confirm-email`, `/login`, `/reset-password`, `/reset-password/confirm` | Auth |
| `/cascades` | Cascades page |
| `/cascades/new` | Cascade builder (type, lexicon, filter rows, load/save search, threshold, preview) |
| `/cascades/:id` | Player for the cascade's deepest level, with the ladder panel |
| `/trash` | Cleared quizzes and trashed cascades |
| `/account` | Preferences, controls, sync status, offline storage use with the total rows and keys against the budget and every kept cascade this device still holds data for (automatic or not), its answer size, its key size and its Keep offline toggle, every account with data on this device and **Remove this account's data from this device** for each, password, sign out everywhere, delete account |
| `/admin` | Catalog overview with delete actions (admins only; others get a not-found page) |
| `/admin/letter-distributions/new`, `/admin/lexicons/new`, `/admin/leave-sets/new` | Upload forms with validation error lists |

Modules:

- **`lib/cascade/`**: the cascade rules and the SplitMix64 and Fisher–Yates
  shuffle. Pure TypeScript, tested against the shared test vectors.
- **`lib/local/`**: the IndexedDB schema, migrations and typed accessors. Reads
  go through one accessor that returns the overlay's row when there is one and
  the base's otherwise, so the rest of the app never knows there are two
  stores. Every write the player makes goes through `applyLocally(op)`, which
  writes the changed rows into the overlay, reads and advances the next
  `device_seq` in `meta`, and appends the operation to the
  outbox, all in one IndexedDB transaction, so two tabs can never mint the
  same sequence; the base is written only by the sync
  engine. One tab at a time runs the sync engine and the download manager,
  elected with the Web Locks API (`navigator.locks.request` on a per-user
  lock held for the tab's life); the other tabs read the shared stores and
  show the leader's status. Where Web Locks is missing, every tab syncs and
  idempotency keeps that correct, at the cost of duplicate rebase work.
- **`lib/sync/`**: the sync engine (triggers, push, paged pull, rebase, backoff,
  `401` handling) and the download manager (its own `429` and `Retry-After`
  backoff, yielding to a page the player needs, card pages split into the
  `questions` and `cards` stores with `keys=1` first, the 14-day policy,
  `ROW_STORAGE_BUDGET`, Keep offline, eviction of answers only, and the
  quota-error path).
- **`lib/export/`**: builds a word list or CSV from the local stores, in a worker
  and in chunks, and decides when the export has to come from the server
  instead. The Rust and TypeScript formatters are held to one set of fixtures in
  `contract-fixtures/export/`, so the file a device writes and the file the
  server streams are byte-identical.

Components:

- **`FilterGroup`**: an AND / OR group: its operator switch, its ordered
  children (rows and nested groups), and drag-and-drop between groups. The top
  of the builder is one of these.
- **`FilterRow`**: one component per condition. It is driven by a single
  frontend table (`lib/filters.ts`) giving each type's label, parameter inputs,
  defaults, bounds, whether Not is allowed, and which quiz types it applies to.
  The Leave Value filter uses decimal inputs that may be left blank. The backend
  has its own validation; this table only drives the UI, and a contract test
  keeps the two in step (see [Testing](#testing)).
- **`TilePalette`**: clickable tiles for the current distribution, shown under
  tile inputs when the distribution has any multi-character or non-ASCII tile.
  Tiles are inserted whole and never re-split.
- **`TileText`**: renders MAGPIE notation, drawing multi-character tiles as
  single joined tiles without brackets.
- **`CascadeLadder`**: the levels of a cascade, with sizes, attempts, last
  scores, the current run of any segmented level, which level is current, a
  pending level shown as "waiting for download", and per-level **Quiz
  options** and **Export…** actions. Above twelve levels the ones between the
  Source quiz and the deepest few collapse into one row with their count, behind
  **Show more**, the way `/trash` collapses a chain group: nothing bounds a
  cascade's depth (see [Schema](#schema)), so the panel cannot assume the list
  is short. The compact ladder on the [Cascades page](#cascades-page) shortens
  the same way, keeping Level 1 and the deepest level with `+N more` between
  them.
- **`QuizOptionsForm`**: segment size, progression and alphabetical order, with
  their inline explanations, and the warning when `question_count / segment_size`
  is above 2,000 (see [Quiz options](#quiz-options)). The same component serves the cascade builder, the
  cascade's options dialog and the quiz settings menu; it is told whether it is
  editing a cascade or a quiz and saves through the matching operation. For a
  segment-chain quiz it shows progression and segment size as fixed and offers
  only alphabetical order, and for the Source quiz it hides progression.
- **`ExportDialog`**: the scope, selection, format and column choices of
  [Exporting words](#exporting-words), a live count of the **questions** the
  selection holds — with the entry count beside it only when the cascade's cards
  are complete locally, since an answers export writes one line per word and the
  words live in the `cards` store that eviction empties — and the fallback notice
  when the export needs data the device does not have.
- **`PlayerLayout`**: chooses the desktop layout (side rails around the quiz area)
  or the touch layout (top bar, drawer, touch zones) from `pointer: fine` and
  window width.
- **`QuizArea`**: turns input into the three actions:
  - desktop mouse and wheel events inside the area, and key events
  - touch zones, with tap-versus-drag detection
  - typed-mode protection
  - the 120 ms repeat guard
  - suppressing the context menu, autoscroll and paste

  The rest of the player only ever sees `show_next`, `toggle_grade` and
  `previous`.
- **`Flashcard`**: question, answer and grade display. The font scales for long
  alphagrams and long definitions.
- **`ControlsEditor`**: the binding list per action, the capture box, conflict
  and reserved-stroke notices, and Reset to defaults. It saves through a
  `set_bindings` operation.
- **`TypedAnagramCard`**: answer input, found counter, found and wrong lists,
  give-up and override controls. It checks entries against the card's answer
  list locally, sets aside an entry whose tile count differs from the
  question's without counting it, and, when the quiz requires alphabetical order, against the
  furthest answer entered so far, marking an out-of-order answer and showing the
  hint for what comes next.
- **`AnagramAnswerList`**: shared by both anagram modes. Renders words with
  optional hooks and definitions, and highlights unfound words. Hooks go through
  `TileText` with the case rule under [Answers](#answers), so a multi-character
  tile is never shown in the form that means a blank.
- **`LeaveValue`**: formats a raw value with a sign and the preferred number of
  decimal places.
- **`FinishBanner`**: the non-blocking message after each finish.
- **`SyncStatus`**: Synced, *n* changes waiting to sync, Offline, Log in to
  sync, not enough room on this device, **signed out in another tab** (see
  [Authentication while offline](#authentication-while-offline)), or
  **reload to keep syncing** — the
  `426` state (see [Deployment](#deployment-and-operations)), shown from the
  first `426` until a sync succeeds, because the app never reloads on its own,
  a plain reload is served the old build by the old worker, and its own reload
  action can activate the new one only online, so this state can last
  days and "n changes waiting to sync" would promise a sync that cannot
  happen — plus notices about dropped work.
- **`OfflineBadge`**: Available offline, download progress, "answers need a
  connection" for a cascade whose cards are gone while the device is offline, or
  the Keep offline
  toggle, shown on with a hint for a cascade kept automatically by size, or
  reading "kept automatically once opened" for one this device has not opened
  **or has dropped for the storage budget**, since neither holds rows here;
  the same toggle appears beside each kept cascade on the Account page's
  storage line.
- **`PreferencesMenu`**: the gear menu in the player. It applies changes locally
  through a `set_preferences` operation and updates the current card straight
  away.
- **`WordListEditor`**: paste or upload a file for In Word List, showing the
  count and how many entries are not valid in the lexicon. It recounts whenever
  the quiz type or the lexicon changes, and shows the row's flag when the type
  crosses Leave Value (see
  [Filter applicability](#filter-applicability-by-quiz-type)).
- **`UploadForm`**: shared by the three admin uploads. Shows upload progress,
  then either the created item or the error list.

---

## Configuration

Everything is read from environment variables: a `.env` file locally, ECS task
definition values in production, with secrets from SSM. A malformed value fails
startup rather than falling back to a default.

| Variable | Default | Notes |
|---|---|---|
| `DATABASE_URL` | — | Required |
| `SESSION_SIGNING_KEY` | — | Required; 32 bytes as hex. The PASETO v4.local key for session cookies; the export-token key is derived from it with HKDF (label `wordfall export token`), so there is still one secret to manage and no token of one kind decrypts as the other |
| `BIND_ADDR` | `0.0.0.0:8080` | |
| `SESSION_TTL_SECONDS` | `2592000` (30 days) | |
| `SECURE_COOKIES` | `false` | `true` in any TLS deployment |
| `MAIL_BACKEND` | `console` | `console` or `ses` |
| `MAIL_FROM` | `no-reply@wordfall.local` | |
| `PUBLIC_URL` | `http://localhost:5173` | Base URL for email links |
| `MAX_QUIZ_QUESTIONS` | `300000` | May be lowered; a value above 300,000 fails startup. Lowering it bounds **new** values only: cascades, quizzes and preferences already holding a larger segment size keep working and are never rewritten, and the forms clamp what they prefill (see [Quiz options](#quiz-options)) |
| `MAX_CASCADES_PER_USER` | `100` | Counts cascades in the Trash |
| `MAX_SAVED_SEARCHES_PER_USER` | `200` | Checked inside the save transaction, like the cascade limit |
| `SEARCH_TIMEOUT_MS` | `2000` | |
| `SEARCH_CONCURRENCY` | `2` | Searches running at once per instance, sized to the task's vCPUs. A request waiting longer than `SEARCH_TIMEOUT_MS` for a permit gets `503` with `Retry-After`, never `422` |
| `TRASH_RETENTION_DAYS` | `30` | Y: how long cleared quizzes and trashed cascades stay in the Trash |
| `SYNC_RETENTION_DAYS` | `90` | How long operation records and tombstones are kept |
| `SYNC_MAX_OPS` | `500` | Operations accepted per sync request; a request carrying more is a `400` |
| `MIN_APP_VERSION` | `0` | The lowest frontend build number `/api/sync` accepts. `0` refuses nothing; raising it is a deploy-time change, made only when a sync API change cannot stay compatible, and a request below it gets `426` after its operations are applied |
| `PURGE_INTERVAL_SECONDS` | `3600` | |
| `PURGE_MAX_QUIZZES_PER_USER_PER_RUN` | `5000` | Quizzes one purge transaction takes for one user, oldest `cleared_at` first; the rest wait for the next run, so a Trash holding tens of thousands of chain quizzes never holds that user's row for one long transaction |
| `ADMIN_UPLOAD_MAX_BYTES` | `104857600` (100 MB) | |
| `API_MAX_BODY_BYTES` | `16777216` (16 MB) | JSON request bodies, sized for a 300,000-entry In Word List |
| `SYNC_RATE_PER_MINUTE` | `120` | Per-user sync requests; sized so a full offline attempt drains in minutes |
| `DOWNLOAD_RATE_PER_MINUTE` | `120` | Per-user; the one shared bucket over the card, question-list and grades pages. Sized for one 300,000-question cascade's reopen per minute |
| `SEARCH_RATE_PER_MINUTE` | `30` | Per-user; search, preview, cascade creation, saving a search and loading one (`GET /api/searches/:id`) |
| `EXPORT_RATE_PER_MINUTE` | `10` | Per-user; server-streamed exports, counted when the dialog asks for an export token, since that is the request whose `429` the page can see |
| `ADMIN_UPLOAD_RATE_PER_MINUTE` | `10` | Per-user; the three admin upload endpoints, each of which can carry 100 MB and hold a connection for 120 seconds |
| `CATALOG_RATE_PER_MINUTE` | `300` | **Per IP**, for the two endpoints that need no session: `GET /api/lexicons` and `GET /api/letter-distributions/:name`. A device needs one of each per login plus one distribution per pulled cascade on a new device, but the bucket is per **address**, and a club or tournament room behind one address is dozens of devices logging in at once; 300 covers a room of about a hundred. A larger room waits out `429`s, which every client honours, rather than failing |
| `LOGIN_FAILURES_PER_IP_PER_MINUTE` | `10` | Failed logins per IP; a successful login spends nothing, so a room logging in together is never refused |
| `LOGIN_FAILURES_PER_USERNAME_PER_MINUTE` | `10` | Failed logins per username, checked before any Argon2 verify |
| `AUTH_RATE_PER_IP_PER_MINUTE` | `30` | Per IP; register, confirm-email, reset-password and its confirm. Not `GET /api/auth/me`, which needs a session, and not logout |
| `CATALOG_RECONCILE_SECONDS` | `60` | Fallback reload interval if a notification is missed |
| `TRUSTED_PROXY_HOPS` | `0` | `1` behind the ALB or compose Nginx |

Client-side limits (14-day download window, 500 MB answer storage soft limit,
the 2 GB `ROW_STORAGE_BUDGET` for rows and question keys, the 50,000-row
automatic Keep offline threshold, 30-second sync interval, the 400 ms
`PREVIEW_DEBOUNCE_MS` and the 2-second `PREVIEW_MIN_INTERVAL_MS`) are
constants in `lib/sync/config.ts`.

---

## Development

### One command

```bash
./scripts/dev.py
```

That is the whole setup. With no arguments it brings up a complete Wordfall on
<http://localhost:5173>, seeded with the
[fixture catalog](#the-fixture-catalog) and a confirmed admin account, and opens
the browser. It needs Docker and Python 3.11; everything else is built in
containers. It is safe to run again at any time — each step checks whether it has
already been done.

What it does, in order:

1. **Builds** the backend and frontend images if their inputs changed. The
   frontend is the production-style build, because the Vite dev server does not
   register the service worker and offline studying is half the product.
2. **Brings up `docker compose`**: Postgres, the backend, and Nginx on :5173.
   The backend runs `0001_initial.sql` before it binds.
3. **Waits for `/health`**, which only reports ready once every catalog item in
   the database has been indexed, so there is no race between seeding and using
   the app.
4. **Creates a confirmed user** `dev` through the real registration path:
   `POST /api/auth/register`, the confirmation code read from the console mail
   backend's log, `POST /api/auth/confirm-email`, and only then `is_admin` set
   with SQL, because [no endpoint can](#admin). Registering rather than
   inserting rows is what gives the account its `user_preferences` row stamped
   with its first sync sequence and its default bindings, which a hand-written
   insert would have to reproduce. The password is a fixed passphrase that
   meets the `zxcvbn` rule, `correct-tile-rack-bingo`, printed on every run.
5. **Uploads the catalog** through the real admin API, as that user, skipping
   anything already present. Seeding this way exercises the upload and
   validation paths every time instead of writing rows behind the API's back.
6. **Opens the browser**, unless told not to.

Useful flags:

| Flag | Effect |
|---|---|
| `--distribution NAME=FILE`, `--lexicon NAME:DIST=FILE`, `--leaves LEXICON=FILE` | Upload real catalog files as well as the fixtures. Repeatable. |
| `--no-fixtures` | Seed only what the flags above name. |
| `--hot-reload` | Also run the Vite dev server on :5174, for fast UI work. The service worker is only on :5173. |
| `--reset` | Drop the database volume and the built frontend, then start clean. |
| `--project NAME`, `--port N` | Run an isolated second stack, for comparing two versions or for tests. |
| `--env KEY=VALUE` | Override any [configuration variable](#configuration). Repeatable. |
| `--no-browser`, `--quiet` | For scripts and CI. |
| `--down` | Stop the stack; `--down --volumes` also discards its data. |

So a real-data instance is still one command:

```bash
./scripts/dev.py \
  --distribution English=~/wordgame/english.csv \
  --lexicon CSW24:English=~/wordgame/CSW24.tsv \
  --leaves CSW24=~/wordgame/CSW24_leaves.csv
```

### How it is put together

`dev.py` is a thin command line over `scripts/stack.py`, which is the only place
that knows how to bring a Wordfall up. It has four entry points, each usable on
its own:

| Entry point | What it does |
|---|---|
| `stack.up(project, port, env, build)` | Compose up, then wait for `/health`, with a deadline and the backend's logs on failure. Returns the base URL. |
| `stack.seed(base_url, catalog, user)` | Create the confirmed admin user through registration and confirmation, as step 4 above does, then upload each catalog item through the admin API, skipping what already exists. It honours `429` and `Retry-After` like every other client, since `ADMIN_UPLOAD_RATE_PER_MINUTE` is 10 and the fixture catalog alone is six uploads, so a run with real catalog files added passes the limit. |
| `stack.reset(project)` | `DROP SCHEMA public CASCADE; CREATE SCHEMA public;` and restart the backend. |
| `stack.down(project, volumes)` | Stop the stack, optionally discarding its volumes. |

Everything that needs a running Wordfall goes through these four — `dev.py`, the
[end-to-end tests](#end-to-end-tests), the [scale tests](#scale-tests) and CI —
so there is one definition of "a working instance", and a change to the stack
cannot fix the tests while breaking local development, or the other way round.
See [What the tests reuse](#what-the-tests-reuse).

### Day to day

- **Email.** With `MAIL_BACKEND=console` (the default locally), confirmation and
  reset links appear in `docker compose logs backend`.
- **Offline.** Use :5173, not :5174, then either the browser DevTools **Offline**
  toggle or `docker compose stop backend`, which is indistinguishable to the app
  and leaves the database alone, so restarting picks up where it left off.
- **After editing `0001_initial.sql`**, run `./scripts/dev.py --reset`. SQLx
  refuses to run a migration whose checksum has changed, and the site data in the
  browser has to be cleared too, because local sync cursors no longer match
  anything on the server. `--reset` prints that reminder.
- **Two accounts, one browser** is the quickest way to see that IndexedDB is
  scoped per user; two browser profiles are the quickest way to see sync.
- **Rust and TypeScript rule changes go together.** Both implement
  [the cascade rules](#rule-implementation), and the shared vectors are what
  keeps them honest, so run `make test-unit` before assuming a divergence is a
  sync bug.

---

## Deployment and Operations

- **Terraform** in `infra/` covers:
  - VPC with public subnets (ALB) and private subnets (ECS, RDS)
  - ALB with an ACM certificate, HTTP → HTTPS redirect, and a 120-second idle
    timeout for admin uploads
  - ECS cluster, service and task definition (backend and Nginx containers),
    at **2 vCPU**, which is the figure `SEARCH_CONCURRENCY` defaults to; raising
    the task size means raising it too, and the runbook says so
  - RDS Postgres, with 30-day point-in-time recovery
  - SES domain identity
  - SSM parameters (names only; values are set out of band)
  - CloudWatch log group
- **Deploys** build and push both images, then update the ECS service. The
  backend runs migrations before it binds, and the ALB health check waits for
  `/health`, which includes catalog indexes being loaded, before sending
  traffic.
  - **Security headers:** the content security policy is split between the two
    places that can each express part of it, because neither can hold all of it.
    - **SvelteKit's `kit.csp`, in `hash` mode**, writes a
      `<meta http-equiv="Content-Security-Policy">` into every built page:
      `default-src 'self'; script-src 'self' <the build's hashes>; style-src 'self' 'unsafe-inline'; object-src 'none'`.
      `adapter-static` starts the app from an **inline** bootstrap `<script>`,
      which a plain `script-src 'self'` would block, leaving a blank page online
      and offline alike; the hashes allow exactly that script and nothing else,
      and only the build knows them, so Nginx could not send them. Styles take
      `'unsafe-inline'` because Svelte's transitions insert `<style>` elements
      and the component library's positioning sets inline `style` attributes.
      The script policy is the part that matters: the CSRF cookie is readable
      by scripts by design, so injected script is what it keeps out. It must be
      present in the **SPA fallback page** in particular, because that is the
      `index.html` the service worker serves for every navigation, and
      `adapter-static` renders the fallback separately from prerendered routes,
      so whether `kit.csp` reaches it depends on the SvelteKit version and the
      adapter's options. A missing policy fails **open** — the app boots, nothing
      is logged, and injected script runs — so `npm run build` ends with a check
      over `build/index.html` that fails the build unless it holds that
      `<meta>` policy with `'self'` and at least one hash in `script-src` and no
      `'unsafe-inline'` there.
    - **Nginx sends, on every response**,
      `Content-Security-Policy: frame-ancestors 'self'`, since a `<meta>` policy
      cannot carry `frame-ancestors`, plus `X-Content-Type-Options: nosniff`
      and, in production, `Strict-Transport-Security: max-age=31536000`. The
      browser enforces both policies, and they do not overlap.

    `frame-ancestors` is `'self'` and not `'none'`, and there is no
    `X-Frame-Options: DENY`, because the [server export](#exporting-words)
    downloads through a same-origin hidden frame: the usual "deny all framing"
    hardening would make every server export silently produce nothing. The
    policy still keeps any other site from framing the app.
  - **Caching:** Nginx serves `service-worker.js` and `index.html` with
    `Cache-Control: no-cache`, so devices pick up new app versions, and hashed
    assets as immutable.
  - **Sync compatibility:** a sync API change must stay compatible with the
    previous app version for at least `SYNC_RETENTION_DAYS`, because devices can
    be offline with an old app that long. The sync request carries the app
    version — the build number described under [the endpoint](#cascades-and-sync) —
    and a request below `MIN_APP_VERSION` gets `426` with a
    reload prompt, after its operations have been accepted. The `426` body
    carries the same `results` array as a `200`, and the device drops the
    acknowledged operations from its outbox, then shows
    [`SyncStatus`](#frontend)'s **reload to keep syncing** until a sync
    succeeds. It keeps the rest of the outbox and goes on letting the user
    study, because the `426` applied the operations it carried and more are
    safe to hold. The prompt is not the end of it: the app never reloads on its
    own initiative, the prompt can be dismissed, and a plain reload is answered
    by the old service worker, so the state's reload action runs the
    `SKIP_WAITING` handshake under [App shell](#on-the-device) rather than a bare
    reload; and offline the new worker cannot even be fetched, so the state
    persists until the device is online and the handshake runs.
- **Granting admin** in production is a one-off SQL statement run through a
  bastion or an ECS exec session. It is documented in the runbook.
- **Catalog uploads** happen in the browser at `/admin`. Every running instance
  picks up changes through `LISTEN/NOTIFY`, with no restart or redeploy.
- **Backups** have their own section: see [Backups](#backups).
- **Capacity**: RDS storage autoscaling is on, with an alarm on free storage,
  because large cascades (about 45 MB at 300,000 questions, plus about 30 MB for
  each large level) are the main driver of database growth. Filter specs are the
  second: a 300,000-entry In Word List costs roughly 6 MB of
  `search_condition_words` rows, which is the **most one spec can hold**, since
  the 300,000 entries are a whole tree's allowance and not one row's (see
  [In Word List](#filter-reference)), and `MAX_CASCADES_PER_USER` and
  `MAX_SAVED_SEARCHES_PER_USER` are what bound how many of them one account can
  hold: about 1.8 GB at both limits, which is what the free-storage alarm is set
  against. `sync_operations` would have outgrown both: studying emits about two
  operations per card (a grade and, online, its own cursor move), at roughly
  250 bytes a record with its indexes, so keeping every record for 90 days
  would hold about 150 MB for each 300,000-card pass, ten times the cascade it
  describes. Records of result-free operations are therefore deleted as soon as
  their device has acknowledged them (see [The sync cycle](#the-sync-cycle)), so
  the table holds about one batch per active device plus a few `finish`,
  `finish_segment` and `restore_quiz` records per attempt. Task memory is sized
  to the catalog, and each index's size is visible in `/admin`.
- **Monitoring**: sync rejections are counted by type and reason in logs and
  graphed. A rise in rejections outside the expected `stale_attempt`, `stale`,
  `not_active` and `not_deepest` cases, which ordinary two-device use
  produces — together with the `not_found` rejections that follow one of those in
  the same batch, when the level behind the rejected operation never reached the
  server (see [Conflicts](#conflicts)) — points
  to a divergence between the Rust and TypeScript rules, and any `error`
  rejection alarms, since it means a database error the rules did not expect.

---

## Backups

Everything that matters is in Postgres. The catalog can be uploaded again from
the admin's own files, and the frontend is rebuilt from the repository, so the
database is the only thing whose loss could not be undone.

### What is backed up

| Layer | What | Where | Kept |
|---|---|---|---|
| **Point in time** | RDS automated backups with PITR | The RDS backup store, same region | 30 days |
| **Nightly dump** | `pg_dump --format=custom` of the whole database | An encrypted, versioned, object-locked S3 bucket in a second region | 90 days, then a monthly dump for a year |
| **Schema only** | `pg_dump --schema-only`, committed nowhere but attached to each release | The same bucket | With its dump |

The nightly dump runs as a scheduled Fargate task using the same image as the
backend, with a read-only database role. It writes to a key named for the day,
so a corrupted dump cannot overwrite a good one, and the bucket's versioning and
object lock mean neither can a mistake or a compromised task role.

Uploaded catalog files are **not** backed up. They are licensed data that
Wordfall never stores as files, only as rows, and those rows are in the dump.

### Alarms

- No successful dump in **36 hours**.
- A dump more than 30% smaller than the previous one, which catches a truncated
  or partial run.
- RDS free storage below its threshold, since a full disk stops backups as well
  as writes ([Capacity](#deployment-and-operations)).

### Restoring

The runbook has the full procedure; the parts specific to Wordfall are:

1. **Restore into a new instance**, never over the live one, and point a staging
   backend at it to check `/health` and a few cascades before any cutover.
2. **Re-index the catalog**: `/health` stays not-ready until every lexicon and
   leave value set has been built into memory, which takes a few seconds per
   item. Nothing has to be re-uploaded.
3. **Force a resync.** This is the one step that is easy to forget and impossible
   to skip. A restore to an earlier point makes the server's sync sequences go
   backwards, so a device that synced after the restore point has a cursor at
   or ahead of the server's and would silently ignore rows it should pull, and
   its pending operations carry a `seen_seq` the server would call `invalid`.
   Setting the floor to the restored `sync_seq` would catch only the devices
   that do not need it. After a restore, set every user's `sync_seq` **and**
   `sync_floor_seq` to `sync_seq + 2^32`, a value above any cursor a device
   could hold (the constant needs no knowledge of the lost state, and a
   `BIGINT` has room for it). Every device's cursor is then below the floor,
   so its next sync answers [`resync_required`](#the-sync-cycle) and it
   rebuilds its local state from the server; every row stamped from now on is
   above every old cursor; and no pending operation's `seen_seq` exceeds
   `sync_seq`, so the push in that same request is applied first, exactly as
   the sync cycle describes. `scripts/restore.py` performs this step.
4. **Expect some lost work**, and say so. Devices keep their outbox, so anything
   a device had not yet pushed is pushed again after the resync and survives.
   Work that was pushed after the restore point and only lived on the server is
   gone.

### The drill

A restore drill runs **quarterly** and is not considered done until the restored
instance serves a real cascade. It restores the latest nightly dump into a
throwaway instance, runs the [end-to-end suite](#end-to-end-tests) against it,
performs the sequence-bump step, and checks with a second browser profile whose
cursor is **ahead** of the restored server and whose outbox holds unsent grades
that the grades are applied, the device resyncs rather than diverging, and no
operation is rejected as `invalid`. The date and the
measured restore time go in the runbook. `scripts/restore.py` performs the
mechanical parts, and `./scripts/dev.py --env DATABASE_URL=…` is how the drill
points a local stack at the restored database.

### Locally

`scripts/backup.py` and `scripts/restore.py` work against any Wordfall,
including the development stack, so the same code paths that run nightly in
production are the ones used to snapshot a local database before a risky
migration edit.

---

## Testing

Five layers, each with a job the others can't do:

| Layer | Runs with | Needs | Covers |
|---|---|---|---|
| **[Unit](#unit-tests)** | `make test-unit` | Nothing | The search engine, tiles, probability, the cascade rules on both sides, and the export formatters |
| **[Integration](#integration-tests)** | `make test-integration` | Postgres | The real router and the real schema: auth, admin, uploads, cascades, sync, purges |
| **[End-to-end](#end-to-end-tests)** | `make test-e2e` | The whole stack | What a person actually does, including offline and two devices |
| **[Scale](#scale-tests)** | `make test-scale` | Postgres | 300,000 questions end to end, within a time budget |
| **[Parity](#zyzzyva-parity)** | `make test-parity` | Licensed data, local only | That a search returns what Zyzzyva returns |

Two things are shared by every layer and by
[local development](#development): the fixture catalog and the contract
fixtures.

### The fixture catalog

`fixtures/catalog/` is a small, hand-built catalog committed to the repository.
It contains made-up and public-domain words only, **no licensed data**:

- an English-style distribution
- a Catalan-style distribution with multi-character tiles (`NY`, `QU`, `L·L`)
  and `Ç`
- a lexicon and a leave value set for each

It is small enough to index in milliseconds and odd enough to catch the tile
bugs a pure A–Z lexicon would hide. The same files are parsed directly by the
backend unit tests, uploaded through the admin API by `scripts/stack.py` when it
seeds, and therefore present in every end-to-end run **and** in every
`./scripts/dev.py`. So the catalog a developer clicks around in is the catalog
the tests assert on, and a fixture that stops parsing breaks both at once.

### Contract fixtures

`contract-fixtures/` holds the JSON that keeps the Rust and TypeScript sides
from drifting. Each file is loaded by a `cargo test` and by a Vitest, and neither
side is allowed to generate the file it is checked against.

- **Shared rule and shuffle vectors** (`contract-fixtures/cascade/`): sequences
  of operations with the expected cascade state after each one, and seeds with
  their expected permutations. Both `cargo test` and the frontend unit tests
  (Vitest) must pass every vector. Together they cover:
  - all five finish outcomes, including exactly-at-threshold scores and
    thresholds of 1 and 100
  - climbing back up
  - **The Source quiz**: a finish with misses at any score resetting it and
    descending, under Ladder and under Drill; a finish with no misses
    completing the cascade and leaving it playable at Level 1; a cascade
    completed a second time; and the Source quiz never appearing in the Trash
  - restoring into live, complete and trashed cascades
  - purges
  - **Drill progression**: a quiz replaced below the threshold, one cleared at or
    above it, the nothing-correct reset, and a cascade completed with the Source
    quiz plus one drill level
  - **Segments**: run boundaries for sizes that do and don't divide the question
    count; a run with no misses creating nothing; a run with misses creating a
    Drill quiz one level down and leaving the cursor at the boundary; drilling
    that quiz down to nothing and coming back to the right run; a segment
    descent inside a Ladder cascade and inside a Drill one; a segment size at or
    above the question count behaving like 0; `next_boundary` returning None
    for a segment size of 0, one at or above the question count, and a
    segment-chain quiz, with no division by the size; a segment size of 4
    refused on both sides while 0 and 5 are accepted; the run number, total and
    within-run position for a uniform attempt and again after a mid-attempt size
    change (250 questions, `run_start` 100, S switched to 30, reading
    `run 4 of 9 · 1 of 20`), and on the last run of a 250-question quiz with
    S = 100, where `next_boundary` is None and the length comes from the
    question count, reading `run 3 of 3 · 1 of 50`, for a size that divides the
    count and one that does not; segments turned on at cursor 200 of a
    250-question quiz whose `run_start` is still 0, reading
    `run 1 of 5 · 201 of 250`, where `next_boundary` is None although four
    boundaries lie inside the quiz, so the numbering rule's reading of a
    mid-attempt change is fixed by a vector rather than discovered; an attempt of 40 questions with S = 5 and every run
    missing something creating exactly 8 chain quizzes, the
    `ceil(question_count / S)` bound; the segment size changing
    mid-attempt, including to a value that puts the next boundary before a
    boundary already descended
  - **Options**: every new quiz taking the cascade's options, a change to a quiz
    not touching the cascade or its siblings, a change to the cascade not
    touching existing quizzes, and a segment chain staying Drill and unsegmented
    in a Ladder cascade through two replacements; a quiz switched to Drill in a
    Ladder cascade finishing as Drill, and a quiz switched to Ladder in a Drill
    cascade descending; and a `set_quiz_options` on a waiting upper-level quiz,
    with the boundary it produces when the user climbs back to it
  - **Restore**: a restore onto a mid-attempt quiz leaving that quiz's attempt,
    grades and cursor untouched; the restored quiz's attempt number going up by
    one and its order coming from `seed` unmodified; an ordinary quiz restored
    after the cascade's segment size changed taking the new size; a restore
    into a Drill cascade; a restore on top of a segment-chain deepest level,
    with the chain resuming its parent's run after the restored quiz clears; a
    restored
    segment-chain
    quiz staying Drill with no segments in a Ladder cascade and returning to
    the level above it when cleared
  - **Run starts**: `run_start` following each boundary passed, drilled or not;
    a size change after a descent starting the next run at `run_start`, not at
    the previous multiple of the new size; Previous refusing to go below
    `run_start`; a `finish_segment` at or below the cursor being rejected;
    `run_start` of 100 on a 250-question quiz surviving a change of the segment
    size to 0 and to 300,000, with `next_boundary` None and Previous still
    refused below 100 on both sides, so neither module clears it; and
    `run_start` 119, reached with S = 7 on a 300-question quiz, with the size
    then set to 120, giving a next boundary of 120 and a run of one question,
    which both modules compute alike and neither refuses
  - **Attempt seeds**: a grade, cursor move or run finish naming a seed the
    server does not have being rejected as `stale_attempt`; and a replay of an
    outbox holding 1,200 operations with a `finish` at position 700, synced in
    three batches, losing nothing
  - **Completion counts**: `peak_depth` and `attempts_since_completion` across
    two completions, the completing attempt included in the count, a
    segmented Source quiz whose drilled misses still make the finish a descent,
    and `peak_depth` rising to 2 on a restore into a completed cascade and to
    3 on a segment descent below a restored quiz
  - **Leave value text**: `0.15` → `0.2` and `0.25` → `0.3` at one place,
    `2.5` → `3` at none, `-0.04` → `0.0` and `0.04` → `0.0` with no sign either
    way, `±1000000` at three places, the upload bound, where the rounded integer
    is the largest either side ever formats, and the exact export form with
    no `+`
  - **Question hashes**: `questions_hash` for a one-question quiz, a
    300,000-question Source quiz, and two three-question sets that share a
    count but differ in one index, each with its exact wire form as decimal
    text; every fixture hash and seed is above 2⁵³, so a side that parsed one
    as a JSON number would fail the vector, and **one hash and one attempt seed
    are above 2⁶³**, so a side that treated the stored `i64` as the value would
    fail it too
- **Contract test**: the frontend filter table and the backend condition schema
  are both generated or checked against one shared JSON fixture in
  `contract-fixtures/`, itself generated from the
  [parameter table](#catalog-and-search) under the API's filter JSON, so a new
  filter parameter cannot be added on one side only and no field name is chosen
  twice. The fixture holds one valid condition per type, serialised by each
  side and compared byte for byte, and one with an extra field, which both
  sides refuse.

### Unit tests

Pure functions and in-memory structures, no database and no network.

**Backend** (`cargo test --lib`), against [the fixture catalog](#the-fixture-catalog):

- **Search engine**:
  - Every example in Zyzzyva's search help, recreated with a fixture that
    contains the example words: `ETX.`, `PI..Z`, `Z[AEIOU][AEIOU]`, `*JBX`,
    `AT..`, `.W*M.S`, `LX[AU]`, the Includes-Letters Q-not-U case, a two-tile
    `Not Includes AB` row matching words that lack either tile,
    `Consists of AEIOU 70–100`, and the lax tie cases. Zyzzyva writes its
    single-tile wildcard as `?`; Wordfall writes it as `.`, so the examples are
    translated.
  - Front Inner Hook and Back Inner Hook against a fixture holding SPORT, PORT
    and SPORTS, including a one-tile word and the negated forms.
  - Groups: `Length 7 AND (Includes Q OR Includes Z)`, an OR of two AND groups,
    a limit inside an AND group ranking that group's result within the
    parent's predicate survivors (`Length 7 AND (Includes V AND Limit 1–50)`
    giving the 50 most probable 7s with a V), a limit-only nested group under
    an AND parent equal to the same limit beside the parent's rows, a limit in
    a child of an OR group ranking within the OR group's candidates, a nested
    group with a limit under a top-level Length row giving the same result with
    and without the candidate shortcut, **a group holding both limit kinds**
    (`Length 7` with `Limit by Probability Order 1–50` and
    `Limit by Playability Order 1–10`) returning the intersection of the two
    slices — not the ten most playable of the fifty most probable, nor the
    reverse — and returning the same set when the two rows are swapped, and
    every validation error (empty
    group, depth over 4, more than 100 rows, and 100 rows each wrapped in their
    own group refused as 101 groups with a field error rather than reaching the
    insert).
  - Leave Value: inclusive bounds, open bounds, negative values, and a value
    exactly equal to a bound.
  - Leaves: `.` in a leave pattern matching the blank, a literal `?` matching
    only the blank, leave probability with the blank counted as an ordinary
    tile at the bag's blank count, Number of Anagrams 0 for a leave holding a
    blank, Number of Vowels not counting the blank, Consists of with and
    without `?` in the set, In Word List entries put in canonical leave order
    with invalid entries ignored, and In Lexicon negated against a second
    fixture lexicon on the same distribution.
  - Limits: a group holding only a limit row ranking every candidate; a limit
    in an OR group ranking the union; a lax row widening freely, a strict row
    not widening, and a strict row capping a lax one, each against the same
    ties as Zyzzyva's `WordEngine`; a min past the last survivor giving an empty
    result; and limited words collapsing to fewer alphagram questions than the
    range.
  - Validation, one case per message: min above max; a range that narrows
    nothing, measured against each filter's own floor, so `Length 1–15`,
    `Probability Order 1–N` and `Consists of 0–100` are refused at their
    defaults while `Length 2–15` and `Length 1–14` are accepted, where **N is
    the fixture's largest length bucket, not its word count**: the same row
    taken to the word count is a field error naming that bucket, since no
    per-length rank reaches it, while `Limit by Probability Order 1–<word
    count>` is the row that narrows nothing at the word count, being ranked
    over a group's survivors instead, and the two figures the builder reads
    them from are `max_order_rank` and `word_count` on
    [`GET /api/lexicons`](#catalog-and-search); a
    **Number of Anagrams** row opening at 0 and the largest `num_anagrams` in
    the target, refused at those defaults and accepted one below the ceiling,
    with a bound above it a field error naming the ceiling, on a word target
    and on a leave target whose own largest count is lower; the same
    check against the **quiz type's own** ceiling, so on a Leave Value cascade
    `Length 1–6` and `Number of Vowels 0–6` narrow nothing and are refused
    where `Length 1–15` is refused for a different reason, its bound being
    above that ceiling, and `Length 1–5` is accepted; a Length or Point Value bound above a Leave Value cascade's
    ceiling, `?` in a word pattern or in a word cascade's tile list, a Leave
    Value row with both bounds blank, a
    filter that does not apply to the quiz type, an In Lexicon row naming a
    lexicon on another distribution, **four In Word List rows whose entries
    total 300,001** refused on the row that crosses the total while the same
    four totalling exactly 300,000 are accepted, a pattern that is not in
    canonical form, and a canonical pattern past the stored length — fifteen positions
    each holding a set of the distribution's consonants — reported as a field
    error naming the limit, while one just under it is accepted and stored.
  - The search deadline: a scan over a fixture large enough to exceed a 1 ms
    `SEARCH_TIMEOUT_MS` stops and reports "search too broad".
- **Tile handling**:
  - `A[NY]S` and typed `ANYS` both parse to three tiles.
  - Palette-inserted `N` + `Y` stay two tiles.
  - `N Y`, `[A NY]` and `[ANY]` tokenise as the canonical form says, and a
    non-canonical pattern is rejected by the server.
  - Malformed notation (`[`, `[]`, `[A]`, nested brackets) is rejected, as in
    MAGPIE's `ld_str_to_mls`.
  - Sort order follows the distribution file, with the blank first.
  - Vowel and point counts use the distribution.
- **Property tests** (`proptest`):
  - The shortcut candidate paths return the same results as a full scan, on a
    word target through the per-length buckets and the alphagram map and on a
    leave target through the per-size buckets and the canonical-leave lookup.
  - Anagram Match without wildcards returns exactly the alphagram map entry.
  - Negating a predicate partitions the candidates.
  - An OR group returns the union of its children's results, an AND group the
    intersection, and a limit inside a group is a subset of that group's
    unlimited result.
  - Limit ranges are subsets of the unlimited results.
  - Over random sequences of legal operations chosen from the current state
    (grades, cursor moves, run finishes at boundaries, segment-size changes
    including 0 and values past the question count, finishes, restores of
    ordinary and chain quizzes, trashing and restoring the cascade), the
    cascade stack stays valid: the active levels are exactly `1..depth`, Level
    1 is always the Source quiz, only the deepest level is played on a device, every
    level's questions come from the Source quiz, `run_start` never exceeds the
    cursor, a passed run never descends again, a chain quiz is never segmented
    and stays Drill, `origin_segment_end` is unique per parent attempt, and
    every quiz's rows hash to its `questions_hash`. A Vitest beside it checks
    the default-name summary for every filter type and a name cut at exactly
    199 scalar values plus `…`. The
    generator is seeded and shared with a Vitest that runs the same sequences
    through the TypeScript rules, so a divergence between the two modules shows
    up here rather than as sync rejections in production.
- **Probability tests** check `combinations` against brute-force enumeration of
  every draw from a small bag that spells the word with up to two blanks, for
  bags holding two, one and no blanks, and check that ranks, minimum ranks and
  maximum ranks are consistent on ties.

**Frontend** (`npm run check`, then Vitest): `lib/cascade` against the shared
rule and shuffle vectors, `lib/local` against `fake-indexeddb`, `lib/sync`'s
rebase and backoff, and `lib/export`'s formatters against the export fixtures.
A Vitest over the cascade builder's preview feeds it a stubbed `503` with
`Retry-After` and asserts that the last count stays on screen with "the server
is busy" beside it, that one retry follows, and that no filter row is marked in
error, then the same for a stubbed `429`; that a run of keystrokes 100 ms
apart produces one request per `PREVIEW_DEBOUNCE_MS` rather than one per
keystroke; and that a run of keystrokes **500 ms** apart — past the debounce on
every pause — still produces one request per `PREVIEW_MIN_INTERVAL_MS`, so a
minute of that typing stays at or under `SEARCH_RATE_PER_MINUTE`, with the last
edit's count on screen when the run ends; that the preview stops at all but two
of that minute's slots and reads "the server is busy" rather than spending them,
so a **Create Cascade** pressed straight after a minute of editing is not
refused; that **two builder instances** sharing one stubbed bucket, each keeping
its own reserve, can still spend it between them, since neither sees the other's
requests; and that a stubbed `429` on Create Cascade is retried after
`Retry-After` with the same body and the same ids, creating one cascade, with
the button showing the busy state meanwhile; the server side of that path is asserted in-process under
[Integration tests](#integration-tests), because holding a search permit open
long enough to force the wait needs a hook the end-to-end stack must not have.
Another covers the filter rows' [ceilings](#filter-reference): a Number of
Anagrams row and a Probability Order row opening at `max_num_anagrams` and
`max_order_rank` from a stubbed `GET /api/lexicons`, a Point Value row at 15 ×
the highest tile value of the stubbed distribution, and a Limit by Probability
Order row at `word_count`, with each refused as narrowing nothing at that
default and each reporting the same ceiling in its field error one above it, so
a builder that reached for `word_count` on all four would fail on two rows.
Another renders `CascadeLadder` and the Cascades page's compact ladder for a
cascade of 30 levels, built by repeatedly missing one question, and asserts that
both collapse the middle levels behind a count with Level 1 and the deepest
level still shown, that **Show more** expands them, and that a 12-level cascade
collapses nothing.
Another covers the In Word List row across a quiz-type change: a list of leaves
stored in canonical order is flagged and recounted as invalid when the type
becomes Anagram, a list of 7-tile words is flagged and recounted when it becomes
Leave Value, and a change between Anagram and Definition flags nothing and
leaves the count alone; and the same on the load path, where a saved search whose
spec records `leave_value` is flagged the moment it loads into an Anagram
builder, from the stored type alone, with no change having happened in the
form; and **Save Search… refused while that row is flagged**, naming the row,
so leave-canonical entries can never be stored under `anagram`.
The `lib/local` tests include two writers in one `fake-indexeddb` never
minting the same `device_seq`, a write from a second tab during a rebase
landing on the rebuilt overlay, and a fixture database at each earlier schema
version, holding an outbox and an overlay, opening at the current version with
every row intact. They also cover the unscoped database: two accounts signing in
leaving two `accounts` rows and one `signed_in` pointer, a logout clearing the
pointer and no row, a startup with no pointer opening no per-user database,
removing an account's data with **no** pending logout deleting its row and its
database while the other account's survive, and the whole path running with
`indexedDB.databases()` deleted from the global, since Safari and Firefox lack
it. They also assert that the storage line's figures for an account that is not
signed in come from its `accounts` row with **no `open` on its database**, and
cover the queued logout end to end: removing the data of an account that
**does** hold an unacknowledged logout leaving a row carrying its id, username
and flag alone, absent from the storage line, and deleted once the request goes
through; the flag clearing on a `2xx`, on a `401` and on a `403` — the class is
any `4xx` but `429`, and a `403` is what a browser that cleared only its CSRF
cookie would meet — while a `429`, a `503` and a network error each leave it
set; the retry carrying the **CSRF token from the cookie** whenever that cookie
is there, since the `httpOnly` session cookie goes with the request whether the
app wants it to or not, and carrying none once both cookies have expired; the
retry being sent at a startup with no pointer and **not** sent while an account
is signed in; and a second account's login clearing every pending flag, so the
queued logout never reaches the server after the cookie it was for was
replaced. A fixture unscoped
database at each earlier version opens at the current one with its rows and
pointer intact.
The rebase tests cover: a rejected `finish` removing the quiz it created in
the overlay; rejected grades on a trashed cascade disappearing from the overlay
although no server row changed; an applied `finish` with a different outcome
kind showing the notice and one differing only in question count staying
silent; a pulled quiz with a new seed being rebuilt; a pull that only echoes the
device's own operations touching a handful of overlay rows and no base rows; the
outbox keeping one `move_cursor` per quiz, re-appended after a queued
`finish_segment`; the engine draining a 600-batch outbox back to back; a pull
arriving while a preferences change is pending leaving the view alone, and a
stale-rejected preferences change reverting it; a second device building a
Source quiz from a pull that carries no question rows; a reset pulled with no
graded rows completing its rebuild at the last page; a cascade outside the
download window fetching its index lists and materialising on first open, and,
when it leaves the window with nothing pending, dropping every question row,
position and card while keeping its cascade, quiz and attempt rows, then on
reopen fetching index lists and graded rows and materialising with every
grade intact; a cascade created while a full pull is in flight surviving the
last page with its grades; a level promoted from an acknowledged `finish`
while a full pull is in flight surviving the last page; the staging swap
surviving a browser kill between two cascades' transactions, with the cursor
unadvanced and the next pull converging; a pull for an out-of-window cascade
writing no question rows and leaving its quizzes pending; a grades fetch whose
attempt differs from the base row being discarded and the device syncing
again; a grades fetch cut off after its first page leaving the quiz **pending**
and fetched again, because the rows it wrote fall short of the quiz row's
`correct_count + missed_count`, while the same fetch run to `question_count`
clears the flag in one pass; the 1,200-operation outbox vector with its `finish` dropped in replay
showing one `stale_attempt` notice counting the grades behind it; the drop
pass removing a 300,000-question cascade's rows within budget; a full
segmented attempt pushed as the device emits it producing no `bad_cursor` or
`bad_segment` rejection and its `finish` applied, not `ungraded`; an
out-of-window cascade re-entering the window through a pull ending with every
grade present, its pending mark surviving the index-list fetch until the
grades arrive; grade rows arriving in a pull for a pending quiz being written
nowhere and the quiz staying pending until the full sequence runs; a new
unplayed quiz materialising with an index-list fetch and no grades request,
and the drop pass leaving such a quiz unflagged; a cascade active on another
device but never opened here holding no question rows and no cards until first
open, one over the automatic keep threshold on such a device keeping nothing
until then, a cascade opened here 15 days ago and studied elsewhere
yesterday leaving this device's window, a cascade studied continuously for 15
days staying in the window and never dropped under an open player, and an
export of a never-opened cascade counting as an open; an out-of-window
cascade whose rows the drop pass spared for its pending operations staying in
`question_rows_for`, receiving another device's grade on those rows, and
having no quiz flagged pending, then surviving a resync with its rows and its
`finish` intact; an
offline restore of a quiz with no local rows leaving it
pending until first open; a cascade whose large cleared level was exported
from the Trash (its rows fetched for that) leaving the window, the cleared
rows dropped, and a second Trash export fetching them again; a Trash export of
a cleared quiz holding only part of its rows, or its rows without positions,
or a set that fails the hash check, fetching them before it writes anything;
a pull that reports a pending quiz cleared clearing its flag; eviction taking an automatically kept cascade's answers while
sparing its rows, its question keys and a user-kept cascade's answers, and taking
them in **last-opened order, oldest first** — three cascades over the limit with
distinct last-open times, the oldest losing its answers first and the one opened
this morning last, with a user-kept cascade skipped even when it is the oldest of
all — with the
player then showing that cascade's questions from the `questions` store and
"answer needs a connection" on the reveal, grading still saved, and a
questions-only export of it still written from local data; a **typed** anagram
card of that cascade falling back to flashcard mode, its grade starting Correct
and Toggle deciding it, rather than every entry counting wrong; a card whose key
never arrived, from a download stopped part way, reading "this question needs a
connection", emitting no grade, and its level marked still downloading, with a
questions-only export of that cascade falling back to the server, and the last
card of that card's run emitting no `finish_segment` while the player names how
many questions still need a connection; the download
manager fetching every in-window cascade's keys with `keys=1` before any
cascade's answers, and a restored quiz's per-quiz sequence fetching the
cascade's missing keys too; a tombstone and a full pull's deletion each removing
a cascade's `questions` and `cards` entries with its rows, and its `meta`
entries too — its last-open time, its Keep offline id, its automatic-keep
opt-out and its budget-dropped mark — so the Account page's storage line stops
listing it; the drop pass
removing a cascade's question keys along with its rows, and the next open
fetching both back from the card pages;
a quiz cleared offline getting a provisional `cleared_at` from the operation's
`at`, and a cascade trashed offline a provisional `trashed_at`, both entries
reading "purges 30 days after this syncs", the trashed cascade still listed in
the Trash while it is gone from the Cascades page, and the acknowledged rows
replacing both; the drop pass dropping the least recently opened unkept cascade
when the base's rows and keys pass `ROW_STORAGE_BUDGET`, before its 14 days are
up, and skipping one that holds pending operations or has its player open; the
same pass taking its two tiers in order, so an **automatically kept** cascade
is dropped only once every unkept one is gone and then oldest first, a
**user-kept** one is never dropped at all, and a base whose user-kept cascades
alone exceed the budget ends the pass still over it with the storage line
saying so rather than looping; a cascade the budget dropped **while it was
still inside the window** being marked in `meta` and left alone by the next
pull's download manager — no index list, no grades, no keys, no cards, absent
from `question_rows_for`, its quizzes pending — so the pass settles in one run
instead of dropping and refetching the same cascade forever, then opening it
clearing the mark and fetching it back whole, and **turning Keep offline on for
a marked cascade** doing the same without opening its player, since that counts
as an open (see [Downloads](#on-the-device)) and the pass may never drop a
user-kept cascade again, and the same mark left by the
quota path's drop so its retry refetches nothing; a `QuotaExceededError` thrown by a pull's per-cascade transaction
leaving the cursor unadvanced and the outbox intact, running the drop pass **and
then the answer eviction**, in that order, and
retrying once, and on a second failure showing "not enough room on this
device" while studying continues; the same error thrown by an **`applyLocally`**
instead, where the grade reached neither the overlay nor the outbox: the player
stays on that card with the storage message in place of the grade, Show / Next
advances nothing, and the grade is written once a cascade is removed and the
retry succeeds — and a base whose cascades are all **user-kept** reaching that
state with both passes freeing nothing, so the message is the only outcome; the download manager reading a missing
distribution's tiles from the `distributions` store and never from `meta`; a page of a pull written while the player is on
the quiz changing nothing the player reads until the last page; a `finish`
rejected `ungraded` after a `not_found` grade dropping the finish and leaving
the missing questions to grade; an acknowledged offline
`finish` leaving its new level playable with no network request, a
same-count, different-set promotion refused by `new_quiz_questions_hash` and
refetched with the notice, and a device-created quiz never triggering an
index-list fetch after acknowledgement; a `finish` the device computed as
`cleared` that the server applied as `descended` leaving the finished quiz
rebuilt from the server's seed with no grades, the reverse leaving the base's
graded rows untouched with no fetch, and a `finish_segment` computed as
`drilled` but applied as `continued` dropping the drill quiz with no request,
and the same for a `finish` the server answered `completed` or `reshuffled`
where the device had built a new level — the `completed` case showing the
came-out-differently notice and the Complete badge from the pulled row and
**no completion screen**, since the counters it reports were reset by that
same finish; a completion this device computed while three finishes another
device had made were still unpulled showing **this device's own** counts on the
screen, with no notice, since the outcome matched, and nothing fetching the
server's higher numbers afterwards;
a `finish` acknowledged on its second send, after a lost response, promoting
its rows with no notice; the fast path taking a batch of applied grades
without touching the other pending grades' overlay rows or replaying them,
decided from each pulled question group's `min_updated_seq` and not from the
grade values, so the full path still runs for a batch whose group carries a
`min_updated_seq` below the response's `sync_seq` although every grade in it
matches what this device sent, as well as for one holding a rejection or a row
from another device, and likewise for a batch whose **cascade, quiz or attempt**
row carries an `updated_seq` below that sequence, or whose tombstone carries
such a `seq`, read from the pulled rows themselves and never from the sequence
the device stamps them with; every base row carrying the sequence of the response that
wrote it, so a full pull deletes the rows an earlier pull wrote and spares
those a creation, a promotion or a grades fetch wrote while it was in flight;
a `restore_quiz` rejected with reason `error` dropping its overlay quiz row and
showing the could-not-be-restored notice; a batch whose only acknowledged
operation is an applied `purge_cascade` taking the fast path with its tombstone
still applied in step 2, and the grades still queued behind the batch limit for
that cascade dropped when the full path next runs; a `finish` rejected `not_deepest` with every grade kept
showing the `not_deepest` sentence alone, and one rejected `stale_attempt` with
42 dropped grades showing both sentences; **two offline finishes in one batch**,
the first rejected `not_deepest` and the second — with the grades on the level it
created — rejected `not_found` because that level never reached the server,
showing **one** notice, the `not_deepest` one, whose count covers both levels'
dropped grades and which never says the cascade was deleted, while the same
`not_found` on a cascade the pull's tombstone removed does show the deletion
sentence, and grades rejected `not_found` for questions the server's own miss set
lacks show the came-out-differently sentence with their count and no deletion
sentence; a `questions_hash` mismatch found after
materialisation refetching the list; the drop pass running after a pull; a cleared quiz
pulled by a new device arriving as one row with no question rows, restorable
offline and pending until online, and a Trash export of it fetching its
rows; a `finish` clearing a quiz leaving that quiz's rows and positions in
the base, so a Trash export of it works offline, until the cascade leaves the
window; a pull that reports a quiz cleared never flagging it pending; grade
rows pulled for a pending quiz of a cascade with a pending `trash_cascade`
being written nowhere; a full pull deleting no base row of a cascade with
pending operations although it is absent from `question_rows_for`; a `stale`
grade beside an applied `finish` with a matching outcome showing the grade's
own sentence; a new order-filter row opening at rank 1; the drop pass in a
browser without Web Locks skipping the running tab's open cascade; a quiz restored on acknowledgment and fetched at once surviving
the next drop pass, while a `trash_cascade`, `purge_quiz` or
`set_quiz_options` does not refresh a cascade's last-open time; the drop
pass skipping a cascade whose player is open in a second tab that holds its
Web Lock; a Catalan cascade pulled from another device having its
distribution stored before Available offline, and typed mode on one whose
distribution is missing showing "needs a connection"; a `set_quiz_options`
with a segment size above the cap `/api/auth/me` reported being refused on
the device, while a quiz **already** holding a larger size keeps playing and the
builder prefills the cap rather than that stored default, so a lowered cap never
sends a value the server refuses; a resync on a device holding five 300,000-question cascades
comparing base and pull within the [scale test](#scale-tests)'s budget; the
Cascades page showing **Available offline**, not the download label, for a
kept cascade outside the window; the `stale`,
`stale_attempt` and `not_active` grade notices for grades with no finish of
this device behind them; a `finish` rejected `not_active` showing the
finished-elsewhere sentence; and a restored quiz whose options the server
built from cascade options changed elsewhere taking the server's row; Keep offline
starting a fetch at once, the drop pass skipping a kept cascade, a cascade over
the automatic Keep offline threshold not being dropped when it leaves the
window and being dropped once the user turns Keep offline off, and eviction
skipping a kept cascade's cards while taking an unkept one's; cards fetched
without definitions, the preference turned on offline, the player showing
words only, and the cards refetched with definitions on reconnect; a first sync
sending `cursor: null`; a `resync_required` response dropping the acknowledged
operations, replacing the base from a `cursor: null` pull and replaying the rest
of the outbox with nothing lost, refetching no index list and recomputing no
positions for a quiz whose seed is unchanged, and deleting a quiz the full pull
did not carry; an interrupted pull converging on the next
sync; a `426` acknowledging its operations, showing **reload to keep syncing**
rather than a waiting count, keeping the rest of the outbox and the grades that
follow it, and still showing that state after a reload the service worker
answered from its cache, until a sync succeeds; a `503` resending the same batch
after `Retry-After` with the outbox intact; and `429`
backoff honouring `Retry-After`. The **download manager** has its own: a `429`
on a card, keys, index-list or grades page pausing that cascade's fetches until
`Retry-After` and then finishing them, with its progress indicator unchanged and
no page lost or refetched twice; ten kept 300,000-question cascades draining
through a bucket that answers `429` from the 120th request of each minute and
completing rather than stalling or spinning; and a page the player needs on the
current card taking priority, so the download manager starts nothing while one
is outstanding and the card shows its answer instead of "answer needs a
connection".

The **export formatters** are held to one set of fixtures on both sides.
`contract-fixtures/export/` covers all three quiz types, the four selections,
both formats, both orderings for a quiz export and the cascade export offering
only one, a selection with no entries in both formats
(zero bytes, and the header row alone), multi-character tiles in MAGPIE notation, CSV
quoting of definitions with commas and quotes, CRLF versus LF line endings,
multi-word anagram answer cells, an anagram row whose definitions contain ` / `
and one whose definition contains ` | ` (written as it is, since the cell is
for reading),
the `hooks` and `grade` cell forms including a word with four front hooks in
tile order, leave values at every decimal setting, empty `definition` and
`hooks` cells for Leave Value rows, the
filename rule applied to a default cascade name with `·` and `–` in it, to a
name holding a character outside the Basic Multilingual Plane (one `_`, not
two) and to a name longer than 100 characters, and the
cascade-wide definition of correct and missed across several active quizzes. A Rust test and a Vitest test
run the same fixtures, and an [integration test](#integration-tests) checks that
`GET /api/cascades/:id/export` returns the same bytes for the same request.

### Integration tests

`cargo test` with `TEST_DATABASE_URL`, driving the real router in-process
against a real Postgres. Each test runs in its own transactional database, so
they run in parallel and leave nothing behind.

- **Upload validation tests**: one failing file per rule under
  [File formats](#file-formats). Each asserts the line numbers in the error list
  and that nothing was written. A file with many errors reports the first 1,000
  and the total. Every letter distribution file in MAGPIE-DATA, fetched at a
  pinned commit, uploads unchanged and produces the expected tiles.
- **Schema tests**:
  - Every one of the 23 condition types round-trips through
    `search_conditions` and back into an equal `ConditionKind`, In Lexicon in
    both of its stored forms — the id in a cascade's spec, the name in a saved
    search's — with a row holding both, or neither, rejected by the `CHECK`, and a nested
    AND / OR tree round-trips through `search_groups` unchanged, with the spec's
    `quiz_type` carried back out beside it.
  - For each type, inserting a row with a missing or extra parameter, or with
    Not where it isn't allowed, is rejected by the `CHECK`.
  - A spec of 100 groups round-trips, and a 101st is rejected by the `id`
    `CHECK`, which is the constraint the application's group cap is set to, so
    the two can never disagree about where the limit is.
  - A Leave Value condition with `NaN`, `Infinity` or `-Infinity` in either
    bound is rejected by the `CHECK`, including the `NaN`–`NaN` pair that
    satisfies `min_leave_value <= max_leave_value`, and a finite pair at the
    upload bound of ±1,000,000 is accepted.
  - A `text_value` of 500 canonical characters is stored and one of 501 is
    rejected by the `CHECK`, so the field error the application reports first
    and the column's own limit are the same number.
  - A Leave Value cascade whose leave value set belongs to another lexicon is
    rejected by the composite foreign key.
  - A second active quiz at the same level, a second Source quiz, a Source quiz
    at any level but 1 or with status `cleared`, a non-Source quiz at Level 1,
    and a cascade with `depth` 0 are each rejected.
- **Sync integration tests** (`cargo test`, `TEST_DATABASE_URL`):
  - A repeated operation is applied once and gets the same result in every
    field, `outcome`, `new_quiz_question_count` and `new_quiz_questions_hash`
    included.
  - A `finish` whose `shuffle_seed` and resulting `questions_hash` are both
    **above 2⁶³** is applied, stored as negative `BIGINT`s, and answered — and
    then pulled, and fetched again as a repeat — with the unsigned decimal text
    the device sent and computed, so the device's hash comparison matches and no
    outcome-mismatch notice appears; the same seed sent back as an
    `attempt_seed` on a later `grade` is recognised rather than rejected as
    `stale_attempt`.
  - Operations are applied in `device_seq` order, and one rejection doesn't stop
    the batch.
  - Two simulated devices cover every row of the [Conflicts](#conflicts) table.
  - Pulls return exactly the rows changed since the cursor, across page
    boundaries, with the first page's `question_rows_for` and sequence ceiling
    fixed for every page even when a later page request sends a different list
    and another device commits a row between two pages.
  - A page holds at most 50,000 rows of any kind: a pull of 60,000 cleared
    chain quizzes is paged although it carries no question rows at all, and so
    is the pull of their 60,000 tombstones after the retention period; no page
    exceeds the limit, each row appears once, and a quiz row still precedes its
    own question rows across a page break that falls inside `quizzes`.
  - Each `quiz_questions` group carries `min_updated_seq` as decimal text,
    equal to the lowest `updated_seq` among its rows in that page, so a page
    holding one grade from an earlier push beside this push's own reports the
    earlier sequence; every cascade, quiz and attempt row carries its own
    `updated_seq` and every tombstone its `seq`, as decimal text, so a pull
    holding one cascade row from an earlier push beside this push's own reports
    both sequences and the device can tell them apart; and a quiz whose rows span a page break reports a minimum
    per page, with the earlier device's grade on the first of them, so a device
    that folded only the last page would wrongly take the fast path.
  - Search concurrency: with `SEARCH_CONCURRENCY=1` and a short
    `SEARCH_TIMEOUT_MS`, a second concurrent search is answered `503` with
    `Retry-After` and never `422`, while a search admitted on an idle instance
    returns its results.
  - Purges produce tombstones, and a cursor below `sync_floor_seq` gets
    `resync_required` carrying the push's `results` and no `changes`, while a
    cursor between the floor and the oldest kept tombstone does not; so does
    a cursor above `sync_seq`, with none of the request's operations applied;
    after the restore procedure's sequence bump a device whose cursor was
    ahead of the server has its operations applied and is told to resync; a
    `cursor: null` request on an account whose floor is above 0 returns every
    row and no tombstones, never `resync_required`; and a `cursor: null`
    request carrying 200 grades from a device that created its cascade and
    never synced applies them and returns every row.
  - A new device's first pull carries the preferences row of an account whose
    preferences were never changed.
  - The grades endpoint returns a quiz's graded rows in pages with its attempt
    and seed, and `404` for another user's or a purged quiz. `from` is an index,
    so a 300,000-question quiz whose only 400 graded rows sit above index
    250,000 answers the first five pages with no rows and the sixth with all
    400, and every page's rows fall inside its own `from … from+limit−1` window.
  - A sync whose `question_rows_for` omits a cascade carries none of its
    question rows, one with an empty list carries no question rows at all, and
    one naming another user's cascade or a purged id has those ids ignored.
  - `/api/lexicons` omits an item while a second instance is still completing
    its startup load and lists it once that instance turns ready, and an item
    an instance drops leaves that instance's `catalog_instance_status` rows at
    once.
  - `GET /api/cascades/:id/cards` with `keys=1` returns `{ from, keys: [...] }`
    with no per-row index, accepts a `limit` of 100,000 and refuses 100,001,
    refuses `hooks` or `definitions` with `400`, and lists the same keys in the
    same order as the full form for the same range; a card page is served
    `no-store` while the questions endpoint keeps its immutable caching. On a
    Catalan Definition cascade of 15-tile words the same page's keys run to tens
    of bytes each, which is why the estimate under
    [On the device](#on-the-device) is a range and the budget measures; the test
    records the page's bytes per key on both that cascade and an English anagram
    one, so the 100,000-row limit's sizing is measured rather than assumed.
  - A device-created quiz id that already exists is rejected.
  - Grades pushed for a cascade purged on another device are rejected as
    `not_found`, and the same pull carries its tombstone.
  - A repeated `finish_segment` for the same quiz, attempt and boundary is
    applied once, and two devices racing on one boundary produce one drill quiz;
    and when that run **missed nothing**, so the winner created no drill quiz,
    the loser is rejected on the cursor test rather than as a duplicate and the
    device shows the run-finished-elsewhere sentence, not the size-or-place one.
  - `finish_segment` is rejected for a quiz with no segment size, for a boundary
    that is not a multiple of it, for one at or past the question count, and for
    one with ungraded questions before it; a `set_cascade_options` or
    `set_quiz_options` carrying a segment size of 1 to 4 is rejected as
    `invalid`, and the schema `CHECK` refuses such a row directly.
  - Options and preference operations resolve whole: with one device changing
    segment size and the other progression with interleaved timestamps, the
    later operation is applied, the earlier is rejected as `stale`, and the
    row matches the later one alone.
  - The first `grade` on an ungraded question and the first `move_cursor` on a
    fresh quiz are applied.
  - A device's own regrade with an earlier `graded_at` is applied; another
    device's earlier grade is not, and is rejected as `stale`, as is an
    earlier cursor move from another device; a second `finish` after one
    that cleared the quiz is rejected `not_active` and after one that reset
    it `stale_attempt`; another device's earlier grade whose
    `seen_seq` covers the current grade is applied; the same for cursor moves,
    options and preferences, including an options change by a device that had
    seen the options but not a later cursor move on the same quiz
    (`options_seq`); a grade's `at` stored as its `graded_at`, the grade
    carrying no other timestamp; a grade dated a year ahead is stored at
    `now() + 5 minutes` and loses to a real grade ten minutes later, and a
    `finish` dated a year ahead records `finished_at` at `now() + 5 minutes`;
    and two
    option changes, and two preference changes, from one device with a clock
    a day ahead, sent in one batch, are both applied
    (`options_device_id`, `changed_by_device_id`).
  - `move_cursor` is rejected below `run_start`, at or past the next run
    boundary, and at the question count of a quiz with no boundary; a
    `device_seq` gap left by a coalesced cursor move is accepted.
  - A `grade` for a question that is not in the quiz is rejected as not found.
  - A pull sends only graded question rows of active quizzes and never an
    index list or a cleared quiz's rows, sends each
    quiz row before its question rows, and orders tables as specified; the
    questions endpoint returns a quiz's index list in pages, `404` for a Source
    quiz, and the same bytes for the quiz's whole life.
  - A sync whose `app_version` is below `MIN_APP_VERSION` gets `426` with its
    operations applied and their `results`, while the build number equal to it
    syncs normally, and a non-numeric `app_version` is a `400`; an operation whose `seen_seq` is
    above the user's `sync_seq` is rejected as `invalid`; and a `trash_cascade`
    on a trashed cascade, a `restore_cascade` or `purge_cascade` on an active
    cascade and a `purge_quiz` on a purged quiz are rejected as `trashed`,
    `not_trashed`, `not_trashed` and `not_found`.
  - A repeated `POST /api/cascades` or start-over with the same `id` returns
    the existing cascade and takes no second slot; an `id` held by another
    user is refused; and an options change seconds after creation from a
    device whose clock is behind the server is applied, not `stale`.
  - A cascade creation runs its search before opening a transaction: with a
    slow search, the purge task for the same user acquires that user's row
    while the search is still running, and neither blocks the other; a
    creation over the limit is refused before the search runs at all, and one
    that loses the limit race after searching is refused with `409`.
  - Restoring a trashed cascade, or one of its quizzes, resets `cleared_at` on
    its remaining cleared quizzes, and the purge task leaves them for a full
    retention period afterwards, while a quiz restored in an active cascade
    leaves its siblings' `cleared_at` untouched; a `finish` whose `at` is 40
    days old, applied today, clears a quiz the purge task leaves alone for a
    full retention period, while the cascade's `last_activity_at` is 40 days
    old on a cascade with no newer activity, and an older `at` never lowers
    either `last_activity_at` column.
  - `finish` is rejected while a question is ungraded, and applied with an
    outcome that differs from the sender's when another device's grade moved
    the score across the threshold.
  - `peak_depth` and `attempts_since_completion` are unchanged by a purge: a
    cascade with fifteen attempts since its last completion has its cleared
    quizzes purged, which deletes their `quiz_attempts` rows, and both
    counters still read what the finishes set, so the completion screen counts
    work the retention period has swallowed.
  - A `grade` on a question in a run another device has already passed is
    applied: it moves that attempt's counters, the run's drill quiz is not
    duplicated (`quizzes_one_per_segment`), the grading device's
    `move_cursor` inside the passed run is rejected `bad_cursor` and its
    `finish_segment` for that boundary `duplicate_segment`.
  - A `set_cascade_options` or `set_quiz_options` carrying a segment size of 3,
    a segment size above `MAX_QUIZ_QUESTIONS`, or an unknown progression is
    rejected as `invalid` before any write, so no such operation is ever
    recorded with reason `error`; the same for a `set_preferences` carrying a
    clear threshold of 0 or `leave_value_decimals` of 4, and for a
    `set_bindings` naming `Escape`, a mouse button the column does not allow, or
    a fourth binding for one action — each `invalid`, with the row unchanged and
    nothing recorded as `error`, since an `error` alarms (see
    [Monitoring](#deployment-and-operations)).
  - A `restore_quiz` whose application raises a database error inside its
    savepoint is recorded `rejected` with reason `error`, leaving the rest of
    the batch applied.
  - Saving a search is refused with `409` and
    `{ error: "saved_search_limit", limit, count }` at
    `MAX_SAVED_SEARCHES_PER_USER`; two simultaneous saves compete for the last
    slot and only one takes it; and an overwrite of an existing name is
    accepted at the limit, since it takes no new slot.
  - `GET /api/letter-distributions/:name` answers from an instance that has
    not finished indexing the lexicons built on that distribution, since it
    reads `letter_distribution_tiles` directly.
  - Every limited endpoint carries the limit its bullet in
    [Security](#authentication) names: with each rate lowered to 1, a second
    call in the same minute to a card page, a search, a saved search's
    `GET /api/searches/:id`, an export, a sync and an
    **admin upload** is answered `429` with `Retry-After`, and spending one
    bucket leaves the others untouched.
  - The auth limits: with `LOGIN_FAILURES_PER_IP_PER_MINUTE` at its default,
    twenty **successful** logins for twenty accounts from one address all
    succeed, while the eleventh **failed** login from that address in the minute
    is refused with `429` before any Argon2 verify runs, and a login for another
    username from a second address still succeeds; the eleventh failure for one
    username is refused from any address; and with
    `AUTH_RATE_PER_IP_PER_MINUTE` lowered to 1, a second register,
    confirm-email, reset-password or reset-password/confirm from one address in
    the minute answers `429` with `Retry-After`, while `GET /api/auth/me` and
    logout from that address do not.
  - The two endpoints that need no session are limited per IP: with
    `CATALOG_RATE_PER_MINUTE` lowered, `GET /api/lexicons` and
    `GET /api/letter-distributions/:name` answer `429` with `Retry-After` once
    the bucket is spent, the same requests from a second `X-Forwarded-For`
    address still succeed, and a signed-in user's sync on the same instance is
    unaffected, since the bucket is not shared with the per-user limits.
  - An operation id already recorded for another user is rejected as not found.
  - The request-level `400`s, each leaving nothing written and no operation
    recorded: `SYNC_MAX_OPS` + 1 operations, a `page_token` sent with an
    operation, a `question_rows_for` of `MAX_CASCADES_PER_USER` + 1 ids, an
    operation-level `device_id`, and a non-numeric `app_version`; and, as
    rejections rather than `400`s, an operation with `device_seq` 0 and one
    reusing a `device_seq` this device already sent under a different `id`,
    both `invalid`, with the `(user_id, device_id, device_seq)` constraint never
    reached and nothing recorded as `error`.
  - A pull spanning several pages returns each row once while another device
    commits between pages.
  - `set_quiz_options` naming `progression` or `segment_size` for a
    segment-chain quiz, or `progression` for the Source quiz, is rejected as
    `invalid`, and restoring a segment-chain quiz in a Ladder cascade keeps
    it on Drill with no segments.
  - `purge_quiz` on a quiz of a trashed cascade is rejected as `trashed`, and
    on an **active** quiz of a live cascade as `not_cleared`, which is also
    what a `restore_quiz` for a quiz another device already restored gets; the
    purge task leaves a trashed cascade's quizzes until the cascade's own purge,
    and every
    rejection in the suite carries a reason from the fixed list.
  - A database error injected inside one operation's savepoint (a `CHECK`
    violation) leaves the rest of the batch applied and records that operation
    as `rejected` with reason `error`; a forced serialization failure answers
    `503` with `Retry-After` and records nothing, and the same batch sent again
    is applied in full; and the purge task running against a concurrent sync
    for the same user completes without a deadlock.
  - Re-registering an unconfirmed email re-sends a code while the old one is
    valid and replaces the account once it has expired, and its username is
    taken while the code is valid and free after; a fourth confirmation,
    notice or reset email to one address from one IP within 24 hours is not sent
    while the response stays the same and a request from another IP still
    sends, while the **twenty-first** to that address within 24 hours is not sent
    from any IP, however many addresses the requests come from, so the per-IP cap
    cannot be walked around with a fresh IP each time; and a replacement made under the cap still
    replaces the account and sends its code with the next registration after
    the window; the purge task deletes stale unconfirmed accounts.
  - A 300,000-question `finish` and reset complete within budget, and the reset
    is pulled by another device as one quiz row.
- **Other integration tests** (`cargo test`, `TEST_DATABASE_URL`) drive the real
  router in-process, covering:
  - the auth flows, including the reset-password response taking the same time
    whether or not the address has an account, and the CSRF cookie carrying
    the session cookie's TTL and being re-set by `GET /api/auth/me`, so a
    client holding only the session cookie recovers a usable token; a logout
    clearing **both** cookies, and a login after it re-setting both; and
    `POST /api/auth/logout` with **no cookies at all** — what a device offline
    past both cookies' TTL sends — answered `2xx` rather than `401` or a CSRF
    `403`, while the same request **with** a session cookie and no CSRF token is
    refused `403` like any other write, and **with the session cookie and a
    matching token** — the ordinary queued logout, whose cookie the browser
    still holds — answered `2xx` with both cookies cleared
  - saved-search overwrite answering `409` without `overwrite` and replacing
    the old spec with it, and delete removing the spec; a save recording its
    `quiz_type` on the spec, `GET /api/searches/:id` returning it, and Start over
    copying it onto the new cascade's spec; a **round trip** of a tree holding
    nested AND and OR groups, every parameter shape and a 10,000-entry In Word
    List, saved and then read back from `GET /api/searches/:id` equal to what was
    sent, child order included, since that response is the only thing
    Load Search… has to rebuild the form from, while `GET /api/searches` lists
    the same search with `word_list_entries` 10,000 and **no tree at all**, and
    `GET /api/searches/:id` for another user's search answers `404`; a save
    whose response is lost, retried with the same `id` and body, returning the
    saved search rather than `name_taken`, and at one below
    `MAX_SAVED_SEARCHES_PER_USER` rather than `saved_search_limit`, with one row
    stored; an `id` held by another user refused with `409`; a missing name and
    a 101-scalar-value name each answered `400` with a field error, and a
    100-scalar-value name stored; and a save validating its tree like
    a preview — an empty group, a fifth nesting level, a row that does not apply
    to the submitted `quiz_type`, a range that narrows nothing, a
    `text_value` past the limit and In Word List rows totalling more than
    300,000 entries each answered `400` with the same
    `{ path, field, message }` errors and nothing written, while a tile that is
    not in some other lexicon's distribution is **not** an error, since a saved
    search carries no lexicon
  - `/api/lexicons` omitting an item one of two in-process instances has not
    loaded, and listing it once both have written `catalog_instance_status`
    rows, with a stale heartbeat ignored; and its four derived maxima —
    `max_num_anagrams`, `max_order_rank` and the two `leave_` figures, `null`
    on a lexicon without leave values — equal to the same maxima computed by
    brute force over the fixture catalog, since they are the only
    [filter ceilings](#filter-reference) the builder cannot work out for itself
  - admin authorization: non-admins get `404` on every admin route, and
    revoking `is_admin` takes effect on the next request
  - **Account binding**: a sync, a card page, a cascade creation and an export
    token request sent with Bob's session cookie but `X-Wordfall-User` naming Alice — or with
    no header at all — each answered `401` with nothing applied, recorded,
    pulled or streamed, and the same requests with Bob's id succeeding; and a
    `POST /api/auth/login` or `/api/auth/register` sent as `text/plain` with a
    JSON-shaped body answered `415` with no cookie set; `GET /api/auth/me` with
    `X-Wordfall-User` naming another account answered with the **cookie's**
    `user_id` and a fresh CSRF cookie rather than `401`; and
    `POST /api/auth/logout` with a session cookie, a
    matching CSRF token and **no** `X-Wordfall-User` answered `2xx` with both
    cookies cleared, as the queued logout sends it
  - **Sign out everywhere** with two sessions for one user: a wrong password
    refused and nothing changed; with the right one, the other session's next
    request answered `401` while the session that pressed it gets fresh session
    and CSRF cookies and its next sync succeeds
  - uploads followed by catalog reload across two in-process app instances
    sharing one database (`NOTIFY`), and the reconcile fallback
  - two app instances booted **at the same time against an empty database** both
    reaching ready, with the migrations applied once and `_sqlx_migrations`
    holding one row per migration, which is what a rolling deploy does on every
    release that carries one
  - deletion refused for each kind of reference, and allowed once unreferenced;
    a lexicon named only by a **saved search's** In Lexicon row deleted without
    refusal, `/admin` counting only cascades, and that saved search then loading
    with the row flagged and refused at creation, while an In Lexicon row in a
    live or trashed cascade's spec still refuses the deletion
  - cascade creation for all three types, Start over (including at the cascade
    limit, and the original being purged afterwards), card pages, a missing
    or 201-scalar-value `name` refused with `400`, and a
    300,000-entry In Word List accepted under `API_MAX_BODY_BYTES` while eight
    rows of 250,000 entries — a body well under the same limit — are refused
    with a field error on the row that crosses the tree's 300,000 total, on
    creation and on preview alike, with nothing written
  - the cascade limit, including trashed cascades counting toward it and two
    simultaneous creations competing for the last slot
  - creating a cascade with each combination of quiz options, and the Source
    quiz carrying the copy
  - every export endpoint selection, for all three quiz types, including an
    export of a quiz in the Trash and a `404` for another user's cascade, each
    through a token from `POST /api/cascades/:id/export-token`; the token
    request answering `401` for a mismatched `X-Wordfall-User`, `404` for a
    purged quiz and `429` with `Retry-After` past `EXPORT_RATE_PER_MINUTE`; and
    the download answering `204` for a token used twice, a token 61 seconds old,
    and a token presented with choices other than those it was issued for; and a
    token issued by one of two in-process instances redeemed on the other, then
    refused on a second redemption on either, since its use is recorded in
    `export_tokens_spent` and not in either process; and an export token
    presented as the `wordfall_session` cookie, and a session token presented
    as an export token, each refused, since the export key is derived from the
    session key and neither decrypts under the other
  - the purge task, including two instances running it at the same time
  - one user being unable to reach another's cascades through REST or sync
  - the application-level invariants listed under [Schema](#schema)

### End-to-end tests

Playwright against the whole stack, in a real browser, with the production-style
build and a registered service worker. This is the only layer that can prove the
offline promises, so it is where the plane, the two devices and the expired
session live.

#### What the tests reuse

Playwright's `globalSetup` does not know how to start a Wordfall. It calls the
same `scripts/stack.py` entry points that
[`./scripts/dev.py`](#how-it-is-put-together) calls, in the same order:

| Reused | How the suite uses it differently |
|---|---|
| `stack.up` | Project `wordfall-e2e` and a port from the environment, so a suite can run beside a development stack without disturbing it |
| The production-style frontend build | Not differently at all — the offline journeys need the real service worker, which is exactly why `dev.py` serves that build too |
| `stack.seed`, through the admin API | Seeds only [the fixture catalog](#the-fixture-catalog); licensed files are never available to CI. Every run therefore registers and confirms the `dev` account through the real endpoints, so a passphrase the strength check refused, or a registration side effect seeding skipped, fails the suite at its first step; and CI runs it once more with `ADMIN_UPLOAD_RATE_PER_MINUTE=2`, where seeding must wait out `429`s and still finish |
| `stack.reset` | Called between suites rather than by hand. Most specs don't need it, because each one registers its own user |
| `stack.down` | With `--volumes` at the end of a CI run |

Everything the tests need that a developer wouldn't goes through the same
`--env` flag a developer would use, so there is **no test-only code path in the
server**: `SESSION_TTL_SECONDS=2` for the expired-session journey,
`TRASH_RETENTION_DAYS=0` with a short `PURGE_INTERVAL_SECONDS` for the purge
journey, and `MAIL_BACKEND=console` so the confirmation link can be read out of
the backend's logs — which is also how a developer confirms an account locally.

What the sharing buys: a broken compose file, a missed `/health` condition, a
changed seeding step or a renamed environment variable fails `./scripts/dev.py`
and `make test-e2e` in the same way, in the same place, and is fixed once. The
failure mode it removes is the familiar one where CI has its own quietly
divergent way of starting the app.

`make test-e2e BASE_URL=https://…` skips `stack.up` and seeding and runs the
journeys against an existing instance instead, given an admin account in the
environment. That is how a deployment is smoke-tested and how the
[restore drill](#the-drill) checks a restored database.

#### The journeys

- Register, confirm, log in, create an anagram cascade with an 80% threshold,
  and see every cascade rule applied as expected:
  - Finish Level 1 below the threshold and go down to Level 2.
  - Clear Level 2 with misses and get a replacement at Level 2.
  - Clear it with no misses and climb back to Level 1's reshuffled quiz.
  - Finish Level 1 with no misses, see the completion screen, choose Keep
    studying and land on the reset Source quiz at Level 1.
- **The plane:**
  1. Create a cascade and wait for Available offline.
  2. Go offline (`context.setOffline(true)`) and reload the page.
  3. Still offline, close and reopen the browser context, and see the studied
     cascade rather than the login page, which is what the unscoped `signed_in`
     pointer is for (see [On the device](#on-the-device)).
  4. Study through several finishes, including a descent and a clear.
  5. Restore a quiz from the Trash.
  6. Go back online and see Synced.
  7. In a fresh browser context, log in and see identical cascade state, grades
     and question order.
- **Two devices:** finish the same level offline in two browser contexts,
  reconnect both, and see the second device's notice and matching final state.
- **Session expiry while offline:** study, expire the session, reconnect, see
  Log in to sync, log in, and see the work synced.
- **Browser restart with a live session:** study, close and reopen the browser
  context keeping its persistent cookies, and see the next sync succeed rather
  than fail CSRF.
- **Logging out.** With nothing unsent, Log out lands on the login page, and a
  reload while offline stays there rather than showing the last account's
  cascades. Logging back in finds every cascade still local, with no download.
  **Offline**, Log out still lands on the login page, and on reconnecting the
  queued `POST /api/auth/logout` goes through, after which the old session
  cookie is gone. In the same state — logged out offline with the request still
  queued — a **second account logging in before the connection returns** keeps
  its session: no logout is sent while it is signed in, its next sync succeeds
  rather than answering `401`, and the first account's flag is gone, since the
  login replaced the cookie the request was for. With unsent work, the dialog names the count and says those
  changes sync at
  the next login here, and **Remove this account's data from this device**
  deletes the per-user database after naming what that discards, and drops that
  account from the storage line while a logout still queued for it is sent on
  the next connection with no account signed in. A second
  account logging in on the same browser sees none of the first account's
  cascades, and its Account page lists **both** accounts with their totals and
  removes the first one's data, leaving its own untouched. Removing its **own**
  data from the same line clears the signed-in pointer and lands on the login
  page.
- **Switching accounts in another tab:** as Alice, grade twenty cards offline
  in one tab; in a second tab, log out and log in as Bob; see the first tab show
  **signed out in another tab** at once and stop syncing. Reconnect, and see
  that no request from the first tab was applied as Bob: Alice's twenty grades
  are still in her outbox, Bob's cascades are nowhere in Alice's database, and
  logging in as Alice again pushes the twenty grades. Then, with two tabs as
  Alice, log out and back in as Alice in one of them and see the other pass
  through **signed out in another tab** and resume syncing and downloading
  without a reload.
- **Updating the app:** with a card open, deploy a second build and see the card
  undisturbed and "a new version is ready" offered; accept it and see the new
  build running after one reload. With `MIN_APP_VERSION` raised past the old
  build, see **reload to keep syncing**, press its reload while online and see it
  clear, where a plain browser reload had not; and with a second tab open on the
  old build and a card revealed in it, accept the update in the first tab and
  see only that tab reload, the second keep its revealed card, show that the app
  was updated in another tab, finish that card from the old build's cache, and
  report the update rather than block the new build's IndexedDB upgrade.
- **Deleting the account** from `/account`, with a second account's data also on
  the browser, lands on the landing page, leaves no per-user database or
  `accounts` row for the deleted account — so the second account's storage line
  lists only itself — and a reload offers the login page rather than the deleted
  account's cascades. On a second browser context still signed in as that
  account, the next sync answers `401`, it shows **Log in to sync** with its
  cascades still readable, and its Account page still offers to remove them.
- Switch to typed mode: a wrong entry grades missed, finding every anagram
  grades correct, Enter on an empty input reveals the answer, a repeated entry
  is ignored, a right click before the reveal still toggles the grade while a
  left click only focuses the input, and switching modes mid-card clears the
  entries. Two answers typed on one line (`RETAINS NASTIER`) stay in the input
  with "type one word at a time" and the card, with every anagram then found,
  still grades correct. With **Enter bound to Toggle grade**, Enter on a typed
  word submits it and leaves the grade alone, and Enter on an empty input
  reveals the answer; with **`Shift+T` bound to Toggle grade**, typing a `T`
  with Shift held puts `T` in the input; and both bindings act as bound once
  the input loses focus and in flashcard mode.
- With **alphabetical order** on, entering answers in order grades correct, an
  out-of-order answer is marked out of order, joins the found list and grades
  the card missed, and a later in-order answer after it is accepted normally.
  With the option off, the same sequence grades correct.
- **Segments and progression:** create a cascade with a segment of 5 and finish
  the first run with misses, see the drill level, clear it down to nothing,
  come back to run 2 at the right question, see Previous refuse to go back into
  run 1, and finish the quiz. Switch the current quiz's progression to Drill
  from its settings menu, finish below the threshold, and see the quiz replaced
  at the same level instead of a new level appearing; then switch the
  cascade's progression to Drill and see the next new quiz take it while a quiz
  in progress keeps its own. Change a quiz's segment size
  from the settings menu mid-attempt and see the next boundary move. Then set
  the size to 0 mid-attempt and see the run indicator disappear while Previous
  still refuses to go back into the finished run, with the reason shown.
- **Export:** download the missed words of a level as a word list and the whole
  cascade as a CSV, offline, and check the contents; then ask for definitions
  that were never downloaded, while offline, and see the fallback notice. With
  the cascade's cards complete, the dialog shows the selection's question count
  and its entry count; with the answers evicted and the app offline, it still
  shows the question count, from the `questions` store, and drops the entry count
  rather than blocking or showing a line count it cannot know. Online, export a
  cascade with definitions the device never downloaded, so it comes from the
  server through a token, and double-click **Download**: one file arrives and the
  app stays on screen, the second request having been answered `204` inside the
  hidden frame. The download reaches the server rather than the service worker,
  and the frame never loads the app: no second sync leader, no second member on
  the account `BroadcastChannel`. All of this runs with the production security
  headers and the build's own `<meta>` policy on, so a `frame-ancestors` or
  `X-Frame-Options` change that blocked the frame would fail here. The suite's
  first journey also asserts that the app **boots** under that policy, with no
  CSP violation reported in the console, so a build whose bootstrap hash is
  missing, or a header that re-added `script-src 'self'`, fails before
  anything else runs; and, since a page with **no** policy also boots cleanly,
  that the policy is **present**: the `index.html` the service worker serves
  carries the `<meta>` policy, its `script-src` holds `'self'` and a hash but
  not `'unsafe-inline'`, and an inline script the test injects into the page
  is refused with a violation. Then stop the backend between the token request and the download,
  so the download meets the proxy's error page, and see the app stay on screen
  with no file and the dialog offering to try again. With
  the cascade's answers evicted, export its questions offline and see a
  complete file, and see the cascade export offer no alphabetical toggle.
- **Answers gone, questions kept:** with a cascade's answers evicted and the
  anagram mode set to Typed, open it offline, see the alphagram and the
  flashcard fallback rather than a typed input, grade a card Correct, and see
  the grade sync as Correct. Then, on a cascade whose first download was stopped
  part way, reach a card past the last key and see "this question needs a
  connection" with no grade saved, then reach the last card of that run and see
  Show / Next do nothing and a message naming how many questions still need a
  connection.
- **Keys before answers:** create three cascades, go offline before any download
  finishes, and see every question and its grading available while the answers
  are still arriving.
- **Trash and purge:** clear a quiz, export its missed words from the Trash
  while offline with no fetch, then restore it and find it as
  the new deepest level, playable at once because the online restore fetched
  its list and grades before it was opened, then, with `TRASH_RETENTION_DAYS=0` and a short purge
  interval, watch a cleared quiz be purged and the tombstone remove it from a
  second browser context. Trash a cascade and see Delete forever offered on the
  cascade but not on the quizzes listed under it. Offline, see each Trash
  entry's purge date.
- A cascade whose segmented attempt left 5,000 cleared chain quizzes opens
  `/trash` within the suite's budget, showing one collapsed group with its count
  and earliest purge date, and expands to a first page of 100 behind **Show
  more**.
- At `MAX_SAVED_SEARCHES_PER_USER`, lowered with a test constant, see Save
  Search… refused with the limit and the count, and an overwrite of an existing
  name still accepted.
- Paste a 10,001-entry word list into an In Word List row and see the live
  preview pause behind a Preview button; change the lexicon and see an In
  Lexicon row on another distribution flagged.
- Offline, restore a quiz of a cascade this device has never opened, see the
  "downloads when online" note and the ladder's "waiting for download" level,
  go online and see it download without opening the cascade, then open it and
  play.
- Over the answer limit with two automatic keeps, see the Account page list
  both with their answer and key sizes, turn one off, and see its answers freed
  on the next drop pass while its questions still show in the player. Go offline
  and see that cascade's badge read "answers need a connection" rather than a
  download bar at zero, and see it return to progress on reconnecting.
- Over `ROW_STORAGE_BUDGET`, lowered with a test constant, see the Account
  page's total against the budget and the least recently opened unkept cascade
  dropped before its window expires, with a kept one untouched; then, still over
  the budget with nothing unkept left, see the oldest **automatically** kept
  cascade dropped too and the one the user marked Keep offline left whole, with
  the storage line saying that only turning Keep offline off can bring the total
  down. See the dropped cascades leave the storage line's kept list rather than
  sitting in it at zero bytes, their badges reading "kept automatically once
  opened". Stay online for several sync cycles and see them stay
  dropped — the storage total steady, no download progress reappearing, and the
  Cascades page offering "open to download" — then open one and see it come
  back whole and listed again.
- Trash a cascade offline and see it listed in the Trash with "purges 30 days
  after this syncs", not missing from both pages.
- Build a cascade with a segment size that would create more than 2,000 levels
  and see the warning naming the count, on the builder and in a quiz's settings
  menu.
- Turn on hooks and definitions, and see them in anagram answers: on the English
  fixture the hooks read Zyzzyva-style in lower case, and on a Catalan cascade a
  multi-character hook reads `NY`, not `ny`, so nothing on screen looks like a
  blank standing for a tile.
- **Desktop controls:**
  - Left click shows and advances, right click toggles, and middle click goes
    back, with no context menu appearing; a double click advances once, and a
    wheel binding fires once per flick.
  - Clicks on the side rails do nothing to the quiz.
  - Rebind Toggle grade to the wheel and to `Shift+T`, and see both work and
    the change sync to a second browser context.
  - Bind wheel-down to Show / Next, reveal a Definition card whose definition
    overflows the panel, and see wheel-down advance without scrolling the text,
    the capture box having warned it would, while the scrollbar and the Page
    Down key still reach the end of the definition and wheel-up still scrolls.
  - A binding can't be removed from an action that has only one, and Escape
    cancels capture instead of being bound.
  - Binding a stroke that already belongs to another action **moves** it, with
    the notice, and leaves the old action with its remaining bindings.
  - In typed mode, bound letter keys type into the input instead of acting, and
    so do Space and Backspace — the defaults for Show / Next and Previous — while
    **Ctrl+Backspace** still acts as Previous, since only unmodified editing keys
    are held back, and middle click still goes back with the input focused.
- **Touch zones** (mobile viewport emulation, portrait and landscape): each
  zone fires its action, a drag in the Show / Next zone scrolls a long answer
  without advancing, and a quick double tap advances only once.
- Set leave decimals to 3 and see a leave answer rendered to three places.
- As an admin, upload a distribution, lexicon and leave value set, see a
  deliberately broken file rejected with line numbers, build a Leave Value
  cascade from the new set, and see deletion refused while the cascade exists.

### Scale tests

- **Scale tests**:
  - Create a 300,000-question cascade, download its cards, grade every question
    (half missed) through sync, finish, and assert the timings for creation,
    download, push (300,000 grades in 600 requests back to back, under the
    per-user sync rate limit), pull and finish stay within budget, assert
    that the rebase work per acknowledged batch stays proportional to the
    batch (the drain's total IndexedDB writes within a small multiple of the
    operation count), and assert
    the IndexedDB row count of one copy plus a small overlay once the outbox
    has drained, recording the
    bytes of the rows, the question keys and the answers separately, with the
    keys near 3 MB, and assert the keys-only pass for ten such cascades takes
    30 card-page requests in all, a tenth of the 300 their answers need, and
    that the bytes it moves are within a quarter of those ten cascades'
    answers rather than a tenth, the ratio
    [Downloads](#on-the-device) states. A 300,001-question search is refused with its count. A new device
    logging into an account with ten such cascades completes its first sync in
    under 5 seconds with no question rows transferred, since none is opened
    here; after opening all ten, with each Source quiz finished once, creating
    a 150,000-question Level 2, then fully graded again on its new attempt
    without finishing, and that Level 2 half graded (375,000 graded rows
    per cascade, about 3.75 million in all, 75
    pages), the sync that follows finishes in under 120 seconds on the CI
    runner, measured with the staging split (rows of quizzes new to the device
    and grade rows for unchanged attempts written once); and each cascade
    materialises on open within budget. The budgets are constants in the test beside the row
    counts they measure.
  - Ten concurrent `GET /api/cascades/:id/export` streams of a 300,000-question
    anagram cascade with definitions, the `EXPORT_RATE_PER_MINUTE` default,
    complete within budget without the task exceeding its memory reservation.
  - Upload a million-row leave file within the upload timeout.
  - Purge a trashed 300,000-question cascade with three levels within budget
    while a sync for the same user proceeds, on a `quizzes` table padded with
    a million rows of other users, asserting with `EXPLAIN` that the delete
    uses `quizzes_cascade_id`.
  - Purge a user whose segmented attempt left **60,000 cleared chain quizzes**
    all past `TRASH_RETENTION_DAYS` at once: assert each run takes at most
    `PURGE_MAX_QUIZZES_PER_USER_PER_RUN` of them, oldest `cleared_at` first,
    that each transaction stays within budget, that the expected number of runs
    empties the Trash, and that a sync for that user during each run is
    answered normally rather than `503`, since the user row is held only for one
    capped transaction.
  - An incremental pull on a user whose segmented attempt left **60,000 cleared
    chain quizzes**, each with its own `quiz_attempts` row, on tables padded
    with a million rows of other users: assert with `EXPLAIN` that the attempt
    query uses `quiz_attempts_user_seq` and the cascade and quiz queries their
    own `user_seq` indexes, that a pull with nothing changed reads a number of
    rows proportional to what changed rather than to the user's quiz count, and
    that it stays within a budget beside the row counts.
  - Resync a device holding five 300,000-question cascades, one of them with
    cleared levels whose rows a Trash export fetched, and assert the full
    pull's base comparison and deletions finish within budget and keep those
    cleared rows. A sixth cascade has run a segmented attempt with a segment
    size of 5 over 20,000 questions, leaving 4,000 chain quizzes in the Trash,
    so its pull pages on quiz rows alone: assert the page count, that no page
    exceeds 50,000 rows, and that the resync stays within the same budget.
  - Let a 300,000-question cascade leave the download window with the
    automatic Keep offline turned off, and assert the drop pass finishes within
    budget and the base holds only its cascade, quiz and attempt rows. Run the
    same pass on forty such cascades all inside the window, every one of them
    over `AUTO_KEEP_ROWS` and so **automatically** kept, and assert it brings
    the base's rows and keys under `ROW_STORAGE_BUDGET` within budget, oldest
    last-open first: this is the case the budget exists for, and it is only
    reachable because an automatic keep is the pass's second tier rather than a
    skip. Mark one of the forty **Keep offline** by hand and assert it is the
    one cascade still whole at the end. Then run one more pull and assert that
    **nothing is refetched**: every cascade the pass dropped is marked
    budget-dropped in `meta` although all forty are inside the window, the
    download manager issues no request for any of them, the sync request's
    `question_rows_for` names only the kept one, and the base's total stays
    under the budget instead of oscillating. Run the
    same case on a cascade that also holds five cleared 150,000-question
    levels inside the window, asserting the row count before the pass counts
    them and the pass removes them too.

### Zyzzyva parity

- **Zyzzyva parity** (local only, needs licensed data): a script runs a
  checked-in list of saved searches against a real CSW24 upload and compares
  the word lists with exports from Zyzzyva for the same searches. Zyzzyva has
  no OR, so the parity searches are single AND groups, and the script writes
  Wordfall's `.` wildcard as Zyzzyva's `?`. The list includes a two-tile
  `Not Includes` row and a mixed-length limit (`Length 7–8`, `Limit by
  Probability Order 1–100`), so the negation's meaning and the limit's ranking
  value are checked against Zyzzyva itself. Differences are either fixed or recorded here as intended deviations.
  The run is also the only place the figures that need real data are measured,
  and it prints them beside the comparison: each index's build time and
  resident size (see [Catalog Indexes](#derived-attributes)) and the
  wall-clock time of every search on the list (see
  [Search Engine](#search-engine)). They are recorded, not asserted, since the
  data they need cannot be committed.

### Running the tests

| Command | What it runs |
|---|---|
| `make test-unit` | `cargo test --lib`, `npm run check`, Vitest. No services. Seconds. |
| `make test-integration` | `cargo test --test '*'` against a throwaway Postgres it starts itself |
| `make test-e2e` | `stack.up`, `stack.seed`, Playwright, `stack.down` |
| `make test-scale` | The 300,000-question budgets |
| `make test-parity` | Zyzzyva comparison; skipped with a notice unless the licensed files are present |
| `make test` | Unit, integration and end-to-end |

CI runs `make test` on every push and `make test-scale` nightly. Parity never
runs in CI, because the data cannot be committed. Every layer is runnable with
one command and no arguments, for the same reason the development stack is: a
test that is hard to start is a test that stops being run.

---

## Delivery Phases

1. **Skeleton**: Compose stack, Axum and SQLx with the full `0001_initial.sql`,
   SvelteKit shell, `/health`, Terraform baseline.
2. **Accounts**: registration, email confirmation, login and logout, password
   reset, sessions and CSRF, rate limits, account page, admin authorization.
3. **Catalog**:
   - admin uploads with full validation for letter distributions, lexicons and
     leave value sets
   - tile parsing
   - `LexiconIndex` and `LeaveSetIndex` with every derived attribute
   - probability and playability ordering
   - `LISTEN/NOTIFY` reload
   - the `/admin` pages
4. **Search engine**: all 23 filters, including both limits, AND / OR groups, validation errors,
   unit, property and parity tests, `/api/search/preview`.
5. **Cascade builder**: filter rows, applicability rules, tile palette, word list
   editor, live preview, saved searches, clear threshold, quiz options,
   `POST /api/cascades`, card pages.
6. **Cascade rules**:
   - the Rust and TypeScript rule modules, including progression and segments,
     the deterministic shuffle, and the shared test vectors
   - the sync endpoint, operations, conflicts, tombstones and the purge task
7. **Local-first player**:
   - IndexedDB stores, the outbox and the sync engine
   - service worker, downloads and eviction
   - the player (desktop layout and configurable controls, touch zones,
     flashcard and typed modes with alphabetical order, the run indicator, the
     ladder, finish and run banners, preferences and quiz options menu)
   - the cascades and trash pages
   - exporting word lists, on the device and from the server
   - the offline Playwright journeys and the 300,000-question scale tests
8. **Production**: SES, ACM, deploy pipeline, backups and alarms, first catalog
   upload, restore drill.
