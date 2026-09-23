#!/usr/bin/env python3
"""Export fixtures (PLAN.md § Exporting words, § Unit tests → "The export
formatters are held to one set of fixtures on both sides").

An independent reference, written from the plan's text alone, formats every
case; the Rust (backend/src/export) and TypeScript (frontend/src/lib/export)
formatters must reproduce each file byte for byte. Writes
contract-fixtures/export/cases.json.

The input model, shared by both sides:
  name, quiz_type, tiles (the distribution's tiles in tile order),
  questions: [{idx, key, words?|definition?|value?, front_hooks?, back_hooks?}]
             in search order (a Definition question may carry its hooks),
  quizzes:   [{level, active, order: [idx in study order], grades: {idx: grade}}],
  choices:   {scope, level?, which, format, lines?, columns?, order, decimals}
"""
import json
import math
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "export" / "cases.json"

EN_TILES = [chr(c) for c in range(ord("A"), ord("Z") + 1)]
CA_TILES = ["A", "B", "C", "Ç", "D", "E", "F", "G", "H", "I", "J", "L", "L·L", "M", "N", "NY", "O", "P", "QU",
            "R", "S", "T", "U", "V", "X", "Z"]
LEAVE_TILES = ["?"] + EN_TILES


# ----------------------------------------------------------------------------
# The reference
# ----------------------------------------------------------------------------

def split_tiles(s):
    """MAGPIE notation: a multi-character tile is written in brackets."""
    out, i = [], 0
    while i < len(s):
        if s[i] == "[":
            j = s.index("]", i)
            out.append(s[i:j + 1])
            i = j + 1
        else:
            out.append(s[i])
            i += 1
    return out


def tile_key(key, tiles):
    return [tiles.index(t[1:-1] if t.startswith("[") else t) for t in split_tiles(key)]


def leave_text(v, d):
    """The export form: half away from zero on the f64, no '+', never '-0'."""
    n = math.trunc(abs(v) * 10 ** d + 0.5)
    digits = str(n)
    if d > 0:
        digits = digits.rjust(d + 1, "0")
        digits = digits[:-d] + "." + digits[-d:]
    return ("-" if v < 0 and n > 0 else "") + digits


def grades_for(case):
    ch = case["choices"]
    if ch["scope"] == "quiz":
        q = next(q for q in case["quizzes"] if q["level"] == ch["level"] and q.get("pick", True))
        return {int(k): v for k, v in q["grades"].items()}, q
    out = {}
    for q in case["quizzes"]:
        if not q["active"]:
            continue
        for k, g in q["grades"].items():
            k = int(k)
            if g == "missed" or out.get(k) == "missed":
                out[k] = "missed"
            else:
                out[k] = "correct"
    return out, None


def entries(case):
    ch = case["choices"]
    by_idx = {q["idx"]: q for q in case["questions"]}
    grades, quiz = grades_for(case)
    if quiz is not None:
        idxs = list(quiz["order"])
        if ch["order"] == "alphabetical":
            idxs.sort(key=lambda i: tile_key(by_idx[i]["key"], case["tiles"]))
    else:
        idxs = [q["idx"] for q in case["questions"]]
    which = ch["which"]
    out = []
    for i in idxs:
        g = grades.get(i)
        if which == "all" or (which == "ungraded" and g is None) or g == which:
            out.append((by_idx[i], g))
    return out


def csv_field(s):
    if any(c in s for c in ',"\r\n'):
        return '"' + s.replace('"', '""') + '"'
    return s


def hooks_pair(front, back):
    return " ".join(split_tiles(front or "")) + "|" + " ".join(split_tiles(back or ""))


def cell(case, q, g, col, d):
    t = case["quiz_type"]
    if col == "question":
        return q["key"]
    if col == "grade":
        return g or ""
    if col == "answer":
        if t == "anagram":
            return " ".join(w["word"] for w in q["words"])
        if t == "definition":
            return q["definition"]
        return leave_text(q["value"], d)
    if col == "definition":
        if t == "anagram":
            return " | ".join(w.get("definition", "") for w in q["words"])
        if t == "definition":
            return q["definition"]
        return ""
    if col == "hooks":
        if t == "anagram":
            return " / ".join(hooks_pair(w.get("front_hooks"), w.get("back_hooks")) for w in q["words"])
        if t == "definition":
            return hooks_pair(q.get("front_hooks"), q.get("back_hooks"))
        return ""
    raise ValueError(col)


