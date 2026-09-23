#!/usr/bin/env python3
"""make test-parity: Zyzzyva comparison (PLAN.md § Zyzzyva parity).

Local only; never in CI. Runs the checked-in searches in
fixtures/parity/searches.json against a real CSW24 upload and compares each
word list with Zyzzyva's export for the same search. Zyzzyva has no OR, so the
searches are single AND groups, and Wordfall's `.` wildcard is written as
Zyzzyva's `?` when the search is shown in Zyzzyva's terms.

It also prints the figures that need real data: each index's build time and
resident size, and the wall-clock time of every search. They are recorded, not
asserted.

Licensed files, from WORDFALL_LICENSED_DIR (default fixtures/licensed/,
which is gitignored):
    english.csv                 the English letter distribution
    CSW24.tsv                   the lexicon
    CSW24_leaves.csv            optional leave values
    zyzzyva/<name>.txt          Zyzzyva's export for each search, one word per line

Skipped with a notice unless those files are present.
"""

from __future__ import annotations

import json
import os
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import stack  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
LICENSED = Path(os.environ.get("WORDFALL_LICENSED_DIR", ROOT / "fixtures" / "licensed"))
SEARCHES = ROOT / "fixtures" / "parity" / "searches.json"
DEVIATIONS = ROOT / "fixtures" / "parity" / "deviations.json"
PROJECT = "wordfall-parity"
PORT = int(os.environ.get("PARITY_PORT", "5190"))
LEXICON = "CSW24"


def zyzzyva_form(filters: dict) -> str:
    """The search as it is entered in Zyzzyva: `.` becomes `?`."""
    parts = []
    for c in filters["children"]:
        params = {k: v for k, v in c.items() if k not in ("type", "negated")}
        if "pattern" in params:
            params["pattern"] = params["pattern"].replace(" ", "").replace(".", "?")
        neg = "NOT " if c["negated"] else ""
        parts.append(f"{neg}{c['type']} {json.dumps(params)}")
    return " AND ".join(parts)


def missing_files(searches: list) -> list:
    need = [LICENSED / "english.csv", LICENSED / f"{LEXICON}.tsv"]
    need += [LICENSED / "zyzzyva" / f"{s['name']}.txt" for s in searches]
    return [str(p.relative_to(LICENSED)) for p in need if not p.exists()]


def read_export(path: Path) -> list:
    words = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            words.append(line.split()[0].upper())
    return sorted(set(words))


def main() -> int:
    searches = json.loads(SEARCHES.read_text())
    missing = missing_files(searches)
    if missing:
        print(f"test-parity: skipped — licensed data not present in {LICENSED} "
              f"(missing: {', '.join(missing[:5])}{' …' if len(missing) > 5 else ''}). "
              f"Set WORDFALL_LICENSED_DIR to run it.")
        print("The searches, as entered in Zyzzyva:")
        for s in searches:
            print(f"  {s['name']}: {zyzzyva_form(s['filters'])}")
        return 0

    deviations = json.loads(DEVIATIONS.read_text())
    catalog = stack.Catalog([
        stack.CatalogItem("distribution", "english", LICENSED / "english.csv"),
        stack.CatalogItem("lexicon", LEXICON, LICENSED / f"{LEXICON}.tsv", "english"),
    ])
    leaves = LICENSED / f"{LEXICON}_leaves.csv"
    if leaves.exists():
        catalog.items.append(stack.CatalogItem("leaves", LEXICON, leaves))

    base_url = stack.up(PROJECT, PORT, {"MAX_CASCADES_PER_USER": "1000",
                                         "SEARCH_RATE_PER_MINUTE": "1000",
                                         "DOWNLOAD_RATE_PER_MINUTE": "10000",
                                         "ADMIN_UPLOAD_RATE_PER_MINUTE": "100"})
    http = stack.seed(base_url, catalog, stack.User(), project=PROJECT)

    status, admin = http.json("GET", "/api/admin/catalog")
    for kind in ("lexicons", "leave_sets"):
        for item in admin.get(kind, []):
            label = item.get("name") or item.get("lexicon")
            print(f"index {kind[:-1]} {label}: build {item.get('build_ms')} ms, "
                  f"resident ≈ {(item.get('index_bytes') or 0) / 1e6:.1f} MB")

    failures = 0
    device_id = str(uuid.uuid4())
    for s in searches:
        body = {
            "id": str(uuid.uuid4()), "source_quiz_id": str(uuid.uuid4()), "device_id": device_id,
            "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "name": f"parity {s['name']}", "lexicon": LEXICON, "quiz_type": "definition",
            "clear_threshold": 80, "segment_size": 0, "progression": "ladder",
            "require_alphabetical": False, "filters": s["filters"],
        }
        t0 = time.monotonic()
        status, created = http.json("POST", "/api/cascades", body)
        elapsed_ms = (time.monotonic() - t0) * 1000
        if status != 201 and status != 200:
            print(f"FAIL {s['name']}: creation answered {status} {created}")
            failures += 1
            continue
        cascade = created["cascade"]
        count = cascade["question_count"]
        got = []
        for start in range(0, count, 100_000):
            status, page = http.json("GET", f"/api/cascades/{cascade['id']}/cards?from={start}&limit=100000&keys=1")
            got.extend(page["keys"])
        got = sorted(w.replace("[", "").replace("]", "") for w in got)
        expected = read_export(LICENSED / "zyzzyva" / f"{s['name']}.txt")
        extra = sorted(set(got) - set(expected))
        lacking = sorted(set(expected) - set(got))
        known = deviations.get(s["name"])
        ok = not extra and not lacking
        verdict = "ok" if ok else ("known deviation" if known else "DIFF")
        print(f"{verdict:>15} {s['name']}: {len(got)} words, search {elapsed_ms:.0f} ms"
              + ("" if ok else f"; only in Wordfall {extra[:10]}, only in Zyzzyva {lacking[:10]}"))
        if not ok and not known:
            failures += 1
    stack.down(PROJECT)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
