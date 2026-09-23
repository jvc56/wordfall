"""The one way to bring a Wordfall up (PLAN.md § Development → How it is put together).

Four entry points, each usable on its own:

- ``up(project, port, env, build)``: compose up, then wait for ``/health``.
- ``seed(base_url, catalog, user)``: create the confirmed admin user through
  registration and confirmation, then upload each catalog item through the
  admin API, skipping what already exists.
- ``reset(project)``: drop and recreate the public schema, restart the backend.
- ``down(project, volumes)``: stop the stack.

Everything that needs a running Wordfall goes through these: ``dev.py``, the
end-to-end tests, the scale tests and CI.
"""

from __future__ import annotations

import json
import os
import re
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional

ROOT = Path(__file__).resolve().parent.parent
STACK_DIR = ROOT / ".stack"
FIXTURE_CATALOG = ROOT / "fixtures" / "catalog"
DEFAULT_PROJECT = "wordfall"
DEFAULT_PORT = 5173
HEALTH_DEADLINE_SECONDS = 900


@dataclass
class User:
    username: str = "dev"
    email: str = "dev@wordfall.local"
    password: str = "correct-tile-rack-bingo"


@dataclass
class CatalogItem:
    """One upload. ``kind`` is distribution, lexicon or leaves."""

    kind: str
    name: str
    path: Path
    parent: Optional[str] = None  # a lexicon's distribution, a leave set's lexicon


@dataclass
class Catalog:
    items: List[CatalogItem] = field(default_factory=list)


def log(msg: str) -> None:
    print(f"[stack] {msg}", file=sys.stderr, flush=True)


def _compose(project: str, *args: str, env: Optional[Dict[str, str]] = None,
             capture: bool = False, check: bool = True) -> subprocess.CompletedProcess:
    cmd = ["docker", "compose", "-p", project, "-f", str(ROOT / "docker-compose.yml"), *args]
    full_env = dict(os.environ)
    full_env["WORDFALL_ENV_FILE"] = str(_env_file(project))
    if env:
        full_env.update(env)
    return subprocess.run(cmd, cwd=ROOT, env=full_env, check=check, text=True,
                          stdout=subprocess.PIPE if capture else None,
                          stderr=subprocess.STDOUT if capture else None)


def _env_file(project: str) -> Path:
    return STACK_DIR / f"{project}.env"


def _signing_key(project: str) -> str:
    """A per-project session key, generated once and kept, so restarts keep sessions."""
    STACK_DIR.mkdir(exist_ok=True)
    path = STACK_DIR / f"{project}.key"
    if not path.exists():
        path.write_text(secrets.token_hex(32))
    return path.read_text().strip()


def _write_env(project: str, port: int, env: Dict[str, str]) -> None:
    values = {
        "SESSION_SIGNING_KEY": _signing_key(project),
        "PUBLIC_URL": f"http://localhost:{port}",
    }
    values.update(env)
    STACK_DIR.mkdir(exist_ok=True)
    _env_file(project).write_text("".join(f"{k}={v}\n" for k, v in values.items()))


def app_build() -> str:
    """The frontend build number the sync request carries: the commit count."""
    try:
        out = subprocess.run(["git", "rev-list", "--count", "HEAD"], cwd=ROOT,
                             capture_output=True, text=True, check=True)
        return out.stdout.strip() or "0"
    except (OSError, subprocess.CalledProcessError):
        return "0"


def up(project: str = DEFAULT_PROJECT, port: int = DEFAULT_PORT,
       env: Optional[Dict[str, str]] = None, build: bool = True,
       profiles: Optional[List[str]] = None) -> str:
    """Compose up, then wait for ``/health``. Returns the base URL."""
    _write_env(project, port, env or {})
    compose_env = {"WORDFALL_PORT": str(port), "WORDFALL_APP_BUILD": app_build()}
    args = []
    for p in profiles or []:
        args += ["--profile", p]
    args += ["up", "-d", "--remove-orphans"]
    if build:
        args.append("--build")
    log(f"bringing up project {project} on :{port}")
    _compose(project, *args, env=compose_env)
    base_url = f"http://localhost:{port}"
    wait_healthy(project, base_url)
    return base_url


def wait_healthy(project: str, base_url: str,
                 deadline_s: float = HEALTH_DEADLINE_SECONDS) -> None:
    deadline = time.monotonic() + deadline_s
    last = ""
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(f"{base_url}/health", timeout=5) as resp:
                if resp.status == 200:
                    log(f"healthy at {base_url}")
                    return
        except urllib.error.HTTPError as e:
            last = f"HTTP {e.code}"
        except (urllib.error.URLError, ConnectionError, OSError) as e:
            last = str(e)
        time.sleep(1)
    logs = _compose(project, "logs", "--no-color", "--tail", "200", "backend",
                    capture=True, check=False).stdout
    raise RuntimeError(f"{base_url}/health not ready before the deadline ({last}).\n"
                       f"Backend logs:\n{logs}")


