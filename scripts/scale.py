#!/usr/bin/env python3
"""make test-scale: the 300,000-question budgets (PLAN.md § Scale tests).

Brings up its own stack through scripts/stack.py and runs the scale suites.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

SUITES: list = []  # filled in by Phase 7h


def main() -> int:
    if not SUITES:
        print("test-scale: no scale suites yet (PLAN.md Phase 7h)")
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
