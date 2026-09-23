#!/usr/bin/env python3
"""./scripts/dev.py: a complete Wordfall on http://localhost:5173 in one command.

A thin command line over scripts/stack.py (PLAN.md § Development). Safe to run
again at any time: each step checks whether it has already been done.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import webbrowser
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import stack  # noqa: E402


def _pair(value: str, sep: str, flag: str):
    if sep not in value:
        raise argparse.ArgumentTypeError(f"{flag} expects the form A{sep}B, got {value!r}")
    return value.split(sep, 1)


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--distribution", action="append", default=[], metavar="NAME=FILE",
                   help="upload a letter distribution file as well as the fixtures")
    p.add_argument("--lexicon", action="append", default=[], metavar="NAME:DIST=FILE",
                   help="upload a lexicon file as well as the fixtures")
    p.add_argument("--leaves", action="append", default=[], metavar="LEXICON=FILE",
                   help="upload a leave values file as well as the fixtures")
    p.add_argument("--no-fixtures", action="store_true",
                   help="seed only what the flags above name")
    p.add_argument("--hot-reload", action="store_true",
                   help="also run the Vite dev server on :5174 (no service worker there)")
    p.add_argument("--reset", action="store_true",
                   help="drop the database volume and the built frontend, then start clean")
    p.add_argument("--project", default=stack.DEFAULT_PROJECT)
    p.add_argument("--port", type=int, default=stack.DEFAULT_PORT)
    p.add_argument("--env", action="append", default=[], metavar="KEY=VALUE",
                   help="override any configuration variable")
    p.add_argument("--no-browser", action="store_true")
    p.add_argument("--quiet", action="store_true")
    p.add_argument("--down", action="store_true", help="stop the stack")
    p.add_argument("--volumes", action="store_true", help="with --down, discard its data")
    args = p.parse_args(argv)

    if args.down:
        stack.down(args.project, volumes=args.volumes)
        return 0

    env = dict(_pair(e, "=", "--env") for e in args.env)
    catalog = stack.Catalog() if args.no_fixtures else stack.fixture_catalog()
    for d in args.distribution:
        name, path = _pair(d, "=", "--distribution")
        catalog.items.append(stack.CatalogItem("distribution", name, Path(path).expanduser()))
    for lx in args.lexicon:
        head, path = _pair(lx, "=", "--lexicon")
        name, dist = _pair(head, ":", "--lexicon")
        catalog.items.append(stack.CatalogItem("lexicon", name, Path(path).expanduser(), dist))
    for lv in args.leaves:
        name, path = _pair(lv, "=", "--leaves")
        catalog.items.append(stack.CatalogItem("leaves", name, Path(path).expanduser()))
    # Distributions first, then lexicons, then leave sets.
    order = {"distribution": 0, "lexicon": 1, "leaves": 2}
    catalog.items.sort(key=lambda i: order[i.kind])

    if args.reset:
        stack.down(args.project, volumes=True)
        subprocess.run(["docker", "image", "rm", "-f", f"{args.project}-frontend"],
                       check=False, capture_output=True)
        print("Reset: clear this site's data in the browser too (DevTools → Application → "
              "Storage → Clear site data), because local sync cursors no longer match "
              "anything on the server.", file=sys.stderr)

    profiles = ["hot-reload"] if args.hot_reload else []
    base_url = stack.up(args.project, args.port, env, build=True, profiles=profiles)
    user = stack.User()
    stack.seed(base_url, catalog, user, project=args.project)

    if not args.quiet:
        print(f"Wordfall is up at {base_url}")
        print(f"Log in as {user.username} with password {user.password}")
        if args.hot_reload:
            print("Hot reload at http://localhost:5174 (no service worker there)")
    if not args.no_browser:
        webbrowser.open(base_url)
    return 0


if __name__ == "__main__":
    sys.exit(main())
