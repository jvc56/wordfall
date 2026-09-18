# Wordfall — Project Plan

## Overview

Wordfall is a word study website for crossword game players. A logged-in user
describes the words they want with filters, and Wordfall turns the matches into
a **cascade** of flashcard quizzes. Getting a quiz at least X% right **clears**
it and removes it from the cascade. Missing too many sends the missed questions
one **level** down. The user works down the cascade and back up until every
level has been cleared.

There are three quiz types:

| Quiz type | Question | Answer |
|---|---|---|
| **Anagram** | An alphagram (the tiles of a word in alphabetical order), e.g. `AEINRST` | Every valid word in the lexicon made from exactly those tiles, e.g. `ANESTRI, ANTSIER, NASTIER, RATINES, RETAINS, RETINAS, RETSINA, STAINER, STEARIN` |
| **Definition** | A word, e.g. `QAT` | That word's definition, e.g. `an evergreen shrub [n -S]` |
| **Leave Value** | An alphabetized set of 1–6 tiles, e.g. `?EIRS` | The leave's value, e.g. `+34.1` |

Most of the filters match search conditions in Zyzzyva
(<https://github.com/scrabblewords/collins-zyzzyva>), so experienced players can
describe a word list the way they already know how. There is one Wordfall-only
filter, **Leave Value**, for Leave Value quizzes. [Filters](#filters) below
explains what each filter means and how Zyzzyva implements it.

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

Creating a cascade needs a connection, because the search runs on the server.
After that, a downloaded cascade works entirely **offline**: a user can keep
studying, finish levels, go down and climb back up with no connection, and have
everything sync when the device is back online. See [Offline and Sync](#offline-and-sync).

---

## Glossary

| Term | Meaning |
|---|---|
| **Cascade** | Everything built from one set of filters: the Source quiz and every level quiz that comes from it. |
| **Source quiz** | The quiz created from the filter search. It is the permanent quiz at Level 1 and is never cleared: it resets instead. |
| **Level** | A position in the cascade, numbered from 1. Each level has at most one active quiz. The **deepest level** is the only one that can be played. |
| **Attempt** | One pass through a quiz's questions. A quiz that is reset starts a new attempt with a new shuffle. |
| **Segment** (or **run**) | A fixed-size run of questions inside an attempt, the length of one sitting. Finishing a run drills its misses before the attempt goes on. Off by default. |
| **Finish** | Move on from the last question of an attempt. Finishing always either sends the quiz to the Trash, sends the user down a level, or resets the quiz in place. |
| **Clear threshold** | The score (X%) needed to clear a quiz. It is set per cascade when the cascade is created. |
| **Quiz options** | Segment size, progression and alphabetical order. They are held by the cascade, copied to each new quiz, and editable on either. |
| **Progression** | What finishing a quiz below the clear threshold does: **Ladder** (keep the quiz and go down a level) or **Drill** (replace the quiz with its misses). |
| **Clear** | Finish an attempt with a score of at least the clear threshold. The quiz goes to the Trash, except the Source quiz, which resets instead. |
| **Trash** | Cleared quizzes and trashed cascades. They can be restored until they are **purged** (deleted permanently) after the trash retention period (Y days, a server setting that defaults to 30). |

---

## Cascade Rules

A cascade is a **stack of levels**, and the user always plays the **deepest
level**. The rules in this first table are the defaults: **Ladder** progression
and no segments. [Progression](#progression-ladder-or-drill) and
[Segments](#segments) below give the two ways the cascade's
[quiz options](#quiz-options) change them.

When the user finishes an attempt at the deepest level N, with score
= correct ÷ questions:

| Result | What happens | Where the user goes next |
|---|---|---|
| **Score ≥ threshold, some misses** | Level N's quiz is **cleared** (to the Trash). A new quiz of the missed questions takes its place at Level N. | The new Level N quiz |
| **Score ≥ threshold, no misses** | Level N's quiz is **cleared** (to the Trash). Level N is removed. | Back up to Level N − 1's quiz. If N was 1, the cascade is **complete**. |
| **Score < threshold, some correct** | Level N's quiz stays, **reset** to a new attempt with a new shuffle. A new quiz of the missed questions is created at **Level N + 1**. | Down to Level N + 1 |
| **Score < threshold, nothing correct** | Level N's quiz is **reset** to a new attempt with a new shuffle. No new level is created, because it would be an identical copy of Level N. | The reset Level N quiz |

The **Source quiz** is the one exception, and it applies whatever the
progression. The Source quiz is never cleared, never replaced and never goes to
the Trash on its own: it is the cascade, and the whole word list stays one
finish away for as long as the cascade exists. Finishing it does this instead:

| Result at the Source quiz | What happens | Where the user goes next |
|---|---|---|
| **Score ≥ threshold, some misses** | The Source quiz is **reset** to a new attempt with a new shuffle. A new quiz of the missed questions is created at **Level 2**. | Down to Level 2 |
| **Score ≥ threshold, no misses** | The Source quiz is **reset** to a new attempt with a new shuffle, and the cascade is **complete**: the user sees the completion screen and can start the reset Source quiz again whenever they like. | The reset Source quiz |
| **Score < threshold, some correct** | Unchanged: the Source quiz is **reset**, and its misses become a new quiz at **Level 2**. | Down to Level 2 |
| **Score < threshold, nothing correct** | Unchanged: the Source quiz is **reset** in place. | The reset Source quiz |

So above the threshold with misses, the Source quiz behaves exactly as it does
below the threshold; the only thing the threshold decides at Level 1 is whether
the attempt is recorded as a pass and whether a perfect attempt completes the
cascade.

Why these rules hold together:

- **The quiz being played is always the deepest level.** Clearing a quiz with
  misses keeps the same depth, clearing one with no misses pops a level, and
  failing pushes a level. So when the user finishes the quiz at Level N, no
  Level N + 1 exists yet. Missed questions never have to be merged into an
  existing quiz.
- **Upper levels wait.** A quiz above the deepest level sits in its reset,
  reshuffled state until the user climbs back to it, then starts from its first
  question. The one exception is a quiz that is waiting because a
  [segment](#segments) went down: it keeps its attempt, its grades and its
  cursor, and resumes at the start of its next run.
- **The same question can be on several levels.** A question missed at Level 2
  is still in Level 2's reset quiz and also in the new Level 3 quiz. That is
  intended: the user has to get it right in both places.
- **Score is compared exactly**, as `correct × 100 ≥ threshold × question_count`
  in integer arithmetic, so there is no rounding at the boundary.
- **A threshold of 100** means only a perfect attempt clears, and **0** means
  every attempt clears.

The "nothing correct" rule is the one departure from a literal reading of "go
down with the misses". Without it, a Level N quiz where every question was missed
would be followed by an identical Level N + 1 quiz, doubling the work for no
benefit. It can be removed without changing anything else.

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
  a segment quiz, whose progression is always Drill (see [Segments](#segments)).
- The **clear threshold** is not one of these options: it stays a single
  cascade-wide setting, because a cascade whose levels cleared at different
  scores would be hard to reason about.

Every option is validated on both sides: segment size is an integer from 0 to
`MAX_QUIZ_QUESTIONS`, progression is Ladder or Drill, and alphabetical order is a
flag. A segment size at or above a quiz's question count means the same as 0 for
that quiz.

### Progression: Ladder or Drill

Progression decides what a finish below the clear threshold does:

| Progression | Finishing below the clear threshold |
|---|---|
| **Ladder** (default) | The table above: Level N's quiz stays, reset and reshuffled, and its misses become a new quiz at Level N + 1. The user has to come back and clear Level N before the cascade is done. |
| **Drill** | Level N's quiz is finished and goes to the Trash whatever the score, and a new quiz of its missed questions takes its place at Level N. The cascade never grows a level from a finish, except the first one below the Source quiz. |

So with Drill, the score decides nothing about where the user goes; only the
misses do:

| Result | What happens | Where the user goes next |
|---|---|---|
| **Some correct, some missed** (any score) | Level N's quiz is finished (to the Trash). A new quiz of the missed questions takes its place at Level N. | The new Level N quiz |
| **No misses** | Level N's quiz is finished (to the Trash). Level N is removed. | Back up to Level N − 1's quiz. If N was 1, the cascade is **complete**. |
| **Nothing correct** | Level N's quiz is **reset** to a new attempt with a new shuffle. Nothing is created, because the replacement would be an identical copy. | The reset Level N quiz |

The Source quiz keeps its exception under Drill too: it is reset instead of
replaced, and because it cannot be replaced at Level 1, its misses become a new
quiz at **Level 2**. That is the only level a Drill cascade ever grows; Level 2
then replaces itself at Level 2 in the usual Drill way, and clearing it with no
misses comes back up to the reset Source quiz.

- The clear threshold is still recorded with the attempt, and the attempt's
  outcome is `cleared` when the score met it and `replaced` when it did not, so
  the Trash and the cascade's history still show which attempts were passes.
- The "nothing correct" rule is kept for the same reason as in Ladder: replacing
  a quiz with a copy of itself is work for no benefit.
- A Drill cascade that has never had a segment descent is one level deep for its
  whole life, which is what makes it a straight drill: keep going until nothing
  is missed.

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
| **Some of the run missed** | A new quiz of that run's missed questions is created at **Level N + 1**, with Drill progression. Level N's quiz keeps its attempt, its grades and its counters, with its cursor at the start of the next run. | Down to the new Level N + 1 |
| **None of the run missed** | Nothing is created, and nothing is finished. | The first question of the next run |

- **A segment quiz always drills.** However the cascade's progression is set,
  the quiz made from a run's misses has Drill progression: finishing it always
  replaces it with its own misses, until an attempt has no misses at all. Then
  its level is removed and the user comes back up to the parent quiz's next run.
  A run is a sitting to be finished, not a level to come back to.
- **A segment quiz is itself segmented** if the cascade's segment size is
  smaller than the number of questions in it, by the same rule. In practice a
  run's misses are far fewer than the run, so this rarely happens.
- **Coming back up** puts the user on the parent quiz's cursor, which is the
  first question of the next run, in the same attempt and the same shuffled
  order.
- **One descent per run.** A run descends at most once per attempt. Going back
  with **Previous** into a run that has already descended and changing a grade
  there does not descend again; the new grade counts at the finish like any
  other.
- **The finish still sees every miss.** A question missed in run 1 and then
  drilled at Level N + 1 is still missed in Level N's attempt: it counts against
  the attempt's score, and it is in the quiz that the finish creates, whether
  that is a descent (Ladder) or a replacement (Drill). Segments change **when**
  misses are drilled, not what the attempt was.
- **Resetting** a quiz starts its runs again from position 0 in the new attempt.
- **Changing the segment size mid-attempt** is allowed. Boundaries are always
  computed from the current size, so the next boundary is the smallest multiple
  of the current S that is greater than the cursor and less than the question
  count. Runs that have already descended stay descended, because a descent is
  recorded by the **boundary position** it happened at, not by a run number.
- **Segments never clear a quiz**, never change its attempt number and never
  touch the clear threshold. Only a finish does those.

### Trash, restore and purge

- **Finished quizzes** go to the Trash automatically, labelled with their
  cascade, level and final score, and with whether the score cleared the quiz or
  it was replaced under [Drill progression](#progression-ladder-or-drill).
- **A completed cascade** (the Source quiz finished at or above the threshold
  with no misses) is not trashed. The Source quiz is reset and waits, and the
  user sees a completion screen showing how many levels and attempts it took.
  The cascade goes to the Trash only when the user trashes it.
- **Trashing a cascade manually** (e.g. "I'm done with this word list") moves the
  whole cascade to the Trash with its levels as they are.
- **Restoring a finished quiz** pushes it back onto its cascade as the **new
  deepest level**, starting a fresh attempt with a new shuffle and a fresh copy
  of the cascade's current [quiz options](#quiz-options), so it is the
  next thing the user studies. If the cascade had been trashed, it comes back
  too, with its Source quiz at Level 1 and the restored quiz below it. Restoring
  never merges quizzes and never leaves a gap in the levels.
- **Restoring a manually trashed cascade** brings it back exactly as it was.
- **Purging** happens after the trash retention period: a cleared quiz is purged
  that long after it was cleared, and a trashed cascade that long after it was
  trashed. Purging a cascade purges every quiz in it. Users can also purge from
  the Trash immediately with **Delete forever**.
- **Start over** on a completed cascade creates a new cascade with the same
  filters, threshold and [quiz options](#quiz-options). It is only a shortcut:
  the completed cascade's own Source quiz is already reset and playable.
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

---

## User Experience

### Accounts

Every page except the landing, login, registration, email confirmation and
password reset pages requires a logged-in user. An account is a username, an
email address and a password. Some accounts are **admins**, who can also upload
and delete catalog data that no active cascade uses (see [Admin](#admin)). See
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
| **Default clear threshold** | New cascades | `80`% | 0–100 |
| **Default segment size** | New cascades | `0` (off) | 0 – `MAX_QUIZ_QUESTIONS` |
| **Default progression** | New cascades | **Ladder** | Ladder / Drill |
| **Default alphabetical order** | New cascades | off | on / off |
| **Leave value decimal places** | Leave Value answers | `1` | `0`, `1`, `2`, `3` |
| **Show definitions with anagrams** | Anagram answers | off | on / off |
| **Show hooks with anagrams** | Anagram answers | off | on / off |
| **Anagram answer mode** | Anagram quizzes | **Flashcard** | Flashcard / Typed |
| **Mouse shortcuts** | Desktop quiz area | On | Off turns every mouse binding inert without forgetting it. See [Controls](#controls). |
| **Keyboard shortcuts** | Player page | On | Off turns every key binding inert without forgetting it. See [Controls](#controls). |
| **Show answers found** | Typed-mode Anagram quizzes | Off | Shows `X of Y found` above the input. Off keeps the number of answers hidden. |
| **Controls** | Desktop quiz area | Show / Next: Mouse 1 or `,` · Toggle grade: Mouse 2 or `M` · Previous: Mouse 3 or `J` | Up to three bindings per action in each set: any mouse button, wheel direction or key, with modifiers. See [Controls](#controls). |

Question order is **not** a preference: questions are always shuffled.

### Creating a cascade

`/cascades/new` is laid out like Zyzzyva's Search tab. It needs a connection,
because searching runs on the server.

1. **Quiz type**: Anagram, Definition or Leave Value.
2. **Lexicon**: e.g. `CSW24`. When the type is Leave Value, lexicons without
   leave values are disabled, with a tooltip saying why.
3. **Filters**: a list of condition rows. Each row has a `+` button (add a row
   below), a `−` button (remove this row), a **Not** checkbox, a filter type
   dropdown, and inputs that change with the type. The dropdown only lists filters
   that apply to the chosen quiz type (see the
   [applicability table](#filter-applicability-by-quiz-type)). The Not checkbox
   is disabled for filters that do not support negation. Every row after the
   first also has a **join** dropdown, **and** or **or**, saying how it joins the
   row above; **and** is the default, so a list of plain rows still means "all of
   these must match". See [Combining filters](#combining-filters-and-and-or). For
   distributions whose tiles are not all
   plain A–Z, a **tile palette** under each tile input inserts tiles by
   clicking.
4. **Clear threshold**: prefilled from the user's default.
5. **Quiz options**: segment size, progression and alphabetical order, each
   prefilled from the user's defaults. They are explained inline ("a segment of
   40 means you study 40 at a time and drill what you missed before going on")
   and can be changed later on the cascade and on any individual quiz. See
   [Quiz options](#quiz-options). The alphabetical-order option is shown for
   Anagram cascades only, since nothing else has typed answers, but it is stored
   whatever the type.
6. **Cascade name**: optional. It defaults to a summary of the filters, such as
   `CSW24 · Length 7–7 · Probability Order 1–1000`.
7. **Preview**: runs the search as filters change (debounced) and shows how many
   questions it matches plus the first few, so the user can adjust before
   committing. The form shows inline errors for invalid rows, such as a
   malformed pattern, a tile not in the distribution, or min > max. Searches
   with more than 300,000 results say so and show the count.
8. **Create Cascade**: runs the search, shuffles the questions into the Source
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
- an **Available offline** badge, or download progress
- a row menu with **Quiz options**, **Export…** (see
  [Exporting words](#exporting-words)), **Keep offline** and **Move to Trash**

Selecting a cascade opens the player at its deepest level. The header shows how
much of the [cascade limit](#cascade-limit) is used (`87 of 100 cascades`) and
the sync status: **Synced**, **3 changes waiting to sync**, or **Offline**.

### Trash page

`/trash` lists finished quizzes and trashed cascades, grouped by cascade. Each
entry shows its final score, whether it cleared or was replaced, and when it will
be purged, and has **Restore**, **Export…** and **Delete forever** actions.

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
    quiz has a [segment size](#segments) (`run 2 of 3 · 37 of 100`), the
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
  again.
- **The last card.** Moving on from it finishes the attempt (see
  [Finishing a quiz](#finishing-a-quiz)).
- **Repeats.** The same action fired twice within 120 ms counts once, so a
  bouncing mouse button or an accidental double tap never skips a card.

#### Answers

This is how Definition and Leave Value quizzes always work, and how Anagram
quizzes work by default (**flashcard mode**). The answer that Show / Next reveals:

- **Anagram**: each valid anagram, one per line, in alphabetical order.
  - With *show hooks* on, each word is written Zyzzyva-style with its front
    hooks to the left and back hooks to the right, e.g. `bcfm AA hls`.
  - With *show definitions* on, each word's definition appears in smaller text
    beneath it.
- **Definition**: the full definition text.
- **Leave Value**: the value with its sign, rounded to the user's decimal places
  (half away from zero), e.g. `+34.1` or `−8.4`. A value that rounds to zero is
  shown as `0.0`, never `−0.0`.

#### Typed mode (Anagram quizzes only)

- The question shows a focused text input. With the **Show answers found**
  [preference](#preferences) on, a counter above it reads `0 of 9 found`. The
  preference is **off by default**, because the total is itself a hint: knowing
  an alphagram has nine anagrams tells the user to keep looking. With it off,
  the found list still grows as answers are entered, but neither the count nor
  the total is shown.
- The user types a word and presses **Enter**. Input is upper-cased, trimmed,
  and converted to tiles (see [Tiles](#tiles)).
  - A word that is **one of the answers and not yet entered** joins the found
    list, shown in alphabetical order, and the counter, if shown, goes up.
  - A word **already entered** is ignored, with a brief "already entered" note.
  - Any other word is **wrong**. It is listed in red under the input, and the
    question will be graded missed.
- The answer is shown when **every anagram has been found**, or on **Show /
  Next**, which here means giving up. **Enter on an empty input** is always Show /
  Next as well. Showing the answer displays the full list, formatted by the hook
  and definition preferences, with the words the user did not find highlighted.
- The grade is set automatically: **correct** only if every anagram was found
  with no wrong entries, otherwise **missed**. Toggle grade flips it, and Show /
  Next (or Enter) saves it and moves on.
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
- **Protecting typing.** While the input has focus, key bindings that would type
  or edit text are ignored: characters, Space, Backspace and Delete without
  Ctrl, Alt or Meta. Other keys still act, including Enter, Escape, the arrow
  keys, function keys and modified keys.
- **Protecting against accidental give-ups.** Before the answer is shown,
  clicking the quiz area focuses the input instead of acting.
- Switching modes in the middle of a card resets that card's typed entries.

#### Controls

The player has two independent sets of shortcuts, **mouse shortcuts** and
**keyboard shortcuts**. Both are user settings, and **both are on by default**.
Either can be turned off on its own in **Controls**, on the Account page and in
the player's preferences menu; turning a set off makes its strokes inert without
forgetting how they are bound, so turning it back on restores them. Both sets
are configurable. With both off, the player is driven by the on-screen buttons
and the touch zones only, and Controls says so.

The defaults are:

| Action | Mouse shortcut (in the quiz area) | Keyboard shortcut |
|---|---|---|
| **Show / Next** | Mouse 1 (left) | `,` |
| **Toggle grade** | Mouse 2 (right) | `M` |
| **Previous** | Mouse 3 (middle) | `J` |

`,`, `M` and `J` sit under the right hand on a home-row grip, so a user can work
through a quiz without looking down and without leaving the mouse.

- **What can be bound.** Each action can have up to three bindings in each set.
  A mouse binding can be:
  - a mouse button: 1 (left), 2 (right), 3 (middle), 4 (back) or 5 (forward)
  - a wheel direction: up or down

  A keyboard binding is any key. Either can be combined with any mix of Ctrl,
  Shift, Alt and Meta.
- **Where bindings act.** Mouse and wheel bindings act only inside the quiz
  area. Key bindings act anywhere on the player page except text fields outside
  the quiz area, such as the preferences menu.
- **Editing.** Bindings are changed in **Controls**, on the Account page and in
  the player's preferences menu. The user chooses **Add binding** and then
  presses the key, or clicks or scrolls, inside a capture box. Escape cancels
  capture and cannot itself be bound.
- **Rules.**
  - A stroke can belong to only one action; binding it to another action moves
    it there, with a notice. Mouse and keyboard strokes never collide with each
    other, so `M` and Mouse 2 are unrelated.
  - Every action must keep at least one binding in each enabled set.
  - **Reset to defaults** restores the table above.
- **Keyboard layouts.** Keys are recorded by physical position
  (`KeyboardEvent.code`) and displayed using the user's keyboard layout where
  the browser supports it (`navigator.keyboard.getLayoutMap()`).
- **Strokes the page may not receive.** Some belong to the browser or operating
  system: Ctrl+W, Cmd+Q, and on some systems the back and forward mouse
  buttons. The capture box warns when a stroke is one of these.
- **Wheel bindings** act at most once every 150 ms, so one flick of the wheel is
  one action.
- **Sync.** Bindings and the two on/off settings sync across devices along with
  the other preferences.

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
- **Customization.** The zones are fixed and always active; only the mouse and
  keyboard shortcuts can be customized or turned off.

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
- `Level 1: 92%, and every question right. Cascade complete in 4 levels and 9
  attempts.` This one leads to the completion screen, which offers **Study it
  again** (the reset Source quiz), **Start over** (a fresh cascade with the same
  filters) and **Back to cascades**.
- `Level 1 cleared with 87%. Down to Level 2 with its 5 missed questions.` The
  Source quiz is reset instead of cleared, so a pass with misses still goes
  down.

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
  search order for a cascade export; the dialog offers **alphabetical instead**.
- [Tiles](#tiles) are written plainly, with no brackets (`ANYS`), the same way
  they are shown on screen, so an export reads as a word list rather than as
  notation. Files are UTF-8 with a trailing newline, and CSV is RFC 4180 with a
  header row. Pasting an export back into an In Word List filter re-splits it by
  greedy matching (see [Tiles](#tiles)), which recovers the original tiles for
  every word whose letters do not also spell a different tiling; the tile
  palette is there for those rare entries.
- The file is named after the source, e.g. `CSW24 7s — L2 missed.txt`, with
  characters that filenames can't hold replaced.

**How it is produced.** The export is built on the device from IndexedDB, so it
works offline for a downloaded cascade, in a worker and in chunks so a
300,000-question list doesn't block the page, and handed over as a Blob. Answers,
definitions and hooks come from the cascade's answer cards, and definitions and
hooks are only in the local cards if the user's preferences asked for them, so an
export that needs something the device doesn't have falls back to
`GET /api/cascades/:id/export`, which streams the same file from the server. When
that is needed and there is no connection, the dialog says so and offers to
export the questions alone, which never needs anything but local data.

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
| `letter` | How the tile is written: one or more characters, e.g. `A`, `Ą`, `Ç`, `NY`, `L·L`. Unique within the file. Apart from the blank's `?`, it cannot contain `[`, `]`, `,`, `?`, `*`, `_` or whitespace. At most MAGPIE's `MAX_LETTER_BYTE_LENGTH` bytes. |
| `blank_letter` | How the tile is written when a blank stands for it, conventionally the lower-case form (`a`, `ą`, `ny`, `l·l`). Unique within the file, with the same character rules. |
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
| `value` | Decimal number. |

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

An admin can delete an item once **no active cascade or quiz** references it.
Only live study counts: a cascade in the Trash, and a cleared quiz waiting to be
purged, do not hold an item back. The admin page disables the delete button and
explains what is still using the item:

- **A letter distribution** is in use while any lexicon refers to it.
- **A lexicon** is in use while it has leave values, or while any active cascade
  or In Lexicon filter refers to it.
- **A lexicon's leave values** are in use while any active Leave Value cascade
  refers to them.

Deleting an item that only trashed cascades and cleared quizzes reference purges
them in the same transaction, exactly as
[purging](#trash-restore-and-purge) would: their quizzes, questions, attempts and
question indexes are deleted, tombstones are written, and each owner's sync
sequence is bumped, so every device drops them on its next pull. The refusal and
the confirmation both name what will go: `Deleting CSW24 will also delete 3
trashed cascades belonging to 2 users. 1 active cascade still uses it, so it
cannot be deleted yet.` Deleting an unreferenced lexicon or leave value set
cascades to its words or values as before.

### Loading changes into running servers

After an upload or deletion commits, the backend runs
`NOTIFY catalog_changed`. Every backend instance `LISTEN`s on that channel and
reconciles its in-memory indexes with the database: it builds indexes for new
items and drops deleted ones. It also reconciles every 60 seconds in case a
notification is missed. A new item appears in the cascade builder once every
instance has loaded it; until then, the item is marked **loading** in `/admin`.

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
- **MAGPIE notation** is how tile sequences are written in upload files, stored
  keys and the API. Single-character tiles are written as they are, and
  multi-character tiles in square brackets: `A[NY]S`, `?A[L·L]`. The notation is
  unambiguous. In MAGPIE, lower-case tiles (a tile's `blank_letter`) mean a blank
  standing for that tile. Wordfall never stores them.
- **Display never uses brackets.** Everywhere a tile sequence is shown to a
  user — questions, answers, alphagrams, leaves, the ladder, cascade names,
  banners, exports — each tile is shown as its plain `letter`, so a Catalan
  word reads `ANYS`, not `A[NY]S`. A multi-character tile is drawn as one
  joined tile, with the letters of a tile kept together when a line wraps, so
  `ANYS` still visibly reads as three tiles. Brackets belong to MAGPIE
  notation, which is for upload files, stored keys and the API only.
- **Typed text** in filter inputs, In Word List entries and typed-mode answers
  is converted to tiles in three steps:
  1. Upper-case it.
  2. Treat any bracketed group as a multi-character tile (`A[NY]S`), except in
     pattern inputs.
  3. Match the remaining text greedily, longest tile first, so `ANYS` becomes
     `A`, `NY`, `S`.

  In pattern inputs, square brackets already mean a set of tiles (see
  [Pattern syntax](#pattern-syntax)). There, multi-character tiles are typed
  plainly or inserted from the tile palette. Palette tiles are inserted whole and
  never re-split. That is how to enter a sequence that greedy matching would
  otherwise join, such as a separate `N` followed by `Y`.
- **Blank.** Written and displayed as `?`. In filter inputs `?` (and its synonym
  `.`) means "any tile", so a literal blank is typed as `_`.
- **Vowels and point values** come from the distribution, so Number of Vowels,
  Consists of `AEIOU`-style sets, Point Value and probability all work for any
  language.

Words and leaves both use the **lexicon's** letter distribution.

---

## Filters

There are 23 filters: 20 of Zyzzyva's condition types, in the order of its
search dropdown, then Wordfall's **Leave Value** filter and the two **inner
hook** filters. Zyzzyva's
Belongs to Group condition is deliberately left out; the other filters are
enough to build any study list. A filter has a type, a Not flag, and
parameters, and it is either a **predicate** or a **limit**:

- A **predicate** tests one candidate on its own. Zyzzyva checks predicates in
  three phases for speed: a word graph walk, then SQL, then post-processing. That
  split is an optimization, not part of their meaning. Wordfall evaluates every
  predicate in memory (see [Search Engine](#search-engine)).
- A **limit** (Limit by Probability Order, Limit by Playability Order) ranks
  the candidates that passed every predicate and keeps a range of that ranking.
  Limits are **always applied last, after all predicates**, however the rows are
  ordered.

### Pattern syntax

Anagram Match, Pattern Match and Subanagram Match take a pattern made of tiles
plus:

| Token | Meaning |
|---|---|
| `?` or `.` | Any single tile. The two are identical; `.` is accepted because Zyzzyva users and regular-expression habits both reach for it. Patterns are stored normalized to `?`. |
| `*` | Any number of tiles, including none. More than one `*` in an anagram or subanagram pattern means the same as one. |
| `[ABC]` | Exactly one tile from the set |
| `_` | A literal blank (Leave Value quizzes only) |

Input is upper-cased as it is typed and converted to tiles, with multi-character
tiles matched greedily or inserted from the tile palette (see [Tiles](#tiles)).
Every `.` outside a bracket set is read as `?` before matching, so a
distribution can still have a tile written with a `.`-free `letter` only; `.`
inside a bracket set is a literal member of that set only if the distribution
has such a tile, and is otherwise a validation error.
Brackets in a pattern always mean a tile set, never MAGPIE's multi-character
notation, so `[A NY]` and `[ANY]` both mean "`A` or `NY`" in a Catalan lexicon.
A tile that is not in the distribution, an unbalanced bracket, or an empty
bracket fails validation.

### Filter reference

Unless a row says otherwise, "word" means the candidate: a word in Anagram and
Definition quizzes, or a leave in Leave Value quizzes. Lengths and counts are in
tiles.

| # | Filter | Parameters | Not | Meaning (as implemented in Zyzzyva) |
|---|---|---|---|---|
| 1 | **Anagram Match** | pattern | ✓ | The word uses exactly the pattern's tiles, in any order. `?` and `[..]` each stand for one tile; `*` allows any number of extra tiles. `ETX?` → EXIT, NEXT, SEXT, TEXT, VEXT. |
| 2 | **Pattern Match** | pattern | ✓ | The word matches the pattern in order, left to right. `T?P` → TAP, TIP, TOP, TUP. `?W*M?S` → SWAMIS, SWAMPS, TWASOMES, … |
| 3 | **Subanagram Match** | pattern | ✓ | Every tile of the word can be taken from the pattern; not every pattern tile has to be used. `LX?` → AL, AX, EL, … LAX, LEX, LOX, LUX. A `*` matches everything. |
| 4 | **Length** | min, max (1–15) | — | The word has min–max tiles. Setting min = max gives an exact length. |
| 5 | **In Lexicon** | lexicon | ✓ | The word is also valid in a second lexicon. Negated, it finds words that are new or unique compared with that lexicon, e.g. CSW24 words not in CSW21. |
| 6 | **In Word List** | list of words | ✓ | The word appears in a list the user pastes or uploads (one word per line, up to 300,000 entries). The list is saved with the filter. Entries that are not valid in the cascade's lexicon are ignored. |
| 7 | **Number of Vowels** | min, max | — | Count of the distribution's vowel tiles is within min–max. |
| 8 | **Includes Letters** | tiles | ✓ | Each tile appears in the word at least as many times as it appears in the parameter (`EE` means two or more Es). Negated, the word contains **none** of the tiles: `Includes Q` plus `Not Includes U` finds Q-without-U words. |
| 9 | **Probability Order** | min, max, blanks (0–2), lax | — | The word's precomputed probability rank among **all words of the same length** in the lexicon is within min–max. See [Probability](#probability-and-probability-order). |
| 10 | **Limit by Probability Order** | min, max, blanks (0–2), lax | — | *Limit.* Rank the words that survived every predicate by probability, then keep ranks min–max of that list. Example: `Length 7`, `Includes V`, `Limit 1–50` gives the 50 most probable 7s with a V. |
| 11 | **Playability Order** | min, max, lax | — | The word's precomputed playability rank among all words of the same length is within min–max. |
| 12 | **Limit by Playability Order** | min, max, lax | — | *Limit.* Like Limit by Probability Order, but ranks by playability value. |
| 13 | **Number of Unique Letters** | min, max | — | Count of distinct tiles is within min–max. |
| 14 | **Point Value** | min, max | — | Sum of tile values is within min–max. Tiles count at face value even when a word needs a blank (ZYZZYVA = 43 in English). The maximum allowed is 15 × the distribution's highest tile value. |
| 15 | **Takes Prefix** | tiles | ✓ | Prefix + word is also a valid word. `PRE` with `VAL*` keeps VALENCE but not VALID. |
| 16 | **Takes Suffix** | tiles | ✓ | Word + suffix is also a valid word. |
| 17 | **Part of Speech** | one of: Adjective, Adverb, Conjunction, Definite Article, Indefinite Article, Interjection, Noun, Preposition, Pronoun, Verb | ✓ | The definition contains that part-of-speech tag in brackets: `[adj`, `[adv`, `[conj`, `[definite_article`, `[indefinite_article`, `[interj`, `[n`, `[prep`, `[pron`, `[v`. Zyzzyva matches `[tag ` (tag, then a space) or `[tag]`, so `[n -S]` and `[n]` are nouns and `[interj]` is not. |
| 18 | **Definition** | text | ✓ | The definition contains the text as a literal, case-insensitive substring. No wildcards. |
| 19 | **Consists of** | tiles, min %, max % | — | `floor(100 × (tiles of the word that are in the set) / length)` is within min–max. Example: `AEIOU`, 70–100 finds words that are at least 70% vowels. |
| 20 | **Number of Anagrams** | min, max | — | The number of valid words with this word's alphagram (including itself) is within min–max. |
| 21 | **Leave Value** *(Wordfall only)* | min, max (decimals; either may be blank) | — | *Leave Value quizzes only.* The leave's stored value is between min and max, inclusive. A blank bound is open, so `min 10, max blank` means "worth at least 10". At least one bound is required, and min ≤ max when both are given. The comparison uses the full stored value, not the rounded display value. |
| 22 | **Has Inner Front Hook** | — | ✓ | The word with its **first** tile removed is also a valid word in the cascade's lexicon, so the word is a front hook of a shorter word. `SHEAR` qualifies because `HEAR` is valid; `SHEAF` does not, because `HEAF` is not a word. Negated, it finds words whose first tile cannot be dropped. A one-tile word never qualifies. |
| 23 | **Has Inner Back Hook** | — | ✓ | The word with its **last** tile removed is also a valid word. `HEARS` qualifies because `HEAR` is valid. Negated, it finds words whose last tile cannot be dropped. A one-tile word never qualifies. |

Details that are easy to get wrong:

- **Range defaults.** A new integer range row starts at min 0 and the maximum
  allowed value. A row is valid only if it actually narrows something (min > 0
  or max below the ceiling) and min ≤ max. Leave Value rows start with both
  bounds blank and are invalid until one is filled in.
- **Lax** (the order filters). Every word has a unique rank, plus the lowest and
  highest rank shared by words with the *same* value (`min_order`, `max_order`).
  Ties are broken by alphagram, then by the word. Strict mode compares the
  unique rank. Lax mode matches any word whose tie range overlaps min–max,
  meaning `max_order ≥ min && min_order ≤ max`, so a whole group of tied words
  is taken or left together. Lax is on by default, as in Zyzzyva.
- **Lax for the limit filters.** The survivors are ranked by (value descending,
  alphagram, word), and the kept slice is widened in both directions to include
  neighbours with an equal value. If several limit rows of the same kind (and
  the same blank count) are given, the ranges intersect: the highest min and the
  lowest max.
- **Several rows of one type** are combined like any other rows. Two Length rows
  joined with **and** intersect; two Includes Letters rows joined with **and**
  both have to hold.

### Combining filters: AND and OR

Each row after the first carries a **join** to the row above, either **and** or
**or**. **And binds tighter than or**, so the rows read as a list of
or-separated groups, each group a run of and-joined rows: a word matches when it
matches every row of at least one group. There are no parentheses and no
nesting; two levels cover the study lists people actually build, and a flat list
of rows with a join on each one stays readable.

```
Length 7–7                    ← group 1
and  Includes Letters  Q      ← group 1
or   Length 8–8               ← group 2
and  Includes Letters  Z      ← group 2
```

matches seven-letter words with a `Q` together with eight-letter words with a
`Z`.

- **Not** negates its own row only, never a group.
- **Limit rows** (Limit by Probability Order, Limit by Playability Order) are not
  predicates: they rank whatever survived and keep a slice, and they are always
  applied last to the whole result. A limit row therefore cannot be joined with
  **or**; the builder disables the choice on those rows and explains why. Several
  limit rows still intersect as before.
- **An empty group is impossible**: removing the only row of a group removes the
  group, and the first row of the list never shows a join.
- The generated cascade name summarizes groups with `or` between them, e.g.
  `CSW24 · Length 7–7 + Q  or  Length 8–8 + Z`.

### Probability and probability order

Following Zyzzyva's `LetterBag::getNumCombinations`, a word's **combinations**
with `b` blanks (0, 1 or 2) is the number of distinct draws from the bag that
spell the word:

- `b = 0`: the product over each distinct tile of `C(count in bag, count in word)`.
- `b = 1`: the 0-blank figure, plus, for each distinct tile, `C(blanks, 1)`
  times the product with that tile's count reduced by one.
- `b = 2`: the 1-blank figure, plus, for each unordered pair of tile slots (the
  same tile twice is allowed when its count permits), `C(blanks, 2)` times the
  product with both counts reduced.

With no blanks in the distribution, the blank terms are zero. Probability order
`b` ranks all words of a length by combinations `b`, highest first, with ties
broken by alphagram and then word.

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
no word-only attributes (validity, definition, playability, hooks, inner
hooks), so filters
that depend on those do not apply. Leave probability uses the lexicon's letter
distribution, with the blank treated as an ordinary tile.

| Filter | Anagram | Definition | Leave Value |
|---|---|---|---|
| Anagram / Pattern / Subanagram Match | ✓ | ✓ | ✓ (Pattern Match matches against the alphabetized leave) |
| Length | ✓ | ✓ | ✓ (1–6) |
| In Lexicon | ✓ | ✓ | — |
| In Word List | ✓ | ✓ | ✓ (the list holds leaves; each entry is put in canonical order on input) |
| Number of Vowels | ✓ | ✓ | ✓ |
| Includes Letters | ✓ | ✓ | ✓ |
| Probability Order / Limit by Probability Order | ✓ | ✓ | ✓ (ranked among leaves of the same size; the blanks parameter is hidden) |
| Playability Order / Limit by Playability Order | ✓ | ✓ | — |
| Number of Unique Letters | ✓ | ✓ | ✓ |
| Point Value | ✓ | ✓ | ✓ (a blank is worth 0) |
| Takes Prefix / Takes Suffix | ✓ | ✓ | — |
| Part of Speech / Definition | ✓ | ✓ | — |
| Consists of | ✓ | ✓ | ✓ |
| Number of Anagrams | ✓ | ✓ | ✓ (valid words in the lexicon using exactly the leave's tiles; 0 if the leave contains a blank) |
| Leave Value | — | — | ✓ |
| Has Inner Front Hook / Has Inner Back Hook | ✓ | ✓ | — |

Changing the quiz type on the creation form keeps the filter rows that still
apply and flags the ones that don't, rather than deleting them silently.

---

## Architecture

```
Browser
├── Service worker        (caches the app so it loads offline)
├── IndexedDB             (cascades, quizzes, grades, answer cards, outbox of operations)
└── Sync engine ──HTTPS──▶ ALB ──▶ ECS Fargate task
                                   ├── nginx      (SvelteKit static build; proxies /api)
                                   └── wordfall   (Axum; in-memory catalog indexes;
                                         │  ▲      cascade rules; sync; purge task)
                                         ▼  │ LISTEN catalog_changed
                                    RDS Postgres  (users, preferences, catalog, cascades,
                                                   quizzes, grades, sync bookkeeping)
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
| Rate limiting | `governor` middleware (in-memory token buckets) |
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
| `docker/` | Backend Dockerfile (multi-stage Rust build → `debian:bookworm-slim`). |
| `infra/` | Terraform. |
| `scripts/` | `dev.py` (bring up the stack, create a confirmed admin dev user, upload catalog files through the API), backup and restore scripts. |
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
- `front_hooks` and `back_hooks` (for the hooks display preference), and
  `has_inner_front_hook` / `has_inner_back_hook`, each a bit set when the word
  without its first (or last) tile is itself in the lexicon (filters 22 and 23)
- `combinations[0..=2]`, `probability_order[b]`, `min_probability_order[b]`
  and `max_probability_order[b]`
- `playability_order`, `min_playability_order`, `max_playability_order`
- parsed parts of speech from the definition tags

Each leave value set gets a `LeaveSetIndex` with the same tile-based attributes
for every leave (length, vowels, unique letters, point value, combinations and
probability order within size, anagram count against its lexicon), plus the
value.

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

`/health` reports ready only once every catalog item in the database has been
indexed at startup. Items that arrive later are built in the background and
never block requests.

---

## Search Engine

`backend/src/search/` is a pure Rust module with no I/O:

```rust
pub struct SearchSpec { pub conditions: Vec<Condition> }

// `join_prev` on each condition after the first makes the list a disjunction of
// conjunctive groups; `groups()` splits it at every `Or`. Limit conditions are
// pulled out first and applied to the union.

pub enum ConditionKind {
    AnagramMatch(Pattern), PatternMatch(Pattern), SubanagramMatch(Pattern),
    Length(Range), InLexicon(LexiconId), InWordList(HashSet<TileString>),
    NumVowels(Range), IncludesLetters(TileCounts),
    ProbabilityOrder { range: Range, blanks: u8, lax: bool },
    LimitByProbabilityOrder { range: Range, blanks: u8, lax: bool },
    PlayabilityOrder { range: Range, lax: bool },
    LimitByPlayabilityOrder { range: Range, lax: bool },
    NumUniqueLetters(Range), PointValue(Range),
    TakesPrefix(TileString), TakesSuffix(TileString),
    PartOfSpeech(Pos), Definition(String),
    ConsistsOf { tiles: TileSet, min_pct: u8, max_pct: u8 },
    NumAnagrams(Range),
    LeaveValue { min: Option<f64>, max: Option<f64> },
    HasInnerFrontHook, HasInnerBackHook,
}

pub struct Condition { pub kind: ConditionKind, pub negated: bool, pub join_prev: JoinOp }

pub enum JoinOp { And, Or }

pub enum Target<'a> { Words(&'a LexiconIndex), Leaves(&'a LeaveSetIndex) }

pub fn search(target: Target, catalog: &Catalog, quiz_type: QuizType,
              spec: &SearchSpec) -> Result<Vec<QuestionKey>, SearchError>;
```

Filter inputs arrive as typed text and are converted to tiles against the
target's distribution during validation, so the engine itself only compares
small tile indexes (`u8`).

How a search runs:

1. **Validate** the spec against the quiz type and target: applicability,
   negation allowed, ranges, pattern syntax, tiles present in the distribution.
   It returns every error, keyed by row index, so the form can mark each bad
   row.
2. **Pick candidates.** Words or leaves of the target. The shortcuts hold only
   when **every** group supports them: if each group has a Length row, iterate
   the union of their ranges' per-length buckets, and if each group has an exact
   Anagram Match with no `*`, start from the union of those alphagram entries.
   A single group without such a row means a full scan. These are shortcuts
   only; the results must match a full scan.
3. **Apply predicates** to each candidate, one group at a time. Within a group
   the predicates run cheapest first — integer and leave value ranges, then tile
   counts, then patterns, then definition substring scans — and stop at the
   first failure. The candidate is kept as soon as one group accepts it, so
   groups are tried cheapest-group-first and later groups are skipped. With a
   single group this is exactly the old behaviour.
4. **Apply limits** to the survivors, grouped by (kind, blanks), with lax
   widening as described in [Filters](#filters).
5. **Make questions.** Anagram: dedupe alphagrams. Definition: words.
   Leave Value: canonical leaves. The result is sorted deterministically. That
   order becomes the cascade's question index (see [Cascades](#cascades)).

Pattern matching:

- **Anagram and Subanagram** compare tile count vectors. `?` (or `.`) and bracket sets
  are matched by a small bipartite assignment: sets are few and short, so a
  greedy most-constrained-first assignment with backtracking is enough.
- **Pattern Match** compiles to an anchored matcher over tile indexes (`?`/`.` → any
  one tile, `*` → any run, `[..]` → a tile set). Compiled patterns are cached
  per request.

The search runs on `tokio::task::spawn_blocking`. A search over a full lexicon
is expected to take tens of milliseconds, and a `SEARCH_TIMEOUT_MS` budget
(default 2 seconds) returns `422` with "search too broad" rather than tying up
a worker thread.

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
2. Seed **SplitMix64** with the seed.
3. Run **Fisher–Yates** from the last position down to position 1, swapping
   position `i` with `j = next_u64() mod (i + 1)`.

The Rust and TypeScript versions are checked against the same test vectors. The
TypeScript version uses `BigInt` for the 64-bit arithmetic.

### Rule implementation

The rules in [Cascade Rules](#cascade-rules) are pure functions, implemented in
`backend/src/cascade/` and `frontend/src/lib/cascade/`:

```
finish(cascade, quiz, grades, shuffle_seed, new_quiz_id) → Outcome
    Finished { cleared: bool, replacement: Option<NewQuiz> }
                                                     // quiz to the Trash; replacement at the same
                                                     // level, or none (level removed). `cleared`
                                                     // says whether the score met the threshold.
                                                     // Never returned for the Source quiz.
    Descended { reset_seed, new_level: NewQuiz }     // quiz reset; new quiz at level + 1. Ladder,
                                                     // and the Source quiz whenever it has misses
    Reshuffled { reset_seed, completed: bool }       // reset in place: nothing correct, or the
                                                     // Source quiz with no misses, which also
                                                     // completes the cascade

next_boundary(quiz) → Option<position>               // smallest multiple of the quiz's segment size
                                                     // above its cursor and below its question count

finish_segment(cascade, quiz, grades, segment_end, shuffle_seed, new_quiz_id) → SegmentOutcome
    Drilled { new_level: NewQuiz }                   // the run's misses, one level down, Drill
    Continued                                        // nothing missed in the run
                                                     // both move the cursor to segment_end

restore_quiz(cascade, quiz, shuffle_seed) → Restored  // pushed as the new deepest level
```

- `finish` reads the cascade's **progression** to choose between `Finished` and
  `Descended`, and returns `Descended` or `Reshuffled` for the Source quiz under
  either progression, because the Source quiz is never cleared, and every quiz these functions create takes its
  [options](#quiz-options) from the **cascade** row, except a segment quiz, which
  is always Drill. Both sides read the same cascade row, so both build the same
  quiz.
- One `shuffle_seed` in an operation derives every shuffle that operation needs.
  The replacement or new level uses `seed`, and a reset uses
  `seed ^ 0x9E3779B97F4A7C15`, so a single number keeps both sides in agreement.

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
- **Answer cards** are downloaded in pages of 10,000, compressed. Anagram cards
  without definitions come to roughly 10–15 MB uncompressed for 300,000
  questions, with definitions considerably more. Definitions and hooks are only
  included when the user's preferences ask for them.
- **Sync pulls** are paged, so a first sync on a new device never builds one
  enormous response.
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
| Creating a cascade (search runs on the server) | Studying any downloaded cascade in both answer modes |
| Start over (creates a new cascade) | Finishing quizzes: clearing, going down a level, going back up, resetting the Source quiz, and completing a cascade |
| Admin and account changes other than preferences | Trash: restoring quizzes and cascades, Delete forever |
| Downloading a cascade's answer cards | Changing preferences, controls and quiz options |
| Exporting answers or definitions the device has not downloaded | Exporting anything the device already has |

**Creating a cascade is the only part of the cascade life cycle that needs a
connection.** The search runs on the server against its in-memory lexicon
indexes, which the device does not have, so `/cascades/new` says so and disables
**Create Cascade** while offline. Everything that happens to a cascade
afterwards is local: once the cascade's answer cards are downloaded, the device
can grade every question, finish a level, go down a level with the misses, clear
a level and climb back up, reset the Source quiz, and complete the cascade,
all with no connection. It can do this for as many finishes as the user has
patience for, because [the cascade rules](#cascade-rules) are pure functions
that exist in TypeScript as well as Rust, every level's questions come from the
Source quiz's already-downloaded question index and answer cards, and new quiz
ids and shuffle seeds are generated on the device. The server replays the same
operations against the same rules when the device syncs and arrives at the same
state.

The one thing an offline descent cannot do is pull answer cards it never had:
a device that went offline mid-download has the cascade marked **partly
downloaded** and refuses to start a quiz whose cards are missing, rather than
showing blank answers.

### On the device

**App shell.** The service worker precaches the built app (SvelteKit's
`$service-worker` `build` and `files` lists) and answers every navigation with
the cached `index.html`, so `/cascades/:id` loads with no connection. It never
caches `/api` responses; downloaded data lives in IndexedDB instead. A new app
version waits and is activated on the next load when no attempt is in progress,
and IndexedDB schema upgrades run from versioned migrations.

**IndexedDB stores**, all scoped to the user id so two accounts on one device
never mix:

| Store | Contents |
|---|---|
| `meta` | user id, username, `device_id` (a UUID made once per device), sync cursor, last sync time |
| `preferences` | the user's preferences |
| `distributions` | the tiles (letter, blank letter, value, vowel) of every distribution the user's cascades use |
| `cascades`, `quizzes`, `quiz_questions`, `quiz_attempts` | local copies of the server rows |
| `cards` | answer cards per cascade, keyed by `(cascade_id, idx)` |
| `outbox` | operations not yet accepted by the server, in `device_seq` order |

**Downloads.**
- A cascade starts downloading the moment it is created, so a cascade made while
  the connection is still up is ready to study once it is gone.
- Every cascade with activity in the last 14 days is kept downloaded, plus any
  cascade the user marks **Keep offline**.
- The player shows **Available offline** or download progress. While a download
  is still running and the device is online, the player fetches the pages it
  needs on demand, so studying never waits for a download to finish. A cascade
  whose download did not finish before the connection went is shown as **partly
  downloaded**, and a quiz whose cards are missing cannot be started until the
  device is online again.
- Card data above a 500 MB soft limit is evicted, least recently used cascade
  first. Only card data is ever evicted, never unsynced operations.
- The app calls `navigator.storage.persist()` on first login. The Account page
  notes that some browsers, notably Safari, can clear site data after a period
  of not being used.

### Operations

| Operation | Fields | Server applies it when… | Effect |
|---|---|---|---|
| `grade` | quiz, attempt, question `idx`, grade, graded at | the quiz is active, the attempt matches, and no later grade for that question exists | Set the grade (the latest `graded_at` wins) and update counters |
| `move_cursor` | quiz, attempt, position, at | the quiz is active and the attempt matches | Set the cursor (latest wins) |
| `finish` | quiz, attempt, shuffle seed, new quiz id | the quiz is active, is at the deepest level, the attempt matches, and every question is graded | Apply [Cascade Rules](#cascade-rules) using the server's grades and the cascade's progression; any new quiz uses the device's id; record the attempt |
| `finish_segment` | quiz, attempt, segment end, shuffle seed, new quiz id | the quiz is active, is at the deepest level, the attempt matches, its segment size is greater than 0, the segment end is a multiple of it strictly between 0 and the question count, every question before the segment end is graded, and no quiz already exists for this quiz, attempt and segment end | Create the drill quiz one level down from the run's misses (nothing if there are none) and move the cursor to the segment end. See [Segments](#segments) |
| `set_cascade_options` | cascade, changed option fields, at | the cascade exists and is not purged | Set the fields (latest `at` wins). Only later quizzes are affected |
| `set_quiz_options` | quiz, changed option fields, at | the quiz is active | Set the fields (latest `at` wins) |
| `restore_quiz` | quiz, shuffle seed | the quiz is cleared and not purged | Push it back as the new deepest level; bring back its cascade if it was trashed |
| `trash_cascade` | cascade | the cascade is not trashed | Trash it |
| `restore_cascade` | cascade | the cascade is trashed and not purged | Restore it as it was |
| `purge_quiz` | quiz | the quiz is cleared | Delete it permanently and record a tombstone |
| `purge_cascade` | cascade | the cascade is trashed | Delete it and all its quizzes permanently and record tombstones |
| `set_preferences` | changed fields, at | always | Set the fields (latest `at` wins) |
| `set_bindings` | the full list of bindings, at | the list is valid: every action has 1–3 bindings in each set and no stroke is used twice | Replace the bindings (latest `at` wins) |

Every operation also carries its `id`, `device_id`, `device_seq` and the device's
timestamp.

### The sync cycle

`POST /api/sync` does a **push** and then a **pull**, in one request.

**Push.**
1. The server locks the user row and takes a new value of the user's sync
   sequence: `UPDATE users SET sync_seq = sync_seq + 1 RETURNING sync_seq`. The
   lock also means two syncs for the same user never run concurrently. Every row
   this request changes is stamped with that value.
2. Operations are applied in order, one savepoint each. An operation whose `id`
   has already been recorded returns its recorded result without being applied
   again.
3. Each operation is recorded as `applied` or `rejected`, with a reason. One
   rejection does not stop the rest of the batch.

**Pull.**
1. The server returns every row belonging to the user with `updated_seq` greater
   than the device's cursor: cascades, quizzes, changed quiz questions, attempts,
   preferences, and tombstones for purged items. Pages run up to 50,000 question
   rows, with a page token for the rest.
2. When the last page arrives, the device's cursor is set to the sync sequence
   the push took.
3. If the device's cursor is older than the oldest tombstone the server still
   keeps (tombstones are pruned after 90 days), the server replies
   `resync_required`. The device then pushes any remaining operations and
   replaces all its local rows with a full pull.

**Applying a pull on the device** works like a rebase:
1. Drop every outbox operation the server has now acknowledged, whether applied
   or rejected.
2. Write the server's rows into IndexedDB.
3. Replay the operations still waiting in the outbox on top, using the same
   cascade rules. Operations that no longer apply are dropped and reported (see
   below).
4. Refresh the player if what it was showing changed.

**When the device syncs:**
- half a second after a new operation, if online
- on the browser's `online` event
- when the tab becomes visible again
- every 30 seconds while the app is open
- right after logging in

Pushes are sent in batches of up to 500 operations. A batch with a large
`finish` still fits easily, because shuffle seeds replace orderings.

### Conflicts

Conflicts need **two devices changing the same cascade while at least one is
offline**. Using a single device never conflicts. The rules:

| Situation | Result |
|---|---|
| The same question graded on two devices in the same attempt | The grade with the later `graded_at` wins. |
| The same quiz finished on two devices | The first `finish` the server receives wins. The other device's `finish` is rejected because its attempt is out of date. Operations that depended on it are dropped, such as grades on the level it created locally. The device shows: "Level 3 was finished on another device. 42 answers from this device weren't kept." |
| A quiz restored on one device and purged on another | Whichever operation arrives first wins; the other is rejected. |
| Grades arriving for a quiz that has since been cleared or reset | Rejected silently. They belong to an attempt that no longer exists. |
| The same run finished on two devices | The first `finish_segment` wins. The second is rejected as a duplicate for that quiz, attempt and segment end, and the device rebases onto the drill quiz the first one created. |
| Quiz or cascade options changed on two devices | Each field keeps its latest change, like preferences. |
| A quiz created while the cascade's options were different | Nothing happens to it. Options are copied at creation and never revisited. |
| Preferences changed on two devices | Each field keeps its latest change. |

Rejected operations are never retried. After the rebase, the device shows what
the server has, and a notice appears only when the user's own work was dropped.

### Authentication while offline

- **Signed-in state.** The app keeps the signed-in user's id and username in
  IndexedDB and treats the user as signed in until a sync is answered with `401`.
- **Expired session.** A `401` during sync, from an expired session or "Sign out
  everywhere", keeps all local data and the outbox, and shows **Log in to
  sync**. Studying continues. Once the user logs in again, the outbox is pushed.
- **A different user.** If a different user logs in on the device, the previous
  user's local data stays, untouched, until that user logs in again.
- **Logging out.** The app tries to sync first. If operations are still unsent,
  it asks for confirmation before deleting that user's local data, stating how
  many changes will be lost.

---

## Schema

There is a single migration, `backend/migrations/0001_initial.sql`, run by SQLx
at startup (SQLx creates its own `_sqlx_migrations` table). It is edited in
place until the first deployment, after which migrations are append-only. The
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
CREATE TYPE join_op AS ENUM ('and', 'or');
CREATE TYPE input_set AS ENUM ('mouse', 'keyboard');

-- 'cleared' means finished and in the Trash, whether or not the score met the
-- clear threshold (see Progression). The Source quiz is never 'cleared'.
CREATE TYPE quiz_status AS ENUM ('active', 'cleared');

-- What a finish below the clear threshold does.
CREATE TYPE quiz_progression AS ENUM ('ladder', 'drill');

-- How a quiz came to exist.
CREATE TYPE quiz_origin AS ENUM (
    'source',             -- created from the filter search
    'clear_replacement',  -- the misses of a cleared quiz, at the same level
    'drill_replacement',  -- Drill progression: the misses of a quiz that was not cleared,
                          -- at the same level
    'descent',            -- the misses of a quiz that was reset rather than cleared, one level
                          -- down: Ladder progression, or any finish of the Source quiz with misses
    'segment'             -- the misses of one run of a quiz, one level down
);

CREATE TYPE finish_outcome AS ENUM (
    'cleared',     -- score met the threshold; quiz to the Trash
    'replaced',    -- Drill: score did not meet the threshold; quiz to the Trash anyway
    'descended',   -- quiz reset, misses one level down: Ladder, or the Source quiz with misses
    'reshuffled',  -- nothing correct; quiz reset in place
    'completed'    -- the Source quiz, at or above the threshold with no misses: reset in place
                   -- and the cascade is complete
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
    'leave_value',
    'has_inner_front_hook',
    'has_inner_back_hook'
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
                                  CHECK (default_clear_threshold BETWEEN 0 AND 100),
    leave_value_decimals      SMALLINT NOT NULL DEFAULT 1
                                  CHECK (leave_value_decimals BETWEEN 0 AND 3),
    anagram_show_definitions  BOOLEAN NOT NULL DEFAULT false,
    anagram_show_hooks        BOOLEAN NOT NULL DEFAULT false,
    anagram_answer_mode       anagram_answer_mode NOT NULL DEFAULT 'flashcard',
    anagram_show_found_count  BOOLEAN NOT NULL DEFAULT false,  -- 'X of Y found' in typed mode
    -- Defaults for new cascades only; changing one never touches an existing cascade.
    default_segment_size      INTEGER NOT NULL DEFAULT 0
                                  CHECK (default_segment_size BETWEEN 0 AND 300000),
    default_progression       quiz_progression NOT NULL DEFAULT 'ladder',
    default_require_alphabetical BOOLEAN NOT NULL DEFAULT false,
    -- Shortcut sets. Both on by default; off makes that set's bindings inert.
    mouse_shortcuts_enabled      BOOLEAN NOT NULL DEFAULT true,
    keyboard_shortcuts_enabled   BOOLEAN NOT NULL DEFAULT true,
    changed_at                TIMESTAMPTZ NOT NULL DEFAULT now(), -- device time of latest change
    bindings_changed_at       TIMESTAMPTZ NOT NULL DEFAULT now(), -- device time of latest binding change
    updated_seq               BIGINT NOT NULL DEFAULT 0           -- also bumped when bindings change
);

-- Player controls. The defaults are inserted with the user; every action keeps
-- at least one binding in each set.
CREATE TABLE user_input_bindings (
    user_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    action   input_action NOT NULL,
    -- 'mouse' for mouse buttons and the wheel, 'keyboard' for keys. Each set holds
    -- up to three bindings per action and is enabled or disabled on its own.
    set      input_set NOT NULL,
    slot     SMALLINT NOT NULL CHECK (slot BETWEEN 0 AND 2),
    kind     input_kind NOT NULL,
    code     TEXT NOT NULL,
    ctrl     BOOLEAN NOT NULL DEFAULT false,
    shift    BOOLEAN NOT NULL DEFAULT false,
    alt      BOOLEAN NOT NULL DEFAULT false,
    meta     BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (user_id, action, set, slot),
    UNIQUE (user_id, kind, code, ctrl, shift, alt, meta), -- one action per stroke
    CHECK (set = CASE kind WHEN 'key' THEN 'keyboard' ELSE 'mouse' END),
    CHECK (CASE kind
        WHEN 'mouse_button' THEN code IN ('1', '2', '3', '4', '5')
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
    letter                  TEXT NOT NULL CHECK (char_length(letter) BETWEEN 1 AND 16),
    blank_letter            TEXT NOT NULL CHECK (char_length(blank_letter) BETWEEN 1 AND 16),
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
    CHECK (position = 0 OR (letter       !~ '[\[\],?*_[:space:]]'
                        AND blank_letter !~ '[\[\],?*_[:space:]]')),
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
    word         TEXT NOT NULL CHECK (char_length(word) BETWEEN 1 AND 128
                                      AND position('?' IN word) = 0),
                     -- MAGPIE notation; 1–15 tiles, checked on upload
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
    leave         TEXT NOT NULL CHECK (char_length(leave) BETWEEN 1 AND 64),
                      -- MAGPIE notation; 1–6 tiles in distribution order, blanks first
    value         DOUBLE PRECISION NOT NULL
                      CHECK (value NOT IN ('NaN'::float8, 'Infinity'::float8, '-Infinity'::float8)),
    PRIMARY KEY (leave_set_id, leave)
);

-- -------------------------------------------------------------------------
-- Filter specifications (one per cascade)
-- -------------------------------------------------------------------------

CREATE TABLE search_specs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX search_specs_user_id ON search_specs (user_id);

CREATE TABLE search_conditions (
    spec_id               UUID NOT NULL REFERENCES search_specs (id) ON DELETE CASCADE,
    position              SMALLINT NOT NULL CHECK (position BETWEEN 0 AND 99),
    -- How this row joins the row above. Position 0 is always 'and'. 'and' binds
    -- tighter than 'or', so the rows are a disjunction of conjunctive groups.
    join_prev             join_op NOT NULL DEFAULT 'and',
    condition_type        condition_type NOT NULL,
    negated               BOOLEAN NOT NULL DEFAULT false,

    -- Parameters. Which ones are set depends on condition_type (see CHECK below).
    text_value            TEXT CHECK (length(text_value) BETWEEN 1 AND 200),
                              -- pattern, tiles, prefix, suffix or definition text,
                              -- stored as the user typed it
    part_of_speech_value  part_of_speech,
    other_lexicon_id      SMALLINT REFERENCES lexicons (id),
    min_value             INTEGER CHECK (min_value >= 0),
    max_value             INTEGER CHECK (max_value >= 0),
    min_leave_value       DOUBLE PRECISION,
    max_leave_value       DOUBLE PRECISION,
    blanks                SMALLINT CHECK (blanks BETWEEN 0 AND 2),
    lax                   BOOLEAN,

    PRIMARY KEY (spec_id, position),

    CHECK (min_value IS NULL OR max_value IS NULL OR min_value <= max_value),
    CHECK (min_leave_value IS NULL OR max_leave_value IS NULL
           OR min_leave_value <= max_leave_value),

    -- Not is only allowed where Zyzzyva allows it.
    CHECK (NOT negated OR condition_type IN (
        'anagram_match', 'pattern_match', 'subanagram_match', 'in_lexicon',
        'in_word_list', 'includes_letters', 'takes_prefix', 'takes_suffix',
        'part_of_speech', 'definition')),

    -- Exactly the parameters each type uses are present.
    CHECK (CASE
        WHEN condition_type IN ('anagram_match', 'pattern_match', 'subanagram_match',
                                'includes_letters', 'takes_prefix', 'takes_suffix',
                                'definition')
            THEN text_value IS NOT NULL
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 1
        WHEN condition_type IN ('length', 'num_vowels', 'num_unique_letters',
                                'point_value', 'num_anagrams')
            THEN num_nonnulls(min_value, max_value) = 2
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 2
        WHEN condition_type IN ('probability_order', 'limit_by_probability_order')
            THEN num_nonnulls(min_value, max_value, blanks, lax) = 4
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 4
        WHEN condition_type IN ('playability_order', 'limit_by_playability_order')
            THEN num_nonnulls(min_value, max_value, lax) = 3
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 3
        WHEN condition_type = 'consists_of'
            THEN num_nonnulls(text_value, min_value, max_value) = 3
             AND max_value <= 100
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 3
        WHEN condition_type = 'part_of_speech'
            THEN part_of_speech_value IS NOT NULL
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 1
        WHEN condition_type = 'in_lexicon'
            THEN other_lexicon_id IS NOT NULL
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 1
        WHEN condition_type = 'in_word_list'   -- entries live in search_condition_words
            THEN num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, min_leave_value,
                              max_leave_value, blanks, lax) = 0
        WHEN condition_type = 'leave_value'    -- either bound may be open, not both
            THEN num_nonnulls(min_leave_value, max_leave_value) >= 1
             AND num_nonnulls(text_value, part_of_speech_value, other_lexicon_id,
                              min_value, max_value, blanks, lax) = 0
    END)
);
CREATE INDEX search_conditions_other_lexicon_id
    ON search_conditions (other_lexicon_id) WHERE other_lexicon_id IS NOT NULL;

-- In Word List entries, as typed (upper-cased). Converted to tiles at search time.
CREATE TABLE search_condition_words (
    spec_id   UUID NOT NULL,
    position  SMALLINT NOT NULL,
    entry     TEXT NOT NULL CHECK (char_length(entry) BETWEEN 1 AND 128),
    PRIMARY KEY (spec_id, position, entry),
    FOREIGN KEY (spec_id, position)
        REFERENCES search_conditions (spec_id, position) ON DELETE CASCADE
);

-- -------------------------------------------------------------------------
-- Cascades and quizzes
-- -------------------------------------------------------------------------

CREATE TABLE cascades (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name              TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    quiz_type         quiz_type NOT NULL,
    lexicon_id        SMALLINT NOT NULL REFERENCES lexicons (id),
    leave_set_id      INTEGER,                    -- Leave Value cascades only
    spec_id           UUID NOT NULL REFERENCES search_specs (id),
                          -- a private copy, owned by this cascade
    clear_threshold   SMALLINT NOT NULL CHECK (clear_threshold BETWEEN 0 AND 100),

    -- Quiz options: what every quiz created for this cascade starts with.
    segment_size         INTEGER NOT NULL DEFAULT 0
                             CHECK (segment_size BETWEEN 0 AND 300000),  -- 0 = no segments
    progression          quiz_progression NOT NULL DEFAULT 'ladder',
    require_alphabetical BOOLEAN NOT NULL DEFAULT false,
    options_changed_at   TIMESTAMPTZ NOT NULL DEFAULT now(), -- device time, for latest-wins

    question_count    INTEGER NOT NULL CHECK (question_count BETWEEN 1 AND 300000),
    depth             INTEGER NOT NULL CHECK (depth >= 1), -- number of active levels; the
                                                  -- Source quiz is never cleared, so a live
                                                  -- cascade always has at least Level 1
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_activity_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at      TIMESTAMPTZ,                -- last time the Source quiz was finished at or
                                                  -- above the threshold with no misses
    trashed_at        TIMESTAMPTZ,                -- set by the user only
    updated_seq       BIGINT NOT NULL,

    -- The leave value set must belong to the cascade's lexicon.
    FOREIGN KEY (leave_set_id, lexicon_id) REFERENCES leave_sets (id, lexicon_id),

    CHECK ((quiz_type = 'leave_value') = (leave_set_id IS NOT NULL)),
    CHECK (completed_at IS NULL OR depth = 1)
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
    question_key  TEXT NOT NULL CHECK (char_length(question_key) BETWEEN 1 AND 128),
                      -- alphagram, word or canonical leave, in MAGPIE notation
    PRIMARY KEY (cascade_id, idx)
);

CREATE TABLE quizzes (
    id                UUID PRIMARY KEY,           -- device-generated, except the Source quiz
    cascade_id        UUID NOT NULL REFERENCES cascades (id) ON DELETE CASCADE,
    user_id           UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    level             INTEGER NOT NULL CHECK (level >= 1),
                          -- for a cleared quiz, the level it was cleared from
    origin            quiz_origin NOT NULL,
    origin_quiz_id    UUID REFERENCES quizzes (id) ON DELETE SET NULL,
    origin_attempt    INTEGER CHECK (origin_attempt >= 1),   -- the attempt it came out of
    origin_segment_end INTEGER CHECK (origin_segment_end >= 1),
                          -- segment quizzes: the run boundary it came from, in the parent's
                          -- positions. Recorded as a position, not a run number, so changing
                          -- the segment size mid-attempt cannot re-descend a finished run.
    status            quiz_status NOT NULL DEFAULT 'active',

    -- Quiz options, copied from the cascade at creation and editable afterwards.
    segment_size         INTEGER NOT NULL DEFAULT 0
                             CHECK (segment_size BETWEEN 0 AND 300000),
    progression          quiz_progression NOT NULL DEFAULT 'ladder',
    require_alphabetical BOOLEAN NOT NULL DEFAULT false,
    options_changed_at   TIMESTAMPTZ NOT NULL DEFAULT now(), -- device time, for latest-wins

    attempt           INTEGER NOT NULL DEFAULT 1 CHECK (attempt >= 1),
    question_count    INTEGER NOT NULL CHECK (question_count BETWEEN 1 AND 300000),
    correct_count     INTEGER NOT NULL DEFAULT 0 CHECK (correct_count >= 0),
    missed_count      INTEGER NOT NULL DEFAULT 0 CHECK (missed_count >= 0),
    cursor            INTEGER NOT NULL DEFAULT 0,
    cursor_moved_at   TIMESTAMPTZ,                -- device time, for latest-wins
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_activity_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    cleared_at        TIMESTAMPTZ,
    updated_seq       BIGINT NOT NULL,            -- also bumped when any of its questions change

    CHECK ((status = 'cleared') = (cleared_at IS NOT NULL)),
    CHECK (origin <> 'source' OR (origin_quiz_id IS NULL AND origin_attempt IS NULL)),
    CHECK ((origin = 'segment') = (origin_segment_end IS NOT NULL)),
    CHECK (origin <> 'segment' OR progression = 'drill'),  -- a run's misses always drill
    CHECK (correct_count + missed_count <= question_count),
    CHECK (cursor BETWEEN 0 AND question_count)
);
CREATE UNIQUE INDEX quizzes_one_active_per_level
    ON quizzes (cascade_id, level) WHERE status = 'active';
CREATE UNIQUE INDEX quizzes_one_source ON quizzes (cascade_id) WHERE origin = 'source';
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
    graded_at     TIMESTAMPTZ,                    -- device time, for latest-wins
    updated_seq   BIGINT NOT NULL,
    PRIMARY KEY (quiz_id, question_idx),
    CONSTRAINT quiz_questions_position_unique
        UNIQUE (quiz_id, position) DEFERRABLE INITIALLY DEFERRED,
    CHECK ((grade IS NULL) = (graded_at IS NULL))
);
CREATE INDEX quiz_questions_quiz_seq ON quiz_questions (quiz_id, updated_seq);

-- One row per finished attempt. Immutable.
CREATE TABLE quiz_attempts (
    quiz_id         UUID NOT NULL REFERENCES quizzes (id) ON DELETE CASCADE,
    attempt         INTEGER NOT NULL CHECK (attempt >= 1),
    question_count  INTEGER NOT NULL CHECK (question_count >= 1),
    correct_count   INTEGER NOT NULL CHECK (correct_count >= 0),
    missed_count    INTEGER NOT NULL CHECK (missed_count >= 0),
    outcome         finish_outcome NOT NULL,
    finished_at     TIMESTAMPTZ NOT NULL,         -- device time
    updated_seq     BIGINT NOT NULL,
    PRIMARY KEY (quiz_id, attempt),
    CHECK (correct_count + missed_count = question_count)
);

-- -------------------------------------------------------------------------
-- Sync bookkeeping
-- -------------------------------------------------------------------------

-- Every operation received, so a repeated operation is recognised. Pruned after 90 days.
CREATE TABLE sync_operations (
    id           UUID PRIMARY KEY,                -- device-generated operation id
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id    UUID NOT NULL,
    device_seq   BIGINT NOT NULL CHECK (device_seq >= 1),
    op_type      sync_op_type NOT NULL,
    status       sync_op_status NOT NULL,
    reason       TEXT,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, device_id, device_seq),
    CHECK ((status = 'rejected') = (reason IS NOT NULL))
);
CREATE INDEX sync_operations_received_at ON sync_operations (received_at);

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
```

Notes on the schema:

- **No JSONB.** Filter parameters are typed columns, and a per-type `CHECK`
  guarantees each stored condition has exactly the parameters its type uses.
  The Rust `ConditionKind` enum is loaded from these rows and written back to
  them. A round-trip test (see [Testing](#testing)) covers all 23 types. Sync
  operation payloads are not stored at all; only each operation's id, type and
  result are kept.
- **The cascade stack is enforced by the database where it can be.** At most one
  active quiz per level, exactly one Source quiz, at most one quiz per
  (parent quiz, attempt, run boundary), a segment quiz always on Drill
  progression, `depth >= 1` always, and a completed cascade is at `depth = 1`
  with its Source quiz active.
- **Quiz options are copied, never referenced.** `cascades` holds what new
  quizzes start with and `quizzes` holds what each quiz actually uses, so
  changing a cascade's options can never rewrite the rules a quiz in progress is
  being played under. Both sets of columns are plain typed columns with the same
  names, so the copy is one `INSERT … SELECT`.
- **Tiles are validated by the application.** A `CHECK` cannot parse MAGPIE
  notation against a distribution, so the upload validator guarantees:
  - every word parses into 1–15 non-blank tiles of its lexicon's distribution
  - every leave parses into 1–6 tiles of its lexicon's distribution, within bag
    counts, written in canonical order
  - a distribution's positions run 0..n−1 with no gaps, and no `letter` is
    longer than MAGPIE's `MAX_LETTER_BYTE_LENGTH`
- **Catalog deletion is refused while actively in use.** References to
  `letter_distributions`, `lexicons` and `leave_sets` from other catalog rows,
  cascades and search conditions have no `ON DELETE` action, so deleting a
  referenced item fails. The admin API therefore counts references first,
  separating active cascades from trashed ones: an active reference is refused
  with an explanation, and trashed cascades and their cleared quizzes are purged
  in the delete transaction before the catalog row is removed. Deleting an
  unreferenced lexicon or leave value set cascades to its words or values.
- **Deleting an account** is a single `DELETE FROM users`. Everything the user
  owns cascades from `users`. The reference between user-owned rows
  (`cascades.spec_id`) uses the default `NO ACTION`, which Postgres checks at the
  end of the statement. Cascaded deletes of both sides therefore succeed, while
  deleting a spec that is still in use fails.
  Catalog items the user uploaded stay, with `uploaded_by` set to `NULL`.
- **Purging a cascade** deletes its question index, quizzes, questions and
  attempts by cascade. In the same transaction, the application deletes the
  cascade's spec, writes tombstones for the cascade
  and each quiz, and bumps the user's sync sequence.
- **The 300,000 ceiling** lives in `cascades.question_count`,
  `quizzes.question_count`, `cascade_questions.idx` and `quiz_questions.position`,
  so no configuration can exceed it.
- **Invariants left to the application** because a `CHECK` cannot see other
  rows. Integration tests and the shared rule vectors cover each one.
  - `question_idx < cascades.question_count`, and `position < quizzes.question_count`
  - `quizzes.user_id` equals its cascade's `user_id`, and the spec belongs to the
    same user
  - `cascades.depth` equals the number of active quizzes, and the active levels
    are exactly `1..depth`
  - the counters on `quizzes` match its questions' grades
  - `search_condition_words` rows belong only to `in_word_list` conditions
  - `question_key` exists in the cascade's lexicon or leave value set
  - `word_count` and `leave_count` match their rows
  - a user has at most `MAX_CASCADES_PER_USER` cascades, counting the Trash
  - every input action has at least one binding in each set
  - a segment quiz's `origin_segment_end` is a multiple of its parent's segment
    size, is less than the parent's `question_count`, and its questions are
    exactly the questions its parent has graded missed in positions below that
    boundary and at or above the previous one
  - a quiz waiting because of a segment descent has its cursor at that boundary
  - a quiz's options were copied from its cascade when it was created, which
    only the creating code can guarantee, so the rule vectors check it

### Purge task

Each backend instance runs an hourly background task. It first takes
`pg_try_advisory_lock`, so only one instance purges at a time, and then:

- **purges** cleared quizzes whose `cleared_at` is older than
  `TRASH_RETENTION_DAYS`, and trashed cascades whose `trashed_at` is older than
  that, writing tombstones and bumping each affected user's sync sequence
- **prunes** `sync_operations` and `sync_tombstones` older than
  `SYNC_RETENTION_DAYS`, raising each affected user's `sync_floor_seq` to the
  highest tombstone sequence it removed

---

## Authentication

The flows are the conventional ones, built directly on Axum:

- **Register** (`/register`): username, email and password. The server
  validates all fields and returns `400` with **every** field error at once.
  Passwords are checked for strength with `zxcvbn` (score ≥ 3) and length.
  A taken username is reported as a field error, since usernames are not secret.
  An email already in use gets the same response as a successful registration,
  and the existing account's owner is emailed a notice instead, so the form
  cannot be used to find out which emails have accounts. The password is hashed
  with Argon2. A confirmation code (32 random bytes) is emailed, and only its
  SHA-256 is stored, with a 24-hour expiry. The `user_preferences` row and the
  default `user_input_bindings` are created with the user.
- **Confirm email** (`/confirm-email?code=…`): the page submits the code; on
  success the user is sent to `/login`. Login is refused with `403` until the
  email is confirmed.
- **Login**: rate limited per IP and per username (10 per minute each), checked
  before any Argon2 verify runs. On success the server sets:
  - a PASETO **v4.local** session token (32-byte key from
    `SESSION_SIGNING_KEY`) in an `httpOnly`, `SameSite=Strict` cookie named
    `wordfall_session`, `Secure` in production, with a 30-day TTL
  - a CSRF cookie for double-submit on every state-changing request

  The token carries the user id and the account's `session_generation`. Every
  request re-reads the user row, including `is_admin`, so a deleted account, a
  bumped generation, or a revoked admin flag takes effect immediately. The long
  TTL is deliberate: a device that has been offline for weeks can still sync as
  soon as it reconnects.
- **Password reset**: always answers with the same message. If the email
  belongs to a confirmed account, a 30-minute single-use token is emailed.
  Completing a reset spends every other outstanding reset token and increments
  `session_generation`, which signs out every session.
- **Sign out everywhere** and **delete account** (with password re-entry) are on
  `/account`.

Security generally:

- CSRF double-submit on every cookie-authenticated `POST`, `PUT`, `PATCH` and
  `DELETE`, including `/api/sync`.
- `/api/admin/*` requires `is_admin`, and non-admins get `404`, so the admin
  surface is not advertised.
- `governor` rate limits on auth endpoints, and per-user limits on search,
  preview, cascade creation, sync and admin uploads, which are the expensive
  calls.
- `429` responses carry `Retry-After`. The sync engine backs off accordingly.
- `TRUSTED_PROXY_HOPS` controls which `X-Forwarded-For` entry per-IP limits key
  on (1 behind the ALB and behind the compose Nginx).
- Every cascade, quiz, saved-search, preference and sync query filters by
  `user_id`. Another user's resource returns `404`, not `403`, and in a sync
  operation it is rejected as "not found".

---

## API

All endpoints are JSON under `/api`, except the multipart admin uploads. Every
endpoint except `auth/*`, `/health`, `GET /api/lexicons` and
`GET /api/letter-distributions/:name` requires a session.

### Auth and account

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/api/auth/register` | Create an account; sends the confirmation email |
| `POST` | `/api/auth/confirm-email` | Confirm with the emailed code |
| `POST` | `/api/auth/login` | Start a session |
| `POST` | `/api/auth/logout` | End this session |
| `POST` | `/api/auth/reset-password` | Request a reset email |
| `POST` | `/api/auth/reset-password/confirm` | Set a new password with a reset token |
| `GET` | `/api/auth/me` | `{ user_id, username, is_admin }` or `401` |
| `POST` | `/api/account/sign-out-everywhere` | Bump `session_generation` |
| `DELETE` | `/api/account` | Delete the account (requires password) |

Preferences are read and written through sync, not a separate endpoint.

### Catalog and search

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/lexicons` | `[{ name, letter_distribution, word_count, leave_count }]`, where `leave_count` is `null` for a lexicon without leave values. Only items indexed by this instance are listed. |
| `GET` | `/api/letter-distributions/:name` | `{ name, tiles: [{ letter, blank_letter, count, value, is_vowel }] }` in tile order (blank first), for parsing, display and the tile palette |
| `POST` | `/api/search/preview` | Body `{ lexicon, quiz_type, conditions[] }` → `{ count, sample[], over_cap }`, or `400` with `{ errors: [{ row, field, message }] }` |

A condition on the wire:

```json
{ "type": "probability_order", "negated": false,
  "min": 1, "max": 1000, "blanks": 2, "lax": true }
```

```json
{ "type": "leave_value", "negated": false, "min": 10.0, "max": null }
```

`type` is one of `anagram_match`, `pattern_match`, `subanagram_match`, `length`,
`in_lexicon`, `in_word_list`, `num_vowels`, `includes_letters`,
`probability_order`, `limit_by_probability_order`, `playability_order`,
`limit_by_playability_order`, `num_unique_letters`, `point_value`,
`takes_prefix`, `takes_suffix`, `part_of_speech`, `definition`, `consists_of`,
`num_anagrams` or `leave_value`. Only the parameters that type uses are
accepted; any extra field is a `400`.

### Cascades and sync

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/api/cascades` | Body `{ name?, lexicon, quiz_type, clear_threshold?, segment_size?, progression?, require_alphabetical?, conditions[] }`. Runs the search, stores the question index, shuffles the Source quiz, and returns `{ cascade, source_quiz, sync_seq }`. `422` if the search is empty or over the cap. `409` with `{ error: "cascade_limit", limit, count }` at the [cascade limit](#cascade-limit). The threshold and the three [quiz options](#quiz-options) default to the user's preferences, and the Source quiz is created with a copy of the options. |
| `POST` | `/api/cascades/:id/start-over` | New cascade with the same spec, threshold and options; returns the same shape. Subject to the cascade limit. |
| `GET` | `/api/cascades/:id/cards?from=<idx>&limit=<n>&hooks=<0\|1>&definitions=<0\|1>` | Answer cards `[{ idx, key, answer }]` for question indexes `from`…`from+limit−1` (`limit` ≤ 10,000). Immutable, so it is served with `Cache-Control: private, max-age=31536000, immutable` and compressed. |
| `GET` | `/api/cascades/:id/export?scope=<cascade\|quiz>&quiz_id=<uuid>&which=<all\|correct\|missed\|ungraded>&format=<txt\|csv>&lines=<answers\|questions>&columns=<list>&order=<study\|alphabetical>&definitions=<0\|1>&hooks=<0\|1>` | The same file the device builds locally (see [Exporting words](#exporting-words)), streamed as `text/plain` or `text/csv` with a `Content-Disposition` filename. Used when the device doesn't have what the export needs. `404` for another user's or a purged cascade. |
| `POST` | `/api/sync` | Body `{ device_id, cursor, page_token?, ops[] }` → `{ results: [{ op_id, status, reason? }], changes: { cascades[], quizzes[], quiz_questions[], quiz_attempts[], preferences?, tombstones[] }, sync_seq, next_page_token?, resync_required? }` |

On the wire, `quiz_questions` changes are grouped per quiz as parallel arrays
(`question_idx[]`, `position[]`, `grade[]`) to keep large pulls small.

An operation on the wire:

```json
{ "id": "0192f0c4-…", "device_seq": 118, "at": "2026-09-15T14:03:22.418Z",
  "type": "finish",
  "quiz_id": "0192f0a1-…", "attempt": 2,
  "shuffle_seed": "9241873301934422881", "new_quiz_id": "0192f0c4-…" }
```

Shuffle seeds are sent as decimal strings, because JSON numbers lose precision
above 2⁵³.

A segment descent and an options change on the wire:

```json
{ "id": "0192f0c9-…", "device_seq": 119, "at": "2026-09-15T14:41:07.902Z",
  "type": "finish_segment",
  "quiz_id": "0192f0a1-…", "attempt": 2, "segment_end": 100,
  "shuffle_seed": "4519004437287701013", "new_quiz_id": "0192f0c9-…" }
```

```json
{ "id": "0192f0d1-…", "device_seq": 120, "at": "2026-09-15T14:41:30.117Z",
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
| `DELETE` | `/api/admin/letter-distributions/:id` | `409` with what still actively uses it. `?purge_trashed=true` confirms purging the trashed cascades that reference it |
| `POST` | `/api/admin/lexicons` | Multipart `name`, `letter_distribution`, `file` |
| `DELETE` | `/api/admin/lexicons/:id` | `409` with what still actively uses it. `?purge_trashed=true` confirms purging the trashed cascades that reference it |
| `POST` | `/api/admin/leave-sets` | Multipart `lexicon`, `file` |
| `DELETE` | `/api/admin/leave-sets/:id` | `409` with what still actively uses it. `?purge_trashed=true` confirms purging the trashed cascades that reference it |

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
| `/cascades/new` | Cascade builder (type, lexicon, filter rows, threshold, preview) |
| `/cascades/:id` | Player for the cascade's deepest level, with the ladder panel |
| `/trash` | Cleared quizzes and trashed cascades |
| `/account` | Preferences, controls, sync status, offline storage use, password, sign out everywhere, delete account |
| `/admin` | Catalog overview with delete actions (admins only; others get a not-found page) |
| `/admin/letter-distributions/new`, `/admin/lexicons/new`, `/admin/leave-sets/new` | Upload forms with validation error lists |

Modules:

- **`lib/cascade/`**: the cascade rules and the SplitMix64 and Fisher–Yates
  shuffle. Pure TypeScript, tested against the shared test vectors.
- **`lib/local/`**: the IndexedDB schema, migrations and typed accessors. Every
  write the player makes goes through `applyLocally(op)`, which updates the
  stores and appends the operation to the outbox in one IndexedDB transaction.
- **`lib/sync/`**: the sync engine (triggers, push, paged pull, rebase, backoff,
  `401` handling) and the download manager (card pages, 14-day policy, Keep
  offline, eviction).
- **`lib/export/`**: builds a word list or CSV from the local stores, in a worker
  and in chunks, and decides when the export has to come from the server
  instead. The Rust and TypeScript formatters are held to one set of fixtures in
  `contract-fixtures/export/`, so the file a device writes and the file the
  server streams are byte-identical.

Components:

- **`FilterRow`**: one component per condition. It is driven by a single
  frontend table (`lib/filters.ts`) giving each type's label, parameter inputs,
  defaults, bounds, whether Not is allowed, whether the row may be joined with
  **or** (limit rows may not), and which quiz types it applies to. The row also
  renders the and/or join dropdown for every row but the first, and the groups
  it forms are shown by an indent rule down the left of the list. The Leave
  Value filter uses decimal inputs that may be left blank, and the two inner
  hook filters take no parameters at all. The backend
  has its own validation; this table only drives the UI, and a contract test
  keeps the two in step (see [Testing](#testing)).
- **`TilePalette`**: clickable tiles for the current distribution, shown under
  tile inputs when the distribution has any multi-character or non-ASCII tile.
  Tiles are inserted whole and never re-split.
- **`TileText`**: renders MAGPIE notation, drawing multi-character tiles as
  single joined tiles without brackets.
- **`CascadeLadder`**: the levels of a cascade, with sizes, attempts, last
  scores, the current run of any segmented level, which level is current, and
  per-level **Quiz options** and **Export…** actions.
- **`QuizOptionsForm`**: segment size, progression and alphabetical order, with
  their inline explanations. The same component serves the cascade builder, the
  cascade's options dialog and the quiz settings menu; it is told whether it is
  editing a cascade or a quiz and saves through the matching operation.
- **`ExportDialog`**: the scope, selection, format and column choices of
  [Exporting words](#exporting-words), a live count of how many entries the file
  will hold, and the fallback notice when the export needs data the device does
  not have.
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
- **`ControlsEditor`**: the two shortcut-set switches, the binding list per
  action within each set, the capture box, conflict and reserved-stroke notices,
  and Reset to defaults. It saves the switches through `set_preferences` and the
  bindings through a `set_bindings` operation.
- **`TypedAnagramCard`**: answer input, the found counter when **Show answers
  found** is on, found and wrong lists,
  give-up and override controls. It checks entries against the card's answer
  list locally, and, when the quiz requires alphabetical order, against the
  furthest answer entered so far, marking an out-of-order answer and showing the
  hint for what comes next.
- **`AnagramAnswerList`**: shared by both anagram modes. Renders words with
  optional hooks and definitions, and highlights unfound words.
- **`LeaveValue`**: formats a raw value with a sign and the preferred number of
  decimal places.
- **`FinishBanner`**: the non-blocking message after each finish.
- **`SyncStatus`**: Synced, *n* changes waiting to sync, Offline, or Log in to
  sync, plus notices about dropped work.
- **`OfflineBadge`**: Available offline, download progress, or the Keep offline
  toggle.
- **`PreferencesMenu`**: the gear menu in the player. It applies changes locally
  through a `set_preferences` operation and updates the current card straight
  away.
- **`WordListEditor`**: paste or upload a file for In Word List, showing the
  count and how many entries are not valid in the lexicon.
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
| `SESSION_SIGNING_KEY` | — | Required; 32 bytes as hex |
| `BIND_ADDR` | `0.0.0.0:8080` | |
| `SESSION_TTL_SECONDS` | `2592000` (30 days) | |
| `SECURE_COOKIES` | `false` | `true` in any TLS deployment |
| `MAIL_BACKEND` | `console` | `console` or `ses` |
| `MAIL_FROM` | `no-reply@wordfall.local` | |
| `PUBLIC_URL` | `http://localhost:5173` | Base URL for email links |
| `MAX_QUIZ_QUESTIONS` | `300000` | May be lowered; a value above 300,000 fails startup |
| `MAX_CASCADES_PER_USER` | `100` | Counts cascades in the Trash |
| `SEARCH_TIMEOUT_MS` | `2000` | |
| `TRASH_RETENTION_DAYS` | `30` | Y: how long cleared quizzes and trashed cascades stay in the Trash |
| `SYNC_RETENTION_DAYS` | `90` | How long operation records and tombstones are kept |
| `SYNC_MAX_OPS` | `500` | Operations accepted per sync request |
| `PURGE_INTERVAL_SECONDS` | `3600` | |
| `ADMIN_UPLOAD_MAX_BYTES` | `104857600` (100 MB) | |
| `CATALOG_RECONCILE_SECONDS` | `60` | Fallback reload interval if a notification is missed |
| `TRUSTED_PROXY_HOPS` | `0` | `1` behind the ALB or compose Nginx |

Client-side limits (14-day download window, 500 MB card storage soft limit,
30-second sync interval) are constants in `lib/sync/config.ts`.

---

## Local Development

```bash
./scripts/dev.py \
  --distribution English=~/wordgame/english.csv \
  --lexicon CSW24:English=~/wordgame/CSW24.tsv \
  --leaves CSW24=~/wordgame/CSW24_leaves.csv
```

This command:

1. Brings up `docker compose` (Postgres, backend, Nginx on :5173).
2. Waits for `/health`.
3. Creates a confirmed user `dev` / `dev-password` and sets `is_admin` with SQL.
4. Uploads each given file through the admin API, skipping items that already
   exist.
5. Opens the browser.

Uploading through the real API means seeding also tests the upload paths.

`--hot-reload` adds the Vite dev server on :5174. With `MAIL_BACKEND=console`,
emailed links appear in `docker compose logs backend`.

To try offline studying locally, use the production-style build on :5173 (the
Vite dev server does not register the service worker). Then use the browser
DevTools **Offline** toggle, or `docker compose stop backend`, which is
indistinguishable to the app.

After editing `0001_initial.sql`, reset the local database
(`DROP SCHEMA public CASCADE; CREATE SCHEMA public;`) and restart the backend,
because SQLx refuses to run a migration whose checksum has changed. Also clear
the site data in the browser, because local cursors no longer match.

---

## Deployment and Operations

- **Terraform** in `infra/` covers:
  - VPC with public subnets (ALB) and private subnets (ECS, RDS)
  - ALB with an ACM certificate, HTTP → HTTPS redirect, and a 120-second idle
    timeout for admin uploads
  - ECS cluster, service and task definition (backend and Nginx containers)
  - RDS Postgres, with 30-day point-in-time recovery
  - SES domain identity
  - SSM parameters (names only; values are set out of band)
  - CloudWatch log group
- **Deploys** build and push both images, then update the ECS service. The
  backend runs migrations before it binds, and the ALB health check waits for
  `/health`, which includes catalog indexes being loaded, before sending
  traffic.
  - **Caching:** Nginx serves `service-worker.js` and `index.html` with
    `Cache-Control: no-cache`, so devices pick up new app versions, and hashed
    assets as immutable.
  - **Sync compatibility:** a sync API change must stay compatible with the
    previous app version for at least `SYNC_RETENTION_DAYS`, because devices can
    be offline with an old app that long. The sync request carries the app
    version, and a request from a version too old to sync gets `426` with a
    reload prompt, after its operations have been accepted.
- **Granting admin** in production is a one-off SQL statement run through a
  bastion or an ECS exec session. It is documented in the runbook.
- **Catalog uploads** happen in the browser at `/admin`. Every running instance
  picks up changes through `LISTEN/NOTIFY`, with no restart or redeploy.
- **Backups:**
  - RDS automated backups with point-in-time recovery.
  - A nightly `pg_dump` to an encrypted, versioned S3 bucket, run as a
    scheduled Fargate task, with an alarm if no successful dump happens in 36
    hours.
  - A documented restore drill. A restore to an earlier point makes server
    sequences go backwards, so after one, every user's `sync_floor_seq` is set
    to their `sync_seq`, forcing devices to resync.
- **Capacity**: RDS storage autoscaling is on, with an alarm on free storage,
  because large cascades (about 45 MB at 300,000 questions, plus about 30 MB for
  each large level) are the main driver of database growth. Task memory is sized
  to the catalog, and each index's size is visible in `/admin`.
- **Monitoring**: sync rejections are counted by type and reason in logs and
  graphed. A rise in rejections outside the expected stale-attempt cases points
  to a divergence between the Rust and TypeScript rules.

---

## Testing

- **Search engine unit tests** (`cargo test --lib`) run against a small
  hand-built fixture catalog committed to the repo. It contains made-up and
  public-domain words only, no licensed data:
  - an English-style distribution
  - a Catalan-style distribution with multi-character tiles (`NY`, `QU`, `L·L`)
    and `Ç`
  - a lexicon and a leave value set for each

  The tests cover:
  - Every example in Zyzzyva's search help, recreated with a fixture that
    contains the example words: `ETX?`, `PI??Z`, `Z[AEIOU][AEIOU]`, `*JBX`,
    `AT??`, `?W*M?S`, `LX[AU]`, the Includes-Letters Q-not-U case,
    `Consists of AEIOU 70–100`, and the lax tie cases.
  - Leave Value: inclusive bounds, open bounds, negative values, and a value
    exactly equal to a bound.
  - Inner hooks: a word whose first tile can be dropped, one whose last tile
    can, one where both can, a one-tile word (never a match), and both negated.
    A Catalan fixture checks that a multi-character first tile is dropped as one
    tile.
  - `.` and `?`: every pattern example rerun with `.` substituted gives
    identical results, and a pattern normalizes to `?` on the way into storage.
  - AND / OR: a two-group spec returns the union of the two groups' results;
    `A and B or C` groups as `(A and B) or C`, not `A and (B or C)`; a duplicate
    matched by both groups appears once; a limit row applies to the union; and a
    single-group spec matches the old AND-only behaviour on every fixture
    search.
  - Tile handling:
    - `A[NY]S` and typed `ANYS` both parse to three tiles.
    - Palette-inserted `N` + `Y` stay two tiles.
    - Malformed notation (`[`, `[]`, `[A]`, nested brackets) is rejected, as in
      MAGPIE's `ld_str_to_mls`.
    - Sort order follows the distribution file, with the blank first.
    - Vowel and point counts use the distribution.
- **Property tests** (`proptest`):
  - The shortcut candidate paths return the same results as a full scan,
    including when only some groups carry a Length or exact Anagram Match row.
  - Anagram Match without wildcards returns exactly the alphagram map entry.
  - Negating a predicate partitions the candidates.
  - Limit ranges are subsets of the unlimited results.
  - Over random sequences of grades, finishes and restores, the cascade stack
    stays valid: the active levels are exactly `1..depth`, `depth >= 1`, the
    Source quiz is always active at Level 1, only the deepest level is played,
    and every level's questions come from the Source quiz.
- **Probability tests** check `combinations` against brute-force enumeration of
  a small bag for 0, 1 and 2 blanks (and for a bag with no blanks), and check
  that ranks, minimum ranks and maximum ranks are consistent on ties.
- **Shared rule and shuffle vectors** (`contract-fixtures/cascade/`): sequences
  of operations with the expected cascade state after each one, and seeds with
  their expected permutations. Both `cargo test` and the frontend unit tests
  (Vitest) must pass every vector. Together they cover:
  - all four finish outcomes, including exactly-at-threshold scores and
    thresholds of 0 and 100
  - climbing back up
  - restoring into live, cleared and trashed cascades
  - purges
  - **Drill progression**: a quiz replaced below the threshold, one cleared at or
    above it, the nothing-correct reset, and a cascade cleared from one level
  - **Segments**: run boundaries for sizes that do and don't divide the question
    count; a run with no misses creating nothing; a run with misses creating a
    Drill quiz one level down and leaving the cursor at the boundary; drilling
    that quiz down to nothing and coming back to the right run; a segment
    descent inside a Ladder cascade and inside a Drill one; a segment size at or
    above the question count behaving like 0; the segment size changing
    mid-attempt, including to a value that puts the next boundary before a
    boundary already descended
  - **Options**: every new quiz taking the cascade's options, a change to a quiz
    not touching the cascade or its siblings, a change to the cascade not
    touching existing quizzes, and a segment quiz being Drill in a Ladder cascade
- **Export tests**: the formatter fixtures in `contract-fixtures/export/` cover
  all three quiz types, the four selections, both formats, both orderings,
  multi-character tiles written plainly with no brackets, CSV quoting of
  definitions with
  commas and quotes, and the cascade-wide definition of correct and missed across
  several active quizzes. A Rust test and a Vitest test run the same fixtures, and
  an integration test checks that `GET /api/cascades/:id/export` returns the same
  bytes for the same request.
- **Zyzzyva parity** (local only, needs licensed data): a script runs a
  checked-in list of searches against a real CSW24 upload and compares
  the word lists with exports from Zyzzyva for the same searches. Differences
  are either fixed or recorded here as intended deviations.
- **Upload validation tests**: one failing file per rule under
  [File formats](#file-formats). Each asserts the line numbers in the error list
  and that nothing was written. A file with many errors reports the first 1,000
  and the total. Every letter distribution file in MAGPIE-DATA, fetched at a
  pinned commit, uploads unchanged and produces the expected tiles.
- **Schema tests**:
  - Every one of the 23 condition types round-trips through
    `search_conditions` and back into an equal `ConditionKind`.
  - For each type, inserting a row with a missing or extra parameter, or with
    Not where it isn't allowed, is rejected by the `CHECK`.
  - A Leave Value cascade whose leave value set belongs to another lexicon is
    rejected by the composite foreign key.
  - A second active quiz at the same level, a second Source quiz, a `depth` of 0
    and a cleared Source quiz are each rejected.
- **Sync integration tests** (`cargo test`, `TEST_DATABASE_URL`):
  - A repeated operation is applied once and gets the same result.
  - Operations are applied in `device_seq` order, and one rejection doesn't stop
    the batch.
  - Two simulated devices cover every row of the [Conflicts](#conflicts) table.
  - Pulls return exactly the rows changed since the cursor, across page
    boundaries.
  - Purges produce tombstones, and a cursor older than `sync_floor_seq` gets
    `resync_required`.
  - A device-created quiz id that already exists is rejected.
  - A repeated `finish_segment` for the same quiz, attempt and boundary is
    applied once, and two devices racing on one boundary produce one drill quiz.
  - `finish_segment` is rejected for a quiz with no segment size, for a boundary
    that is not a multiple of it, for one at or past the question count, and for
    one with ungraded questions before it.
  - Options operations keep each field's latest change across two devices.
  - A 300,000-question `finish` and reset complete within budget.
- **Other integration tests** (`cargo test`, `TEST_DATABASE_URL`) drive the real
  router in-process, covering:
  - the auth flows
  - admin authorization: non-admins get `404` on every admin route, and
    revoking `is_admin` takes effect on the next request
  - uploads followed by catalog reload across two in-process app instances
    sharing one database (`NOTIFY`), and the reconcile fallback
  - deletion refused for each kind of active reference, allowed once
    unreferenced, and allowed with `purge_trashed` when only trashed cascades
    and cleared quizzes are left, which are purged with tombstones
  - cascade creation for all three types, Start over, and card pages
  - the cascade limit, including trashed cascades counting toward it and two
    simultaneous creations competing for the last slot
  - creating a cascade with each combination of quiz options, and the Source
    quiz carrying the copy
  - every export endpoint selection, for all three quiz types, including an
    export of a quiz in the Trash and a `404` for another user's cascade
  - the purge task, including two instances running it at the same time
  - one user being unable to reach another's cascades through REST or sync
  - the application-level invariants listed under [Schema](#schema)
- **Scale tests**:
  - Create a 300,000-question cascade, download its cards, grade every question
    (half missed) through sync, finish, and assert the timings for creation,
    download, push, pull and finish stay within budget. A 300,001-question search
    is refused with its count.
  - Upload a million-row leave file within the upload timeout.
- **Contract test**: the frontend filter table and the backend condition schema
  are both generated or checked against one shared JSON fixture in
  `contract-fixtures/`, so a new filter parameter cannot be added on one side
  only.
- **Frontend**: `npm run check`, Vitest for `lib/cascade`, `lib/local` and
  `lib/sync` (with `fake-indexeddb`), and Playwright journeys:
  - Register, confirm, log in, create an anagram cascade with an 80% threshold,
    and see every cascade rule applied as expected:
    - Finish Level 1 below the threshold and go down to Level 2.
    - Clear Level 2 with misses and get a replacement at Level 2.
    - Clear it with no misses and climb back to Level 1's reshuffled quiz.
    - Finish Level 1 at or above the threshold with misses and see the Source
      quiz reset, not trashed, with a new Level 2 of its misses.
    - Finish Level 1 with no misses and see the completion screen, the cascade
      still present with its Source quiz reset and playable, and nothing about
      the Source quiz in the Trash.
  - **Offline study:**
    1. Create a cascade and wait for Available offline.
    2. Go offline (`context.setOffline(true)`) and reload the page.
    3. Study through several finishes, including a descent, a clear that climbs
       back up, and a Source quiz reset.
    4. Restore a quiz from the Trash.
    5. Confirm Create Cascade is disabled and says why.
    6. Go back online and see Synced.
    7. In a fresh browser context, log in and see identical cascade state, grades
       and question order.
  - **Two devices:** finish the same level offline in two browser contexts,
    reconnect both, and see the second device's notice and matching final state.
  - **Session expiry while offline:** study, expire the session, reconnect, see
    Log in to sync, log in, and see the work synced.
  - Switch to typed mode: a wrong entry grades missed, finding every anagram
    grades correct, and Enter on an empty input reveals the answer.
  - With **alphabetical order** on, entering answers in order grades correct, an
    out-of-order answer is marked out of order, joins the found list and grades
    the card missed, and a later in-order answer after it is accepted normally.
    With the option off, the same sequence grades correct.
  - **Segments and progression:** create a cascade with a segment of 5 and finish
    the first run with misses, see the drill level, clear it down to nothing,
    come back to run 2 at the right question, and finish the quiz. Switch a
    cascade to Drill, finish below the threshold, and see the quiz replaced at
    the same level instead of a new level appearing. Change a quiz's segment size
    from the settings menu mid-attempt and see the next boundary move.
  - **Export:** download the missed words of a level as a word list and the whole
    cascade as a CSV, offline, and check the contents; then ask for definitions
    that were never downloaded, while offline, and see the fallback notice.
  - Turn on hooks and definitions, and see them in anagram answers.
  - Turn on **Show answers found** and see `0 of 9 found` above the typed input;
    with it off (the default), neither the count nor the total appears.
  - Build a cascade with two filter groups joined by **or** and see the preview
    count match the union, and a `.` in a pattern behave exactly like `?`.
  - **Desktop controls:**
    - Mouse 1 shows and advances, Mouse 2 toggles, and Mouse 3 goes back, with no
      context menu appearing.
    - `,` shows and advances, `M` toggles, and `J` goes back.
    - Clicks on the side rails do nothing to the quiz.
    - Turn **mouse shortcuts** off: clicks in the quiz area do nothing, the keys
      still work, and turning the setting back on restores the same bindings.
    - Turn **keyboard shortcuts** off: `,`, `M` and `J` do nothing and the mouse
      still works.
    - Rebind Toggle grade to the wheel and to `Shift+T`, and see both work and
      the change sync to a second browser context.
    - A binding can't be removed from an action that has only one in its set.
    - In typed mode, bound letter keys type into the input instead of acting.
  - **Touch zones** (mobile viewport emulation, portrait and landscape): each
    zone fires its action, a drag in the Show / Next zone scrolls a long answer
    without advancing, and a quick double tap advances only once.
  - Set leave decimals to 3 and see a leave answer rendered to three places.
  - As an admin, upload a distribution, lexicon and leave value set, see a
    deliberately broken file rejected with line numbers, build a Leave Value
    cascade from the new set, see deletion refused while the cascade is active,
    then trash the cascade and see deletion go through after confirming the
    purge.

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
4. **Search engine**: all 23 filters, including both limits, validation errors,
   unit, property and parity tests, `/api/search/preview`.
5. **Cascade builder**: filter rows, applicability rules, tile palette, word list
   editor, live preview, clear threshold, quiz options,
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