def down(project: str = DEFAULT_PROJECT, volumes: bool = False) -> None:
    args = ["--profile", "hot-reload", "down", "--remove-orphans"]
    if volumes:
        args.append("--volumes")
    _compose(project, *args)


def psql(project: str, sql: str) -> str:
    out = _compose(project, "exec", "-T", "postgres", "psql", "-v", "ON_ERROR_STOP=1",
                   "-U", "wordfall", "-d", "wordfall", "-At", "-c", sql, capture=True)
    return out.stdout


def reset(project: str = DEFAULT_PROJECT) -> None:
    """``DROP SCHEMA public CASCADE; CREATE SCHEMA public;`` and restart the backend."""
    psql(project, "DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
    _compose(project, "restart", "backend")


# ---------------------------------------------------------------------------
# Seeding
# ---------------------------------------------------------------------------


class Http:
    """A tiny cookie-keeping JSON client that honours 429 and Retry-After."""

    def __init__(self, base_url: str):
        self.base_url = base_url
        self.cookies: Dict[str, str] = {}
        self.user_id: Optional[str] = None

    def _headers(self, extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
        h = {}
        if self.cookies:
            h["Cookie"] = "; ".join(f"{k}={v}" for k, v in self.cookies.items())
        csrf = self.cookies.get("wordfall_csrf")
        if csrf:
            h["X-CSRF-Token"] = csrf
        if self.user_id:
            h["X-Wordfall-User"] = self.user_id
        h.update(extra or {})
        return h

    def request(self, method: str, path: str, body: Optional[bytes] = None,
                content_type: Optional[str] = None, deadline_s: float = 600):
        deadline = time.monotonic() + deadline_s
        while True:
            headers = self._headers({"Content-Type": content_type} if content_type else None)
            req = urllib.request.Request(self.base_url + path, data=body, method=method,
                                         headers=headers)
            try:
                with urllib.request.urlopen(req, timeout=180) as resp:
                    self._keep_cookies(resp.headers.get_all("Set-Cookie") or [])
                    data = resp.read()
                    return resp.status, (json.loads(data) if data else None)
            except urllib.error.HTTPError as e:
                self._keep_cookies(e.headers.get_all("Set-Cookie") or [])
                data = e.read()
                if e.code == 429 and time.monotonic() < deadline:
                    wait = float(e.headers.get("Retry-After") or 1)
                    log(f"429 on {path}; waiting {wait:.0f}s")
                    time.sleep(wait)
                    continue
                try:
                    parsed = json.loads(data) if data else None
                except ValueError:
                    parsed = data.decode(errors="replace")
                return e.code, parsed

    def json(self, method: str, path: str, payload=None):
        body = json.dumps(payload).encode() if payload is not None else None
        return self.request(method, path, body, "application/json" if body is not None else None)

    def multipart(self, path: str, fields: Dict[str, str], file_field: str, file_path: Path):
        boundary = uuid.uuid4().hex
        parts = []
        for k, v in fields.items():
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n'
                         f"{v}\r\n".encode())
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{file_field}"; '
                     f'filename="{file_path.name}"\r\nContent-Type: application/octet-stream'
                     f"\r\n\r\n".encode() + file_path.read_bytes() + b"\r\n")
        parts.append(f"--{boundary}--\r\n".encode())
        return self.request("POST", path, b"".join(parts),
                            f"multipart/form-data; boundary={boundary}")

    def _keep_cookies(self, set_cookies: List[str]) -> None:
        for sc in set_cookies:
            name, _, rest = sc.partition("=")
            value = rest.split(";", 1)[0]
            if "max-age=0" in sc.lower() or value == "":
                self.cookies.pop(name.strip(), None)
            else:
                self.cookies[name.strip()] = value


def fixture_catalog() -> Catalog:
    """The committed fixture catalog (PLAN.md § The fixture catalog)."""
    manifest = FIXTURE_CATALOG / "manifest.json"
    if not manifest.exists():
        return Catalog()
    items = []
    for entry in json.loads(manifest.read_text()):
        items.append(CatalogItem(kind=entry["kind"], name=entry["name"],
                                 path=FIXTURE_CATALOG / entry["file"],
                                 parent=entry.get("parent")))
    return Catalog(items)


