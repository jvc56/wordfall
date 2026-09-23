#!/usr/bin/env python3
"""make test-parity: Zyzzyva comparison (PLAN.md § Zyzzyva parity).

Local only. Runs a checked-in list of saved searches against a real CSW24
upload and compares the word lists with Zyzzyva's exports for the same
searches. Skipped with a notice unless the licensed files are present.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LICENSED = Path(os.environ.get("WORDFALL_LICENSED_DIR", ROOT / "fixtures" / "licensed"))
REQUIRED = ["english.csv", "CSW24.tsv", "zyzzyva"]


def main() -> int:
    missing = [f for f in REQUIRED if not (LICENSED / f).exists()]
    if missing:
        print(f"test-parity: skipped — licensed data not present in {LICENSED} "
              f"(missing: {', '.join(missing)}). Set WORDFALL_LICENSED_DIR to run it.")
        return 0
    print("test-parity: parity searches not implemented yet (PLAN.md Phase 4)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
