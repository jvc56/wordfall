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
