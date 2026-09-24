#!/usr/bin/env python3
"""Restore a Wordfall dump, and force every device to resync (PLAN.md § Backups → Restoring).

  ./scripts/restore.py backups/wordfall-2026-09-23.dump --database-url URL
  ./scripts/restore.py s3://bucket/daily/2026-09-23.dump --s3-region R --database-url URL
  ./scripts/restore.py backups/wordfall-2026-09-23.dump --project wordfall --replace
  ./scripts/restore.py --bump-only --database-url URL     # after an RDS point-in-time restore

Restore into a **new** instance, never over the live one: the target must hold
no Wordfall schema, except with --replace on a local stack (--project), which
drops it first — the "snapshot before a risky migration edit" case.

The step that is easy to forget and impossible to skip always runs: every
user's `sync_seq` and `sync_floor_seq` become `sync_seq + 2^32`. Sequences went
backwards with the restore, so a device that synced after the restore point
holds a cursor at or ahead of the server's; with every cursor now below the
floor, each device's next sync answers `resync_required` and it rebuilds from
the server, its unsent operations pushed first in that same request.

Then point a stack at it — `./scripts/dev.py --env DATABASE_URL=…` — and check
/health (it stays not-ready until every catalog index is built from the rows)
and a few cascades before any cutover.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Optional

ROOT = Path(__file__).resolve().parent.parent
BUMP = 2 ** 32
BUMP_SQL = (
    f"UPDATE users SET sync_seq = sync_seq + {BUMP}, sync_floor_seq = sync_seq + {BUMP}"
)


def compose(project: str, *args: str) -> List[str]:
    return ["docker", "compose", "-p", project, "-f", str(ROOT / "docker-compose.yml"), *args]


def psql(args: argparse.Namespace, sql: str) -> str:
    if args.project:
        cmd = compose(args.project, "exec", "-T", "postgres", "psql", "-U", "wordfall", "-d", "wordfall")
    else:
        cmd = ["psql", "--dbname", args.database_url]
    out = subprocess.run([*cmd, "-v", "ON_ERROR_STOP=1", "-At", "-c", sql],
                         check=True, text=True, stdout=subprocess.PIPE)
    return out.stdout.strip()


def fetch(source: str, region: Optional[str], into: Path) -> Path:
    if not source.startswith("s3://"):
        return Path(source)
    import boto3

    bucket, _, key = source[len("s3://"):].partition("/")
    path = into / Path(key).name
    boto3.client("s3", region_name=region).download_file(bucket, key, str(path))
    return path


def restore(args: argparse.Namespace, dump: Path) -> None:
    has_schema = psql(args, "SELECT to_regclass('public.users') IS NOT NULL") == "t"
    if has_schema:
        if not (args.replace and args.project):
            sys.exit("restore: the target already holds a Wordfall schema; restore into a new "
                     "database (or, for a local stack, pass --project and --replace)")
        psql(args, "DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
    if args.project:
        with open(dump, "rb") as f:
            subprocess.run(compose(args.project, "exec", "-T", "postgres", "pg_restore", "-U", "wordfall",
                                   "-d", "wordfall", "--no-owner", "--no-privileges", "--exit-on-error"),
                           stdin=f, check=True)
    else:
        subprocess.run(["pg_restore", "--dbname", args.database_url, "--no-owner", "--no-privileges",
                        "--exit-on-error", str(dump)], check=True)
    print(f"restore: {dump} restored")


def bump(args: argparse.Namespace) -> None:
    users = psql(args, "SELECT count(*) FROM users")
    psql(args, BUMP_SQL)
    print(f"restore: sync_seq and sync_floor_seq raised by 2^32 for {users} users; "
          "every device resyncs at its next sync")


def main(argv: Optional[List[str]] = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("dump", nargs="?", help="a .dump file, or s3://bucket/key")
    p.add_argument("--s3-region")
    tgt = p.add_mutually_exclusive_group(required=True)
    tgt.add_argument("--database-url", help="the new database to restore into")
    tgt.add_argument("--project", help="a local compose stack to restore into")
    p.add_argument("--replace", action="store_true", help="with --project: drop the stack's schema first")
    p.add_argument("--bump-only", action="store_true",
                   help="only the resync step, e.g. after an RDS point-in-time restore")
    args = p.parse_args(argv)
    if args.bump_only:
        bump(args)
        return 0
    if not args.dump:
        p.error("name a dump, or pass --bump-only")
    with tempfile.TemporaryDirectory() as tmp:
        restore(args, fetch(args.dump, args.s3_region, Path(tmp)))
    bump(args)
    if args.project:
        subprocess.run(compose(args.project, "restart", "backend"), check=True)
        print("restore: backend restarted; /health turns ready once the catalog is indexed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
