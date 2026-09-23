"""An independent reference model of the cascade rules, shuffle, question hash
and leave value text, written from PLAN.md alone. It generates the contract
fixtures that the Rust and TypeScript modules are checked against; neither of
those is ever used to generate them (PLAN.md § Contract fixtures).

Cited algorithms:
- SplitMix64: Vigna's reference splitmix64.c (PLAN.md § Deterministic
  shuffles): state += 0x9E3779B97F4A7C15; z = state;
  z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9; z = (z ^ (z >> 27)) *
  0x94D049BB133111EB; return z ^ (z >> 31), all mod 2^64.
- Fisher–Yates from the last position down to 1, j = next() mod (i + 1), over
  the indexes sorted ascending.
- FNV-1a 64 (offset basis 14695981039346656037, prime 1099511628211) over the
  ascending indexes, each as a little-endian u32.
- Leave value text (PLAN.md § Answers): n = trunc(|v| × 10^d + 0.5) in f64;
  n as an integer with '.' d digits from the right, zero-padded; '-' when
  v < 0 and n > 0; '+' on screen when v > 0 and n > 0; never '+' in exports.
- The cascade rules: PLAN.md § Cascade Rules, § Rule implementation,
  § Operations (the conditions and rejection reasons).
"""

import math

MASK = (1 << 64) - 1
GAMMA = 0x9E3779B97F4A7C15
RESET_XOR = 0x9E3779B97F4A7C15


def splitmix64(seed):
    state = seed & MASK
    while True:
        state = (state + GAMMA) & MASK
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        yield z ^ (z >> 31)


def shuffle(idx, seed):
    """The indexes in position order."""
    v = sorted(idx)
    rng = splitmix64(seed)
    for i in range(len(v) - 1, 0, -1):
        j = next(rng) % (i + 1)
        v[i], v[j] = v[j], v[i]
    return v


def questions_hash(idx):
    h = 14695981039346656037
    for i in sorted(idx):
        for b in int(i).to_bytes(4, "little"):
            h ^= b
            h = (h * 1099511628211) & MASK
    return h


def leave_text(v, d, screen):
    n = math.trunc(abs(v) * (10.0 ** d) + 0.5)
    digits = str(n)
    if d > 0:
        digits = digits.rjust(d + 1, "0")
        digits = digits[:-d] + "." + digits[-d:]
    if n > 0 and v < 0:
        return "-" + digits
    if n > 0 and v > 0 and screen:
        return "+" + digits
    return digits


# ---------------------------------------------------------------------------
# The cascade model
# ---------------------------------------------------------------------------