def body(case):
    ch = case["choices"]
    d = ch["decimals"]
    es = entries(case)
    if ch["format"] == "txt":
        lines = []
        for q, _ in es:
            if ch["lines"] == "questions":
                lines.append(q["key"])
            elif case["quiz_type"] == "anagram":
                lines.extend(w["word"] for w in q["words"])
            elif case["quiz_type"] == "definition":
                lines.append(q["definition"])
            else:
                lines.append(leave_text(q["value"], d))
        return "".join(line + "\n" for line in lines)
    cols = ch["columns"]
    rows = [",".join(cols)]
    for q, g in es:
        rows.append(",".join(csv_field(cell(case, q, g, c, d)) for c in cols))
    return "".join(r + "\r\n" for r in rows)


def filename(case):
    ch = case["choices"]
    base = case["name"]
    if ch["scope"] == "quiz":
        base += f" - L{ch['level']}"
    if ch["which"] != "all":
        base += f" {ch['which']}"
    safe = "".join(c if (c.isascii() and (c.isalnum() or c in " ._-")) else "_" for c in base)
    return safe[:100] + "." + ch["format"]


# ----------------------------------------------------------------------------
# The cases
# ----------------------------------------------------------------------------

ANAGRAM_QUESTIONS = [
    {"idx": 0, "key": "AEINRST", "words": [
        {"word": "ANESTRI", "definition": "a word, with a comma", "front_hooks": "", "back_hooks": "S"},
        {"word": "NASTIER", "definition": 'said "nasty" / more nasty', "front_hooks": "", "back_hooks": ""},
        {"word": "RETAINS", "definition": "keeps | holds", "front_hooks": "", "back_hooks": ""},
        {"word": "STAINER", "definition": "one who stains", "front_hooks": "", "back_hooks": "S"}]},
    {"idx": 1, "key": "AA", "words": [
        {"word": "AA", "definition": "lava", "front_hooks": "BCFM", "back_hooks": "HLS"}]},
    {"idx": 2, "key": "EIQSTU", "words": [
        {"word": "QUIETS", "definition": "calms", "front_hooks": "", "back_hooks": ""}]},
    {"idx": 3, "key": "ADEIRST", "words": [
        {"word": "ASTERID", "definition": "a plant", "front_hooks": "", "back_hooks": "S"},
        {"word": "STAIRED", "definition": "having stairs", "front_hooks": "", "back_hooks": ""}]},
    {"idx": 4, "key": "AEGINST", "words": [
        {"word": "EASTING", "definition": "a direction", "front_hooks": "", "back_hooks": "S"}]},
]
# Level 1 (the Source quiz), Level 2 of its misses, and a cleared Level 2 in the Trash.
ANAGRAM_QUIZZES = [
    {"level": 1, "active": True, "order": [3, 0, 4, 2, 1], "grades": {"0": "missed", "1": "correct", "3": "correct", "4": "missed"}},
    {"level": 2, "active": True, "order": [4, 0], "grades": {"0": "correct"}},
    {"level": 2, "active": False, "pick": False, "order": [2, 1], "grades": {"1": "missed", "2": "correct"}},
]

DEF_QUESTIONS = [
    {"idx": 0, "key": "A[NY]S", "definition": "anys, plural, of \"any\"", "front_hooks": "[NY]", "back_hooks": ""},
    {"idx": 1, "key": "[L·L]IBRE", "definition": "book", "front_hooks": "", "back_hooks": "S"},
    {"idx": 2, "key": "[QU]E", "definition": "what; that", "front_hooks": "", "back_hooks": ""},
    {"idx": 3, "key": "ÇA", "definition": "here,\nnear", "front_hooks": "", "back_hooks": ""},
]
DEF_QUIZZES = [
    {"level": 1, "active": True, "order": [2, 0, 3, 1], "grades": {"0": "correct", "2": "missed"}},
]

