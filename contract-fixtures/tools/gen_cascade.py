#!/usr/bin/env python3
"""Generates contract-fixtures/cascade/*.json from the independent reference
model in reference.py (PLAN.md § Contract fixtures → Shared rule and shuffle
vectors). Run from the repository root:

    python3 contract-fixtures/tools/gen_cascade.py
"""

import json
from pathlib import Path

import reference as ref

OUT = Path(__file__).resolve().parents[1] / "cascade"
BIG = 1 << 63


def uid(n):
    return f"00000000-0000-4000-8000-{n:012d}"


def seed_of(n):
    """Seeds above 2^53, half of them above 2^63, deterministically."""
    s = next(ref.splitmix64(0xC0FFEE + n))
    return s | (1 << 60)


class Vector:
    def __init__(self, name, covers, count=10, threshold=80, opts=(0, "ladder", False), source_seed=None):
        self.name = name
        self.covers = covers
        self.ids = 1
        self.source = self.next_id()
        seed = source_seed if source_seed is not None else seed_of(self.ids)
        self.setup = {
            "question_count": count,
            "clear_threshold": threshold,
            "segment_size": opts[0],
            "progression": opts[1],
            "require_alphabetical": opts[2],
            "source_quiz_id": self.source,
            "source_seed": str(seed),
        }
        self.c = ref.Cascade(threshold, opts, count, self.source, seed)
        self.steps = []
        self.seeds = 100

    def next_id(self):
        self.ids += 1
        return uid(self.ids)

    def next_seed(self):
        self.seeds += 1
        return str(seed_of(self.seeds))

    def op(self, op, checkpoint=True, expect=None):
        try:
            r = self.c.apply(op)
            result = {"status": "applied", **{k: v for k, v in r.items()}}
        except ref.Rejected as e:
            result = {"status": "rejected", "reason": e.reason}
        if expect is not None:
            assert result["status"] == expect or result.get("reason") == expect, (self.name, op, result)
        step = {"op": op, "result": result}
        if checkpoint:
            step["state"] = self.c.snapshot()
        self.steps.append(step)
        return result

    def quiz(self, qid):
        return self.c.quizzes[qid]

    def play(self, qid, grades, checkpoint=True):
        """Grades cards in position order from the cursor, moving the cursor
        after each one except the last card of a run or of the attempt."""
        q = self.quiz(qid)
        pos = q.positions()
        for i, g in enumerate(grades):
            p = q.cursor
            self.op({"type": "grade", "quiz": qid, "attempt": q.attempt, "attempt_seed": str(q.seed),
                     "question_idx": pos[p], "grade": "correct" if g == "C" else "missed"},
                    checkpoint=False, expect="applied")
            nb = q.next_boundary()
            last_of_run = nb is not None and p + 1 == nb
            last = p + 1 == len(q.idx)
            if last_of_run or last:
                assert i == len(grades) - 1, (self.name, "grades run past a boundary")
                break
            self.op({"type": "move_cursor", "quiz": qid, "attempt": q.attempt, "attempt_seed": str(q.seed),
                     "position": p + 1}, checkpoint=False, expect="applied")
        if checkpoint and self.steps:
            self.steps[-1]["state"] = self.c.snapshot()

    def finish(self, qid, expect="applied"):
        q = self.quiz(qid)
        new = self.next_id()
        r = self.op({"type": "finish", "quiz": qid, "attempt": q.attempt, "attempt_seed": str(q.seed),
                     "shuffle_seed": self.next_seed(), "new_quiz_id": new}, expect=expect)
        return new, r

    def finish_segment(self, qid, end, expect="applied"):
        q = self.quiz(qid)
        new = self.next_id()
        r = self.op({"type": "finish_segment", "quiz": qid, "attempt": q.attempt, "attempt_seed": str(q.seed),
                     "segment_end": end, "shuffle_seed": self.next_seed(), "new_quiz_id": new}, expect=expect)
        return new, r

    def deepest(self):
        return self.c.active_at(self.c.depth).id

    def to_json(self):
        return {"name": self.name, "covers": self.covers, "setup": self.setup, "steps": self.steps}


