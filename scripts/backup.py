#!/usr/bin/env python3
"""Dump a Wordfall database (PLAN.md § Backups).

A `pg_dump --format=custom` of the whole database, and a `--schema-only`
dump beside it. The same code runs nightly in production, as a scheduled
Fargate task on the backend image, and locally before a risky migration edit.

  ./scripts/backup.py                          # the dev stack, into ./backups/
  ./scripts/backup.py --project wordfall-e2e   # another local stack
  ./scripts/backup.py --database-url URL --out DIR
  backup.py --s3-bucket B --s3-region R --metrics-region R2   # production

With --s3-bucket the dump streams to `daily/YYYY-MM-DD.dump` (and
`.schema.sql`), named for the day so a bad run cannot overwrite a good one;
on the first of the month it is also written to `monthly/YYYY-MM.dump`, kept a
year. With --metrics-region it reports `Wordfall/Backups` DumpSucceeded,
DumpBytes and DumpSizeRatio (this dump over the previous one), which the
alarms in infra/alarms.tf watch. The database URL comes from --database-url
or DATABASE_URL; production gives it a read-only role.
"""

from __future__ import annotations

import argparse
import datetime as dt
import os
import subprocess
import sys
from pathlib import Path
from typing import IO, List, Optional

ROOT = Path(__file__).resolve().parent.parent
MONTHLY_RETENTION_DAYS = 366


def dump_command(args: argparse.Namespace, schema_only: bool) -> List[str]:
    flags = ["--schema-only"] if schema_only else ["--format=custom"]
    if args.project:
        # The stack's own pg_dump, so its version matches the server's.
        return ["docker", "compose", "-p", args.project, "-f", str(ROOT / "docker-compose.yml"),
                "exec", "-T", "postgres", "pg_dump", "-U", "wordfall", "-d", "wordfall", *flags]
    url = args.database_url or os.environ.get("DATABASE_URL")
    if not url:
        sys.exit("backup: no database: pass --database-url, set DATABASE_URL, or name a --project")
    return ["pg_dump", "--dbname", url, *flags]


class Counting:
    """A readable stream that counts what passes through it."""

    def __init__(self, raw: IO[bytes]):
        self.raw = raw
        self.bytes = 0

    def read(self, n: int = -1) -> bytes:
        b = self.raw.read(n)
        self.bytes += len(b)
        return b


def run_dump(args: argparse.Namespace, schema_only: bool, sink) -> int:
    """Runs pg_dump into `sink(stream)`; returns the bytes written."""
    proc = subprocess.Popen(dump_command(args, schema_only), stdout=subprocess.PIPE)
    assert proc.stdout is not None
    counting = Counting(proc.stdout)
    sink(counting)
    if proc.wait() != 0:
        raise RuntimeError(f"pg_dump exited with {proc.returncode}")
    if counting.bytes == 0:
        raise RuntimeError("pg_dump wrote nothing")
    return counting.bytes


def to_directory(args: argparse.Namespace, day: dt.date) -> int:
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    stem = out / f"wordfall-{day.isoformat()}"

    def writer(path: Path):
        def sink(stream):
            tmp = path.with_suffix(path.suffix + ".partial")
            with open(tmp, "wb") as f:
                while True:
                    chunk = stream.read(1 << 20)
                    if not chunk:
                        break
                    f.write(chunk)
            tmp.replace(path)
        return sink

    size = run_dump(args, False, writer(stem.with_suffix(".dump")))
    run_dump(args, True, writer(stem.with_suffix(".schema.sql")))
    print(f"backup: {stem}.dump ({size} bytes) and {stem}.schema.sql")
    return size


def previous_size(s3, bucket: str, key: str) -> Optional[int]:
    """The size of the latest daily dump before `key`."""
    best = None
    for page in s3.get_paginator("list_objects_v2").paginate(Bucket=bucket, Prefix="daily/"):
        for o in page.get("Contents", []):
            k = o["Key"]
            if k.endswith(".dump") and k < key and (best is None or k > best[0]):
                best = (k, o["Size"])
    return best[1] if best else None


def to_s3(args: argparse.Namespace, day: dt.date) -> int:
    import boto3  # the backend image carries python3-boto3

    s3 = boto3.client("s3", region_name=args.s3_region)
    key = f"daily/{day.isoformat()}.dump"
    before = previous_size(s3, args.s3_bucket, key)

    def uploader(k: str):
        return lambda stream: s3.upload_fileobj(stream, args.s3_bucket, k)

    size = run_dump(args, False, uploader(key))
    run_dump(args, True, uploader(f"daily/{day.isoformat()}.schema.sql"))
    if day.day == 1:
        until = dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=MONTHLY_RETENTION_DAYS)
        for suffix in (".dump", ".schema.sql"):
            s3.copy_object(
                Bucket=args.s3_bucket,
                Key=f"monthly/{day.strftime('%Y-%m')}{suffix}",
                CopySource={"Bucket": args.s3_bucket, "Key": f"daily/{day.isoformat()}{suffix}"},
                ObjectLockMode="GOVERNANCE",
                ObjectLockRetainUntilDate=until,
            )
    print(f"backup: s3://{args.s3_bucket}/{key} ({size} bytes; previous {before})")
    if args.metrics_region:
        cw = boto3.client("cloudwatch", region_name=args.metrics_region)
        data = [
            {"MetricName": "DumpSucceeded", "Value": 1, "Unit": "Count"},
            {"MetricName": "DumpBytes", "Value": size, "Unit": "Bytes"},
        ]
        if before:
            data.append({"MetricName": "DumpSizeRatio", "Value": size / before, "Unit": "None"})
        cw.put_metric_data(Namespace="Wordfall/Backups", MetricData=data)
    return size


def main(argv: Optional[List[str]] = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    src = p.add_mutually_exclusive_group()
    src.add_argument("--database-url", help="the database to dump (default: DATABASE_URL)")
    src.add_argument("--project", help="a local compose stack to dump (default: wordfall, the dev stack)")
    p.add_argument("--out", default=str(ROOT / "backups"), help="directory for the files (default: ./backups)")
    p.add_argument("--s3-bucket", help="write to this bucket instead of a directory")
    p.add_argument("--s3-region")
    p.add_argument("--metrics-region", help="report Wordfall/Backups metrics to CloudWatch in this region")
    args = p.parse_args(argv)
    if not args.database_url and not args.project and not os.environ.get("DATABASE_URL"):
        args.project = "wordfall"
    day = dt.datetime.now(dt.timezone.utc).date()
    if args.s3_bucket:
        to_s3(args, day)
    else:
        to_directory(args, day)
    return 0


if __name__ == "__main__":
    sys.exit(main())