LEAVE_QUESTIONS = [
    {"idx": 0, "key": "?", "value": 25.55},
    {"idx": 1, "key": "?S", "value": 34.05},
    {"idx": 2, "key": "ER", "value": -8.35},
    {"idx": 3, "key": "Q", "value": -0.04},
    {"idx": 4, "key": "IU", "value": 0.0},
    {"idx": 5, "key": "EST", "value": 1.0005},
]
LEAVE_QUIZZES = [
    {"level": 1, "active": True, "order": [5, 2, 0, 4, 1, 3], "grades": {"1": "missed", "2": "correct", "5": "correct"}},
]

WHICH = ["all", "correct", "missed", "ungraded"]


def cases():
    out = []

    def add(name, data, choices):
        case = {"name": data["name"], "quiz_type": data["quiz_type"], "tiles": data["tiles"],
                "questions": data["questions"], "quizzes": data["quizzes"], "choices": choices}
        case["expected"] = {"filename": filename(case), "body": body(case)}
        case["case"] = name
        out.append(case)

    datasets = [
        {"name": "EN-FIX · Length 7–7", "quiz_type": "anagram", "tiles": EN_TILES,
         "questions": ANAGRAM_QUESTIONS, "quizzes": ANAGRAM_QUIZZES},
        {"name": "CA defs", "quiz_type": "definition", "tiles": CA_TILES,
         "questions": DEF_QUESTIONS, "quizzes": DEF_QUIZZES},
        {"name": "Leaves", "quiz_type": "leave_value", "tiles": LEAVE_TILES,
         "questions": LEAVE_QUESTIONS, "quizzes": LEAVE_QUIZZES},
    ]
    for data in datasets:
        t = data["quiz_type"]
        default_lines = "answers" if t == "anagram" else "questions"
        all_cols = ["question", "answer", "grade"] if t == "leave_value" else ["question", "answer", "definition", "hooks", "grade"]
        decimals = [0, 1, 2, 3] if t == "leave_value" else [1]
        scopes = [("cascade", None, ["study"])]
        for q in data["quizzes"]:
            if q["active"]:
                scopes.append(("quiz", q["level"], ["study", "alphabetical"]))
        for scope, level, orders in scopes:
            for order in orders:
                for which in WHICH:
                    for d in decimals:
                        base = {"scope": scope, "which": which, "order": order, "decimals": d}
                        if level is not None:
                            base["level"] = level
                        for lines in ["answers", "questions"]:
                            add(f"{t} {scope}{level or ''} {order} {which} txt {lines} d{d}",
                                data, {**base, "format": "txt", "lines": lines})
                        add(f"{t} {scope}{level or ''} {order} {which} csv default d{d}",
                            data, {**base, "format": "csv", "columns": ["question", "answer", "grade"]})
                        add(f"{t} {scope}{level or ''} {order} {which} csv all d{d}",
                            data, {**base, "format": "csv", "columns": all_cols})
        # The default lines for this type, kept as a named case.
        add(f"{t} default lines", data, {"scope": "cascade", "which": "all", "order": "study", "decimals": 1,
                                          "format": "txt", "lines": default_lines})

    # A quiz in the Trash: the attempt it finished on.
    trash = {"name": "Trash export", "quiz_type": "anagram", "tiles": EN_TILES, "questions": ANAGRAM_QUESTIONS,
             "quizzes": [dict(q, pick=True) if not q["active"] else dict(q, pick=False) for q in ANAGRAM_QUIZZES]}
    for which in WHICH:
        add(f"trash quiz {which}", trash, {"scope": "quiz", "level": 2, "which": which, "order": "study",
                                          "decimals": 1, "format": "csv", "columns": ["question", "answer", "grade"]})
    # Filenames: a character outside the Basic Multilingual Plane is one '_',
    # and a long name is cut to 100 characters before the extension.
    for name in ["Emoji 🎲 list", "A" * 60 + " and then " + "B" * 60]:
        d = {"name": name, "quiz_type": "leave_value", "tiles": LEAVE_TILES, "questions": LEAVE_QUESTIONS,
             "quizzes": LEAVE_QUIZZES}
        add(f"filename {name[:12]}", d, {"scope": "quiz", "level": 1, "which": "missed", "order": "study",
                                         "decimals": 1, "format": "txt", "lines": "questions"})
    return out


def main():
    cs = cases()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps({"cases": cs}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(cs)} export cases")


if __name__ == "__main__":
    main()