RULES = "PLAN.md § Cascade Rules"
SRC = "PLAN.md § The Source quiz"
PROG = "PLAN.md § Progression: Ladder or Drill"
SEG = "PLAN.md § Segments"
OPTS = "PLAN.md § Quiz options"
TRASH = "PLAN.md § Trash, restore and purge"
OPS = "PLAN.md § Operations"


def scenarios():
    out = []

    # -- The five finish outcomes, thresholds, climbing back up -------------
    v = Vector("ladder outcomes at 80%", [RULES, "exactly-at-threshold scores", "climbing back up"])
    v.play(v.source, "CCCCCMMMMM")  # 50%: the Source quiz descends at any score
    l2, _ = v.finish(v.source)
    v.play(l2, "CCCCM")  # exactly 80%: cleared with a replacement at Level 2
    l2b, _ = v.finish(l2)
    v.play(l2b, "M")  # nothing correct: reshuffled in place
    v.finish(l2b)
    v.play(l2b, "C")  # no misses: cleared, Level 2 removed, back up to Level 1
    v.finish(l2b)
    out.append(v)

    v = Vector("ladder descent and climb", [RULES, "climbing back up"])
    v.play(v.source, "CCMMMMMMMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CCCMMMMM")  # 37.5% < 80%, some correct: reset and descend
    l3, _ = v.finish(l2)
    v.play(l3, "CCCCC")  # cleared with no misses: Level 3 removed
    v.finish(l3)
    v.play(l2, "CCCCCCCC")  # the reset Level 2 clears too
    v.finish(l2)
    out.append(v)

    v = Vector("threshold 100", [RULES, "a threshold of 100"], threshold=100)
    v.play(v.source, "CCCCCMMMMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CCCCM")  # 80% < 100%: descends
    v.finish(l2)
    out.append(v)

    v = Vector("threshold 1", [RULES, "a threshold of 1"], threshold=1)
    v.play(v.source, "CMMMMMMMMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CMMMMMMMM")  # 1 of 9 ≥ 1%: cleared with a replacement
    v.finish(l2)
    out.append(v)

    # -- The Source quiz ------------------------------------------------------
    v = Vector("source quiz completes twice", [SRC, "completion counts", "never in the Trash"])
    v.play(v.source, "CCCCCCCCCM")  # 90%: descends although above the threshold
    l2, _ = v.finish(v.source)
    v.play(l2, "C")
    v.finish(l2)
    v.play(v.source, "CCCCCCCCCC")  # no misses: complete, reset, playable at Level 1
    v.finish(v.source)
    v.play(v.source, "MMMMMMMMMM")  # nothing correct: reshuffled
    v.finish(v.source)
    v.play(v.source, "CCCCCCCCCC")  # a second completion
    v.finish(v.source)
    out.append(v)

    v = Vector("source quiz under drill", [SRC, PROG, "the Source quiz descends under Drill too"],
               opts=(0, "drill", False))
    v.play(v.source, "CMMMMMMMMM")  # 10%: descends, Level 2 on Drill
    l2, _ = v.finish(v.source)
    v.play(l2, "CCCCCMMMM")  # 5 of 9 below 80%: replaced under Drill
    l2b, _ = v.finish(l2)
    v.play(l2b, "CCCC")  # at the threshold, no misses: cleared, level removed
    v.finish(l2b)
    v.play(v.source, "CCCCCCCCCC")  # completed with the Source quiz plus one drill level
    v.finish(v.source)
    out.append(v)

    # -- Drill progression ---------------------------------------------------------
    v = Vector("drill outcomes", [PROG], opts=(0, "drill", False))
    v.play(v.source, "CCMMMMMMMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "MMMMMMMM")  # nothing correct: reset in place
    v.finish(l2)
    v.play(l2, "CCCCCCCM")  # 87.5% with a miss: cleared, replaced by its miss
    l2b, _ = v.finish(l2)
    v.play(l2b, "M")
    v.finish(l2b)
    out.append(v)

    # -- Restore, purge, trash ----------------------------------------------------
    v = Vector("restore into live, complete and trashed cascades", [TRASH, "restoring", "peak_depth on restore"])
    v.play(v.source, "CCCCCCCCMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CM")  # 50%: descends
    l3, _ = v.finish(l2)
    v.play(l3, "C")
    v.finish(l3)  # cleared, level 3 removed
    v.play(l2, "C")  # mid-attempt on Level 2: one card graded
    v.op({"type": "restore_quiz", "quiz": l3, "shuffle_seed": v.next_seed()})  # onto a mid-attempt quiz
    v.play(l3, "C")
    v.finish(l3)
    v.play(l2, "C")  # Level 2 resumes where it was
    v.finish(l2)
    v.play(v.source, "CCCCCCCCCC")
    v.finish(v.source)  # complete
    v.op({"type": "restore_quiz", "quiz": l2, "shuffle_seed": v.next_seed()})  # into a complete cascade
    v.op({"type": "trash_cascade"})
    v.op({"type": "grade", "quiz": l2, "attempt": v.quiz(l2).attempt, "attempt_seed": str(v.quiz(l2).seed),
          "question_idx": v.quiz(l2).idx[0], "grade": "correct"}, expect="trashed")
    v.op({"type": "restore_quiz", "quiz": l3, "shuffle_seed": v.next_seed()})  # into a trashed cascade
    out.append(v)

    v = Vector("purges and the frozen trash", [TRASH, OPS, "purges"])
    v.play(v.source, "CCCCCCCCCM")
    l2, _ = v.finish(v.source)
    v.op({"type": "purge_quiz", "quiz": l2}, expect="not_cleared")
    v.play(l2, "C")
    v.finish(l2)
    v.op({"type": "restore_cascade"}, expect="not_trashed")
    v.op({"type": "purge_cascade"}, expect="not_trashed")
    v.op({"type": "purge_quiz", "quiz": l2})
    v.op({"type": "purge_quiz", "quiz": l2}, expect="not_found")
    v.op({"type": "trash_cascade"})
    v.op({"type": "trash_cascade"}, expect="trashed")
    v.op({"type": "set_cascade_options", "segment_size": 5}, expect="trashed")
    v.op({"type": "restore_cascade"})
    v.op({"type": "trash_cascade"})
    v.op({"type": "purge_cascade"})
    v.op({"type": "trash_cascade"}, expect="not_found")
    out.append(v)

    # -- Segments --------------------------------------------------------------------
    v = Vector("segments: boundaries, continued and drilled runs", [SEG], count=22, opts=(5, "ladder", False))
    v.play(v.source, "CCCCC")  # a run with no misses creates nothing
    v.finish_segment(v.source, 5)
    v.play(v.source, "CCMCM")
    chain, _ = v.finish_segment(v.source, 10)  # drilled: a chain quiz one level down
    v.play(chain, "CM")  # the chain drills down to nothing
    chain2, _ = v.finish(chain)
    v.play(chain2, "C")
    v.finish(chain2)  # back up to the parent's next run
    v.play(v.source, "CCCCC")
    v.finish_segment(v.source, 15)
    v.play(v.source, "CCCCC")
    v.finish_segment(v.source, 20)
    v.play(v.source, "CC")  # the last run ends at the last question
    v.finish(v.source)
    out.append(v)

    v = Vector("segment descent in a drill cascade", [SEG, PROG], count=10, opts=(5, "drill", False))
    v.play(v.source, "CCCCM")
    chain, _ = v.finish_segment(v.source, 5)
    v.play(chain, "C")
    v.finish(chain)
    v.play(v.source, "MMMMM")
    v.finish(v.source)
    out.append(v)

    v = Vector("segments off or too large", [SEG, "next_boundary None"], count=10, opts=(10, "ladder", False))
    v.finish_segment(v.source, 5, expect="bad_segment")
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 0})
    v.finish_segment(v.source, 5, expect="bad_segment")
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 4}, expect="invalid")
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 5})
    v.op({"type": "set_cascade_options", "segment_size": 4}, expect="invalid")
    v.op({"type": "set_cascade_options", "segment_size": 0})
    v.op({"type": "set_cascade_options", "segment_size": 5})
    out.append(v)

    v = Vector("run numbering after a size change", [SEG, "How a run is numbered"], count=250, opts=(100, "ladder", False))
    v.play(v.source, "C" * 99 + "M")
    chain, _ = v.finish_segment(v.source, 100)
    v.play(chain, "C")
    v.finish(chain)
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 30})  # run 4 of 9 · 1 of 20
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 0})  # run_start survives
    v.op({"type": "move_cursor", "quiz": v.source, "attempt": 1, "attempt_seed": str(v.quiz(v.source).seed),
          "position": 99}, expect="bad_cursor")
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 300000})
    v.op({"type": "move_cursor", "quiz": v.source, "attempt": 1, "attempt_seed": str(v.quiz(v.source).seed),
          "position": 99}, expect="bad_cursor")
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 100})
    v.play(v.source, "C" * 100)
    v.finish_segment(v.source, 200)  # run 3 of 3 · 1 of 50 on the last run
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 50})  # a size that divides the count
    out.append(v)

    v = Vector("segments turned on mid-attempt", [SEG, "How a run is numbered"], count=250)
    v.play(v.source, "C" * 200)
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 50})  # run 1 of 5 · 201 of 250
    out.append(v)

    v = Vector("the ceil(question_count / S) bound", [OPTS, SEG], count=40, opts=(5, "ladder", False))
    for r in range(8):
        v.play(v.source, "CCCCM", checkpoint=False)
        if r < 7:
            chain, _ = v.finish_segment(v.source, 5 * (r + 1))
            v.play(chain, "C", checkpoint=False)
            v.finish(chain)
    last, _ = v.finish(v.source)  # the last run's miss descends with the whole attempt's misses
    out.append(v)

    v = Vector("size changes after a descent", [SEG, "run starts"], count=100, opts=(20, "ladder", False))
    v.play(v.source, "C" * 19 + "M")
    chain, _ = v.finish_segment(v.source, 20)
    v.play(chain, "C")
    v.finish(chain)
    v.play(v.source, "C" * 19 + "M")
    chain, _ = v.finish_segment(v.source, 40)
    v.play(chain, "C")
    v.finish(chain)
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 15})  # next boundary 45, not 30
    v.finish_segment(v.source, 30, expect="bad_segment")
    v.play(v.source, "CCCCC")
    v.finish_segment(v.source, 45)
    out.append(v)

    v = Vector("a one-question run", [OPTS, "run starts"], count=300, opts=(7, "ladder", False))
    for _ in range(17):
        v.play(v.source, "C" * 7, checkpoint=False)
        v.finish_segment(v.source, v.quiz(v.source).cursor + 1, expect="applied")
    v.op({"type": "set_quiz_options", "quiz": v.source, "segment_size": 120})  # next boundary 120: one question
    v.play(v.source, "C")
    v.finish_segment(v.source, 120)
    out.append(v)

    v = Vector("previous never crosses a run boundary", [SEG, "run starts"], count=20, opts=(5, "ladder", False))
    v.play(v.source, "CCCCC")
    v.finish_segment(v.source, 5)
    v.op({"type": "move_cursor", "quiz": v.source, "attempt": 1, "attempt_seed": str(v.quiz(v.source).seed),
          "position": 4}, expect="bad_cursor")
    v.op({"type": "move_cursor", "quiz": v.source, "attempt": 1, "attempt_seed": str(v.quiz(v.source).seed),
          "position": 10}, expect="bad_cursor")
    v.finish_segment(v.source, 5, expect="bad_segment")
    v.finish_segment(v.source, 10, expect="ungraded")
    v.finish_segment(v.source, 7, expect="bad_segment")
    v.finish_segment(v.source, 20, expect="bad_segment")
    out.append(v)

    # -- Options ----------------------------------------------------------------------
    v = Vector("options are copied, never referenced", [OPTS])
    v.op({"type": "set_cascade_options", "progression": "drill", "require_alphabetical": True})
    v.play(v.source, "CCCCCMMMMM")
    l2, _ = v.finish(v.source)  # the new quiz takes the cascade's options
    v.op({"type": "set_quiz_options", "quiz": l2, "require_alphabetical": False})  # touches nothing else
    v.op({"type": "set_cascade_options", "segment_size": 5})  # touches no existing quiz
    v.op({"type": "set_quiz_options", "quiz": v.source, "progression": "drill"}, expect="invalid")
    out.append(v)

    v = Vector("a chain stays drill through replacements", [OPTS, SEG], count=10, opts=(5, "ladder", False))
    v.play(v.source, "CMMMM")
    chain, _ = v.finish_segment(v.source, 5)
    v.op({"type": "set_quiz_options", "quiz": chain, "progression": "ladder"}, expect="invalid")
    v.op({"type": "set_quiz_options", "quiz": chain, "segment_size": 5}, expect="invalid")
    v.op({"type": "set_quiz_options", "quiz": chain, "require_alphabetical": True})
    v.play(chain, "CMMM")
    c2, _ = v.finish(chain)
    v.play(c2, "CMM")
    c3, _ = v.finish(c2)
    out.append(v)

    v = Vector("a quiz's own progression decides", [OPTS, PROG])
    v.play(v.source, "CCCCCMMMMM")
    l2, _ = v.finish(v.source)
    v.op({"type": "set_quiz_options", "quiz": l2, "progression": "drill"})
    v.play(l2, "CCMMM")  # below the threshold: replaced, as Drill
    l2b, _ = v.finish(l2)
    out.append(v)

    v = Vector("switched to ladder in a drill cascade", [OPTS, PROG], opts=(0, "drill", False))
    v.play(v.source, "CCCCCMMMMM")
    l2, _ = v.finish(v.source)
    v.op({"type": "set_quiz_options", "quiz": l2, "progression": "ladder"})
    v.play(l2, "CCMMM")  # below the threshold: descends, as Ladder
    v.finish(l2)
    out.append(v)

    v = Vector("options on a waiting upper level", [OPTS, SEG], count=20)
    v.play(v.source, "CCCCCMMMMMCCCCCMMMMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CCMMMMMMMM")
    l3, _ = v.finish(l2)
    v.op({"type": "set_quiz_options", "quiz": l2, "segment_size": 5})  # Level 2 waits, reset
    v.play(l3, "CCCCCCCC")
    v.finish(l3)  # back up: Level 2 now has its boundary at 5
    out.append(v)

    # -- Restore details -------------------------------------------------------------
    v = Vector("restores take the cascade's current options", [TRASH, OPTS], count=10, opts=(0, "ladder", False))
    v.play(v.source, "CCCCCCCCMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CC")
    v.finish(l2)
    v.op({"type": "set_cascade_options", "segment_size": 5, "progression": "drill"})
    v.op({"type": "restore_quiz", "quiz": l2, "shuffle_seed": v.next_seed()})  # takes the new options
    out.append(v)

    v = Vector("restore on top of a chain", [TRASH, SEG], count=20, opts=(5, "ladder", False))
    v.play(v.source, "CCCCM")
    chain, _ = v.finish_segment(v.source, 5)
    v.play(chain, "C")
    v.finish(chain)  # the chain quiz goes to the Trash
    v.play(v.source, "CCCCM")
    chain2, _ = v.finish_segment(v.source, 10)
    v.op({"type": "restore_quiz", "quiz": chain, "shuffle_seed": v.next_seed()})  # on top of the chain
    v.play(chain, "C")
    v.finish(chain)  # the restored chain returns to the level above it
    v.play(chain2, "C")
    v.finish(chain2)  # and the chain resumes its parent's run
    out.append(v)

    # -- Attempt seeds -----------------------------------------------------------
    v = Vector("attempt seeds tie operations together", [OPS, "stale_attempt"])
    q = v.quiz(v.source)
    bad = str((q.seed + 1) % (1 << 64))
    v.op({"type": "grade", "quiz": v.source, "attempt": 1, "attempt_seed": bad, "question_idx": 0,
          "grade": "correct"}, expect="stale_attempt")
    v.op({"type": "move_cursor", "quiz": v.source, "attempt": 1, "attempt_seed": bad, "position": 1},
         expect="stale_attempt")
    v.op({"type": "finish", "quiz": v.source, "attempt": 2, "attempt_seed": str(q.seed),
          "shuffle_seed": v.next_seed(), "new_quiz_id": v.next_id()}, expect="stale_attempt")
    v.op({"type": "grade", "quiz": v.source, "attempt": 1, "attempt_seed": str(q.seed), "question_idx": 99,
          "grade": "correct"}, expect="not_found")
    v.finish(v.source, expect="ungraded")
    out.append(v)

    v = Vector("a 1,200-operation outbox with a finish at 700", [OPS, "The sync cycle"], count=350)
    v.play(v.source, "C" * 100 + "M" * 250, checkpoint=False)
    l2, _ = v.finish(v.source)
    assert len(v.steps) == 700, len(v.steps)
    v.play(l2, "C" * 250, checkpoint=False)
    v.finish(l2)
    assert len(v.steps) == 1200, len(v.steps)
    out.append(v)

    # -- Completion counts --------------------------------------------------------
    v = Vector("completion counts", [SRC, "Completion counts"], count=10, opts=(5, "ladder", False))
    v.play(v.source, "CCCCM")  # a segmented Source quiz
    chain, _ = v.finish_segment(v.source, 5)
    v.play(chain, "C")
    v.finish(chain)
    v.play(v.source, "CCCCC")
    l2, _ = v.finish(v.source)  # the drilled miss still makes the finish a descent
    v.play(l2, "C")
    v.finish(l2)
    v.play(v.source, "CCCCC")
    v.finish_segment(v.source, 5)
    v.play(v.source, "CCCCC")
    v.finish(v.source)  # completed: levels 3, attempts counted
    v.op({"type": "restore_quiz", "quiz": l2, "shuffle_seed": v.next_seed()})  # peak_depth 2
    v.op({"type": "set_quiz_options", "quiz": l2, "segment_size": 5})
    out.append(v)

    v = Vector("peak depth 3 below a restored quiz", [SRC, "Completion counts"], count=10)
    v.play(v.source, "CCMMMMMMMM")
    l2, _ = v.finish(v.source)
    v.play(l2, "CCCCCCCC")
    v.finish(l2)
    v.play(v.source, "CCCCCCCCCC")
    v.finish(v.source)  # complete: peak_depth back to 1
    v.op({"type": "set_cascade_options", "segment_size": 5})
    v.op({"type": "restore_quiz", "quiz": l2, "shuffle_seed": v.next_seed()})  # peak_depth 2
    v.play(l2, "CCCCM")
    v.finish_segment(l2, 5)  # a segment descent below the restored quiz: peak_depth 3
    out.append(v)

    # -- Check order (PQ-013) -------------------------------------------------------
    v = Vector("the attempt and a duplicate run come before the depth", [OPS, "PLAN.md § Conflicts", "PQ-013"],
               count=10, opts=(5, "ladder", False))
    v.play(v.source, "CCCCM")
    chain, _ = v.finish_segment(v.source, 5)
    v.finish_segment(v.source, 5, expect="duplicate_segment")  # not the deepest, but a duplicate
    v.play(chain, "C")
    v.finish(chain)
    v.play(v.source, "CCCCM")
    q = v.quiz(v.source)
    old = {"attempt": q.attempt, "attempt_seed": str(q.seed)}
    v.finish(v.source)  # descends: the Source quiz is reset and no longer the deepest
    v.op({"type": "finish", "quiz": v.source, **old, "shuffle_seed": v.next_seed(), "new_quiz_id": v.next_id()},
         expect="stale_attempt")
    v.finish(v.source, expect="not_deepest")
    out.append(v)
    return out