class Rejected(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


class Quiz:
    def __init__(self, qid, level, origin, idx, seed, opts, chain=False, origin_quiz=None,
                 origin_attempt=None, origin_segment_end=None):
        self.id = qid
        self.level = level
        self.origin = origin
        self.origin_quiz_id = origin_quiz
        self.origin_attempt = origin_attempt
        self.origin_segment_end = origin_segment_end
        self.status = "active"
        self.segment_chain = chain
        self.segment_size, self.progression, self.require_alphabetical = opts
        if chain:
            # A segment chain is Drill for life and never segmented.
            self.segment_size, self.progression = 0, "drill"
        self.attempt = 1
        self.seed = seed
        self.idx = sorted(idx)
        self.grades = {}
        self.cursor = 0
        self.run_start = 0

    @property
    def correct(self):
        return sum(1 for g in self.grades.values() if g == "correct")

    @property
    def missed(self):
        return sum(1 for g in self.grades.values() if g == "missed")

    def positions(self):
        return shuffle(self.idx, self.seed)

    def reset(self, seed):
        """A new attempt: new shuffle, grades, counters, cursor and run_start cleared."""
        self.attempt += 1
        self.seed = seed
        self.grades = {}
        self.cursor = 0
        self.run_start = 0

    def effective_size(self):
        """Segment size as the rules read it: 0 for a chain or at/above the count."""
        s = self.segment_size
        if self.segment_chain or s == 0 or s >= len(self.idx):
            return 0
        return s

    def next_boundary(self):
        s = self.effective_size()
        if s == 0:
            return None
        b = (self.cursor // s + 1) * s
        return b if b < len(self.idx) else None

    def run_indicator(self):
        """`run a of b · c of d` from the current size, or None unsegmented."""
        s = self.effective_size()
        if s == 0:
            return None
        q = len(self.idx)
        run = self.run_start // s + 1
        total = (q - 1) // s + 1
        nb = self.next_boundary()
        length = (nb if nb is not None else q) - self.run_start
        return f"run {run} of {total} · {self.cursor - self.run_start + 1} of {length}"


class Cascade:
    def __init__(self, threshold, opts, count, source_id, source_seed):
        self.threshold = threshold
        self.segment_size, self.progression, self.require_alphabetical = opts
        self.depth = 1
        self.peak_depth = 1
        self.attempts_since_completion = 0
        self.completions = 0
        self.trashed = False
        self.purged = False
        self.quizzes = {}
        self.purged_quizzes = set()
        self.quizzes[source_id] = Quiz(source_id, 1, "source", range(count), source_seed, self.opts())

    def opts(self):
        return (self.segment_size, self.progression, self.require_alphabetical)

    def active_at(self, level):
        for q in self.quizzes.values():
            if q.status == "active" and q.level == level:
                return q
        return None

    # -- lookups shared by every operation -------------------------------

    def quiz(self, qid):
        if self.purged or qid not in self.quizzes:
            raise Rejected("not_found")
        return self.quizzes[qid]

    def live_quiz(self, qid):
        q = self.quiz(qid)
        if self.trashed:
            raise Rejected("trashed")
        if q.status != "active":
            raise Rejected("not_active")
        return q

    def check_attempt(self, q, op):
        if op["attempt"] != q.attempt or int(op["attempt_seed"]) != q.seed:
            raise Rejected("stale_attempt")

    def new_id_free(self, qid):
        if qid in self.quizzes or qid in self.purged_quizzes:
            raise Rejected("invalid")

    def new_quiz(self, qid, level, origin, idx, seed, chain, **origin_fields):
        opts = (0, "drill", self.require_alphabetical) if chain else self.opts()
        q = Quiz(qid, level, origin, idx, seed, opts, chain=chain, **origin_fields)
        self.quizzes[qid] = q
        return q

    def push_depth(self):
        self.depth += 1
        self.peak_depth = max(self.peak_depth, self.depth)

    # -- operations --------------------------------------------------------

    def apply(self, op):
        t = op["type"]
        return getattr(self, "op_" + t)(op)

    def op_grade(self, op):
        q = self.live_quiz(op["quiz"])
        self.check_attempt(q, op)
        if op["question_idx"] not in q.idx:
            raise Rejected("not_found")
        q.grades[op["question_idx"]] = op["grade"]
        return {}

    def op_move_cursor(self, op):
        q = self.live_quiz(op["quiz"])
        self.check_attempt(q, op)
        p = op["position"]
        nb = q.next_boundary()
        limit = nb if nb is not None else len(q.idx)
        if p < q.run_start or p >= limit:
            raise Rejected("bad_cursor")
        q.cursor = p
        return {}

    def check_deepest(self, q):
        if q.level != self.depth:
            raise Rejected("not_deepest")

    def op_finish(self, op):
        q = self.live_quiz(op["quiz"])
        # PQ-013: the attempt before the depth, so a finish racing one that
        # descended is stale_attempt, as the Conflicts table says.
        self.check_attempt(q, op)
        self.check_deepest(q)
        if len(q.grades) != len(q.idx):
            raise Rejected("ungraded")
        new_id = op["new_quiz_id"]
        self.new_id_free(new_id)
        seed = int(op["shuffle_seed"])
        count = len(q.idx)
        cor, mis = q.correct, q.missed
        passed = cor * 100 >= self.threshold * count
        misses = sorted(i for i, g in q.grades.items() if g == "missed")
        attempt_seed = q.seed
        # Every finish counts toward attempts_since_completion.
        self.attempts_since_completion += 1
        result = {}
        created = None
        if q.level == 1:
            if mis == 0:
                outcome = "completed"
                result["completion"] = {"levels": self.peak_depth, "attempts": self.attempts_since_completion}
                q.reset(seed ^ RESET_XOR)
                self.completions += 1
                self.peak_depth = 1
                self.attempts_since_completion = 0
            elif cor == 0:
                outcome = "reshuffled"
                q.reset(seed ^ RESET_XOR)
            else:
                outcome = "descended"
                q.reset(seed ^ RESET_XOR)
                created = self.new_quiz(new_id, 2, "descent", misses, seed, False,
                                        origin_quiz=q.id, origin_attempt=q.attempt - 1)
                self.push_depth()
        else:
            level = q.level
            if cor == 0:
                outcome = "reshuffled"
                q.reset(seed ^ RESET_XOR)
            elif q.progression == "ladder" and not passed:
                outcome = "descended"
                q.reset(seed ^ RESET_XOR)
                created = self.new_quiz(new_id, level + 1, "descent", misses, seed, False,
                                        origin_quiz=q.id, origin_attempt=q.attempt - 1)
                self.push_depth()
            else:
                # Cleared (score at or above the threshold) or, under Drill,
                # replaced: the quiz goes to the Trash.
                outcome = "cleared" if passed else "replaced"
                q.status = "cleared"
                if misses:
                    origin = "clear_replacement" if passed else "drill_replacement"
                    created = self.new_quiz(new_id, level, origin, misses, seed, q.segment_chain,
                                            origin_quiz=q.id, origin_attempt=q.attempt)
                else:
                    self.depth -= 1
        result["outcome"] = outcome
        result["attempt"] = {"quiz": q.id, "attempt": q.attempt if q.status == "cleared" else q.attempt - 1,
                             "correct": cor, "missed": mis, "outcome": outcome, "shuffle_seed": str(attempt_seed)}
        target = created if created is not None else (q if q.status == "active" else None)
        if target is not None:
            result["new_quiz_question_count"] = len(target.idx)
            result["new_quiz_questions_hash"] = str(questions_hash(target.idx))
        return result

    def op_finish_segment(self, op):
        q = self.live_quiz(op["quiz"])
        # PQ-013: the attempt, then the duplicate, before the depth, so a run
        # already drilled on another device is duplicate_segment.
        self.check_attempt(q, op)
        end = op["segment_end"]
        s = q.segment_size
        count = len(q.idx)
        if q.segment_chain or s == 0 or end % s != 0 or not (0 < end < count):
            raise Rejected("bad_segment")
        for other in self.quizzes.values():
            if (other.origin == "segment" and other.origin_quiz_id == q.id
                    and other.origin_attempt == q.attempt and other.origin_segment_end == end):
                raise Rejected("duplicate_segment")
        self.check_deepest(q)
        if end <= q.cursor:
            raise Rejected("bad_segment")
        pos = q.positions()
        if any(pos[p] not in q.grades for p in range(end)):
            raise Rejected("ungraded")
        new_id = op["new_quiz_id"]
        self.new_id_free(new_id)
        misses = sorted(pos[p] for p in range(q.run_start, end) if q.grades[pos[p]] == "missed")
        result = {}
        if misses:
            created = self.new_quiz(new_id, q.level + 1, "segment", misses, int(op["shuffle_seed"]), True,
                                    origin_quiz=q.id, origin_attempt=q.attempt, origin_segment_end=end)
            self.push_depth()
            result["outcome"] = "drilled"
            result["new_quiz_question_count"] = len(created.idx)
            result["new_quiz_questions_hash"] = str(questions_hash(created.idx))
        else:
            result["outcome"] = "continued"
        q.cursor = end
        q.run_start = end
        return result

    def op_restore_quiz(self, op):
        q = self.quiz(op["quiz"])
        if q.status != "cleared":
            raise Rejected("not_cleared")
        if self.trashed:
            self.trashed = False
        q.status = "active"
        self.push_depth()
        q.level = self.depth
        q.reset(int(op["shuffle_seed"]))
        # Options copied afresh from the cascade; a chain takes only alphabetical order.
        if q.segment_chain:
            q.require_alphabetical = self.require_alphabetical
        else:
            q.segment_size, q.progression, q.require_alphabetical = self.opts()
        return {"new_quiz_question_count": len(q.idx), "new_quiz_questions_hash": str(questions_hash(q.idx))}

    def op_trash_cascade(self, op):
        if self.purged:
            raise Rejected("not_found")
        if self.trashed:
            raise Rejected("trashed")
        self.trashed = True
        return {}

    def op_restore_cascade(self, op):
        if self.purged:
            raise Rejected("not_found")
        if not self.trashed:
            raise Rejected("not_trashed")
        self.trashed = False
        return {}

    def op_purge_quiz(self, op):
        q = self.quiz(op["quiz"])
        if self.trashed:
            raise Rejected("trashed")
        if q.status != "cleared":
            raise Rejected("not_cleared")
        del self.quizzes[q.id]
        self.purged_quizzes.add(q.id)
        return {}

    def op_purge_cascade(self, op):
        if self.purged:
            raise Rejected("not_found")
        if not self.trashed:
            raise Rejected("not_trashed")
        self.purged = True
        for qid in list(self.quizzes):
            self.purged_quizzes.add(qid)
        self.quizzes = {}
        return {}

    @staticmethod
    def check_option_values(op, cap=300000):
        if "segment_size" in op:
            s = op["segment_size"]
            if not isinstance(s, int) or not (s == 0 or 5 <= s <= cap):
                raise Rejected("invalid")
        if "progression" in op and op["progression"] not in ("ladder", "drill"):
            raise Rejected("invalid")
        if "require_alphabetical" in op and not isinstance(op["require_alphabetical"], bool):
            raise Rejected("invalid")

    def op_set_cascade_options(self, op):
        if self.purged:
            raise Rejected("not_found")
        if self.trashed:
            raise Rejected("trashed")
        self.check_option_values(op)
        for k in ("segment_size", "progression", "require_alphabetical"):
            if k in op:
                setattr(self, k, op[k])
        return {}

    def op_set_quiz_options(self, op):
        q = self.live_quiz(op["quiz"])
        self.check_option_values(op)
        if q.segment_chain and ("progression" in op or "segment_size" in op):
            raise Rejected("invalid")
        if q.origin == "source" and "progression" in op:
            raise Rejected("invalid")
        for k in ("segment_size", "progression", "require_alphabetical"):
            if k in op:
                setattr(q, k, op[k])
        return {}

    # -- state for the fixtures -----------------------------------------------

    def snapshot(self):
        quizzes = []
        for q in sorted(self.quizzes.values(), key=lambda q: q.id):
            quizzes.append({
                "id": q.id,
                "level": q.level,
                "status": q.status,
                "origin": q.origin,
                "origin_quiz_id": q.origin_quiz_id,
                "origin_attempt": q.origin_attempt,
                "origin_segment_end": q.origin_segment_end,
                "segment_chain": q.segment_chain,
                "segment_size": q.segment_size,
                "progression": q.progression,
                "require_alphabetical": q.require_alphabetical,
                "attempt": q.attempt,
                "shuffle_seed": str(q.seed),
                "question_count": len(q.idx),
                "questions_hash": str(questions_hash(q.idx)),
                "questions": q.idx,
                "correct": q.correct,
                "missed": q.missed,
                "cursor": q.cursor,
                "run_start": q.run_start,
                "next_boundary": q.next_boundary() if q.status == "active" else None,
                "run_indicator": q.run_indicator() if q.status == "active" else None,
            })
        return {
            "cascade": {
                "depth": self.depth,
                "peak_depth": self.peak_depth,
                "attempts_since_completion": self.attempts_since_completion,
                "completions": self.completions,
                "trashed": self.trashed,
                "purged": self.purged,
                "segment_size": self.segment_size,
                "progression": self.progression,
                "require_alphabetical": self.require_alphabetical,
            },
            "quizzes": quizzes,
        }