def _confirmation_code(project: str, email: str, deadline_s: float = 30) -> str:
    """Read the newest confirmation code for ``email`` from the console mail log."""
    deadline = time.monotonic() + deadline_s
    pattern = re.compile(r'"mail_to":"' + re.escape(email) + r'".*?"confirmation_code":"([^"]+)"')
    while time.monotonic() < deadline:
        logs = _compose(project, "logs", "--no-color", "backend", capture=True).stdout
        found = pattern.findall(logs)
        if found:
            return found[-1]
        time.sleep(0.5)
    raise RuntimeError(f"no confirmation code for {email} in the backend's console mail log")


def seed(base_url: str, catalog: Optional[Catalog] = None, user: Optional[User] = None,
         project: str = DEFAULT_PROJECT) -> Http:
    """Create the confirmed admin user through the real registration path, then
    upload each catalog item through the admin API, skipping what exists."""
    user = user or User()
    catalog = catalog if catalog is not None else fixture_catalog()
    http = Http(base_url)

    status, body = http.json("POST", "/api/auth/login",
                             {"username": user.username, "password": user.password})
    if status != 200:
        status, body = http.json("POST", "/api/auth/register", {
            "username": user.username, "email": user.email, "password": user.password})
        if status not in (200, 201, 202):
            raise RuntimeError(f"registration failed: {status} {body}")
        code = _confirmation_code(project, user.email)
        status, body = http.json("POST", "/api/auth/confirm-email", {"code": code})
        if status not in (200, 204):
            raise RuntimeError(f"confirmation failed: {status} {body}")
        status, body = http.json("POST", "/api/auth/login",
                                 {"username": user.username, "password": user.password})
        if status != 200:
            raise RuntimeError(f"login failed: {status} {body}")
    # No endpoint can grant admin (PLAN.md § Admin).
    psql(project, f"UPDATE users SET is_admin = true WHERE username = '{user.username}'")
    status, me = http.json("GET", "/api/auth/me")
    if status != 200:
        raise RuntimeError(f"/api/auth/me failed: {status} {me}")
    http.user_id = me["user_id"]
    log(f"user {user.username} ready (password {user.password})")

    if catalog.items:
        _upload_catalog(http, catalog)
    return http


def _upload_catalog(http: Http, catalog: Catalog) -> None:
    status, existing = http.json("GET", "/api/admin/catalog")
    if status != 200:
        raise RuntimeError(f"/api/admin/catalog failed: {status} {existing}")
    have_dists = {d["name"] for d in existing.get("letter_distributions", [])}
    have_lexicons = {x["name"] for x in existing.get("lexicons", [])}
    have_leaves = {x["lexicon"] for x in existing.get("leave_sets", [])}
    for item in catalog.items:
        if item.kind == "distribution":
            if item.name in have_dists:
                continue
            status, body = http.multipart("/api/admin/letter-distributions",
                                          {"name": item.name}, "file", item.path)
        elif item.kind == "lexicon":
            if item.name in have_lexicons:
                continue
            status, body = http.multipart("/api/admin/lexicons",
                                          {"name": item.name, "letter_distribution": item.parent},
                                          "file", item.path)
        elif item.kind == "leaves":
            if item.name in have_leaves:
                continue
            status, body = http.multipart("/api/admin/leave-sets", {"lexicon": item.name},
                                          "file", item.path)
        else:
            raise ValueError(f"unknown catalog kind {item.kind}")
        if status != 201:
            raise RuntimeError(f"upload of {item.kind} {item.name} failed: {status} {body}")
        log(f"uploaded {item.kind} {item.name}")


# ---------------------------------------------------------------------------
# Command line, so non-Python callers (Playwright's globalSetup, the Makefile)
# reach the same four entry points.
# ---------------------------------------------------------------------------


def _main(argv: Optional[List[str]] = None) -> int:
    import argparse

    p = argparse.ArgumentParser(prog="stack.py")
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("up", "seed", "reset", "down"):
        s = sub.add_parser(name)
        s.add_argument("--project", default=DEFAULT_PROJECT)
        s.add_argument("--port", type=int, default=DEFAULT_PORT)
        s.add_argument("--env", action="append", default=[], metavar="KEY=VALUE")
        s.add_argument("--no-build", action="store_true")
        s.add_argument("--volumes", action="store_true")
        s.add_argument("--base-url")
    args = p.parse_args(argv)
    if args.cmd == "up":
        env = dict(e.split("=", 1) for e in args.env)
        print(up(args.project, args.port, env, build=not args.no_build))
    elif args.cmd == "seed":
        base_url = args.base_url or f"http://localhost:{args.port}"
        seed(base_url, fixture_catalog(), User(), project=args.project)
    elif args.cmd == "reset":
        reset(args.project)
    elif args.cmd == "down":
        down(args.project, volumes=args.volumes)
    return 0


if __name__ == "__main__":
    sys.exit(_main())