def random_sequences(n_seqs=40, n_steps=160):
    """Seeded random sequences of operations chosen from the current state:
    grades, cursor moves (Previous included), run finishes at boundaries,
    segment-size changes including 0 and past the count, finishes, restores of
    ordinary and chain quizzes, trashing and restoring the cascade (PLAN.md §
    Unit tests → Property tests). Shared by the Rust and TypeScript tests."""
    out = []
    for s in range(n_seqs):
        rng = ref.splitmix64(0xD1CE + s)
        pick = lambda n: next(rng) % n  # noqa: E731
        count = 5 + pick(40)
        opts = ([0, 0, 5, 7, 10, 100][pick(6)], ["ladder", "drill"][pick(2)], bool(pick(2)))
        v = Vector(f"random sequence {s}", ["PLAN.md § Unit tests → Property tests"], count=count,
                   threshold=[1, 50, 80, 100][pick(4)], opts=opts)
        for step in range(n_steps):
            c = v.c
            active = [q for q in c.quizzes.values() if q.status == "active"]
            cleared = [q for q in c.quizzes.values() if q.status == "cleared"]
            deep = c.active_at(c.depth) if not c.trashed else None
            r = pick(100)
            if c.trashed:
                if r < 50 or not cleared:
                    v.op({"type": "restore_cascade"}, checkpoint=False)
                else:
                    q = cleared[pick(len(cleared))]
                    v.op({"type": "restore_quiz", "quiz": q.id, "shuffle_seed": v.next_seed()}, checkpoint=False)
            elif r < 55 and deep is not None:
                # Study the deepest level: grade the card and move on, or finish.
                q = deep
                pos = q.positions()
                p = q.cursor
                nb = q.next_boundary()
                end = nb if nb is not None else len(q.idx)
                if all(pos[i] in q.grades for i in range(q.run_start, end)) and (p + 1 == end):
                    if nb is not None:
                        v.finish_segment(q.id, nb, expect=None)
                    else:
                        v.finish(q.id, expect=None)
                else:
                    g = "correct" if pick(100) < 60 else "missed"
                    v.op({"type": "grade", "quiz": q.id, "attempt": q.attempt, "attempt_seed": str(q.seed),
                          "question_idx": pos[p], "grade": g}, checkpoint=False)
                    if p + 1 < end:
                        v.op({"type": "move_cursor", "quiz": q.id, "attempt": q.attempt,
                              "attempt_seed": str(q.seed), "position": p + 1}, checkpoint=False)
            elif r < 62 and deep is not None:
                q = deep  # Previous, never below run_start
                v.op({"type": "move_cursor", "quiz": q.id, "attempt": q.attempt, "attempt_seed": str(q.seed),
                      "position": max(q.cursor - 1, 0)}, checkpoint=False)
            elif r < 70 and active:
                q = active[pick(len(active))]  # a regrade anywhere, even a passed run
                v.op({"type": "grade", "quiz": q.id, "attempt": q.attempt, "attempt_seed": str(q.seed),
                      "question_idx": q.idx[pick(len(q.idx))], "grade": ["correct", "missed"][pick(2)]},
                     checkpoint=False)
            elif r < 78 and active:
                q = active[pick(len(active))]
                size = [0, 5, 6, 7, 9, 13, 300000, len(q.idx), len(q.idx) + 5][pick(9)]
                v.op({"type": "set_quiz_options", "quiz": q.id, "segment_size": size}, checkpoint=False)
            elif r < 83:
                v.op({"type": "set_cascade_options", "segment_size": [0, 5, 8, 20][pick(4)],
                      "progression": ["ladder", "drill"][pick(2)]}, checkpoint=False)
            elif r < 93 and cleared:
                q = cleared[pick(len(cleared))]
                v.op({"type": "restore_quiz", "quiz": q.id, "shuffle_seed": v.next_seed()}, checkpoint=False)
            elif r < 96:
                v.op({"type": "trash_cascade"}, checkpoint=False)
            elif deep is not None:
                v.finish(deep.id, expect=None)
            if step % 10 == 9 and v.steps:
                v.steps[-1]["state"] = c.snapshot()
        if v.steps:
            v.steps[-1]["state"] = v.c.snapshot()
        out.append(v.to_json())
    return out


def shuffle_vectors():
    cases = []
    for n, seed in [(1, 0), (2, 1), (10, 42), (10, (1 << 64) - 1), (20, seed_of(900)), (37, seed_of(901)),
                    (100, BIG + 12345), (5, 1 << 53)]:
        idx = list(range(n))
        cases.append({"seed": str(seed), "idx": idx, "positions": ref.shuffle(idx, seed)})
    # Out-of-order input with gaps: the questions are sorted by idx first.
    idx = [42, 7, 1000, 3, 299999, 15]
    cases.append({"seed": str(seed_of(902)), "idx": idx, "positions": ref.shuffle(idx, seed_of(902))})
    # The reset seed.
    s = seed_of(903)
    resets = [{"seed": str(s), "reset_seed": str(s ^ ref.RESET_XOR)},
              {"seed": str(BIG + 1), "reset_seed": str((BIG + 1) ^ ref.RESET_XOR)}]
    first_outputs = []
    for x in [0, 1234567, BIG + 99]:
        g = ref.splitmix64(x)
        first_outputs.append({"seed": str(x), "outputs": [str(next(g)) for _ in range(5)]})
    return {"source": "contract-fixtures/tools/reference.py (Vigna's splitmix64.c; Fisher–Yates)",
            "splitmix64": first_outputs, "shuffles": cases, "reset_seeds": resets}


def hash_vectors():
    cases = [
        {"name": "one question", "idx": [0]},
        {"name": "three questions", "idx": [1, 2, 3]},
        {"name": "three questions differing in one index", "idx": [1, 2, 4]},
        {"name": "sparse", "idx": [299999, 7, 12345]},
        {"name": "300,000-question Source quiz", "idx_range": [0, 300000]},
    ]
    for c in cases:
        idx = c.get("idx") or list(range(*c["idx_range"]))
        h = ref.questions_hash(idx)
        c["hash"] = str(h)
        c["above_2_53"] = h > (1 << 53)
        c["above_2_63"] = h > BIG
    assert all(c["above_2_53"] for c in cases), cases
    assert any(c["above_2_63"] for c in cases), cases
    return {"source": "contract-fixtures/tools/reference.py (FNV-1a 64, little-endian u32 indexes)", "hashes": cases}


def leave_vectors():
    cases = []
    for v, d in [(0.15, 1), (0.25, 1), (2.5, 0), (-0.04, 1), (0.04, 1), (34.117, 1), (-8.35, 1), (28.292, 3),
                 (1000000.0, 3), (-1000000.0, 3), (1000000.0, 0), (-0.378, 2), (0.0, 1), (-0.0, 2), (0.5, 0),
                 (-0.5, 0), (1.005, 2), (12.75, 1), (-4.0, 1), (25.6, 0)]:
        cases.append({"value": v, "decimals": d, "screen": ref.leave_text(v, d, True),
                      "export": ref.leave_text(v, d, False)})
    return {"source": "contract-fixtures/tools/reference.py (PLAN.md § Answers rounding)", "cases": cases}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    vectors = [v.to_json() for v in scenarios()]
    (OUT / "rules.json").write_text(json.dumps({"source": "contract-fixtures/tools/reference.py", "vectors": vectors},
                                               indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    seqs = random_sequences()
    (OUT / "random.json").write_text(json.dumps({"source": "contract-fixtures/tools/reference.py (seeded generator)",
                                                 "vectors": seqs}, separators=(",", ":")) + "\n")
    sv = shuffle_vectors()
    (OUT / "shuffle.json").write_text(json.dumps(sv, indent=1) + "\n")
    (OUT / "hash.json").write_text(json.dumps(hash_vectors(), indent=1) + "\n")
    (OUT / "leave_text.json").write_text(json.dumps(leave_vectors(), indent=1, ensure_ascii=False) + "\n",
                                          encoding="utf-8")
    print(f"wrote {len(vectors)} rule vectors, {sum(len(v['steps']) for v in vectors)} steps")


if __name__ == "__main__":
    main()
