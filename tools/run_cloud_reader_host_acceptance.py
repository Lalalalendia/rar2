#!/usr/bin/env python3
"""Physical Cloud Reader host acceptance for CLOUD-READER-SERVER7-HOST-ACCEPTANCE-01.

Runs on the deployed Linux host. It composes existing Chaptera/systemd/Nginx
authorities and emits a source-safe receipt. It never deploys, migrates, resets
the database, or prints guest credentials.

A closure-grade run uses both --exercise-restart and --exercise-rollback.
Rollback is attempted only when the exact previous binary reports the current
database schema as its own current target.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

PROTOCOL = "chaptera.cloud-reader-host-acceptance.v1"
GUEST_PROTOCOL = "chaptera.reader-guest-session.v1"
COMPAT_PROTOCOL = "chaptera.reader-compatibility-report.v1"
DEFAULT_CHAPTERA = pathlib.Path("/opt/chaptera/current/chaptera")
DEFAULT_CONFIG = pathlib.Path("/etc/chaptera/chaptera.toml")
CHAPTERA_ROOT = pathlib.Path("/opt/chaptera")
# One public Reader service/cgroup; do not restart legacy Product web+worker units.
SERVICES = ("chaptera-reader.service",)
STATE_BY_CLASSIFICATION = {
    "supported": "opens_normally",
    "partial": "needs_review",
    "salvage": "opens_with_salvage",
    "unsupported": "unsupported",
}


class AcceptanceError(RuntimeError):
    pass


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def run(command: list[str], *, timeout: int = 60, check: bool = True) -> subprocess.CompletedProcess[str]:
    try:
        completed = subprocess.run(
            command,
            text=True,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        # Never stringify a subprocess exception: it can contain arguments,
        # stdout, stderr, source filenames, tokens or other host-only values.
        raise AcceptanceError("host acceptance subprocess timeout") from None
    if check and completed.returncode != 0:
        # Do not copy arbitrary host command stderr into CI receipts/logs.
        raise AcceptanceError(
            f"host acceptance subprocess failed rc={completed.returncode}"
        )
    return completed


def read_json_command(command: list[str], *, timeout: int = 60) -> dict[str, Any]:
    completed = run(command, timeout=timeout)
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise AcceptanceError(f"{command[0]} did not emit JSON") from exc
    if not isinstance(value, dict):
        raise AcceptanceError(f"{command[0]} emitted non-object JSON")
    return value


def safe_release_target(link: pathlib.Path) -> str:
    if not link.is_symlink():
        raise AcceptanceError(f"{link} is not a symlink")
    target = os.readlink(link)
    # Current immutable installer writes an absolute release symlink. The
    # older relative release format remains valid for historical packets.
    releases = CHAPTERA_ROOT / "releases"
    absolute = re.escape(str(releases)) + r"/[0-9a-f]{40}"
    if not (re.fullmatch(r"releases/[0-9a-f]{40}", target)
            or re.fullmatch(absolute, target)):
        raise AcceptanceError(f"unsafe Chaptera release target at {link}")
    resolved = (link.parent / target).resolve()
    if resolved.parent != releases.resolve() or not resolved.is_dir():
        raise AcceptanceError(f"release target is outside immutable release root: {link}")
    return target


def binary_for_target(target: str) -> pathlib.Path:
    path = CHAPTERA_ROOT / target / "chaptera"
    if not path.is_file():
        raise AcceptanceError(f"release binary missing: {target}")
    return path


def service_active(name: str) -> bool:
    return run(["systemctl", "is-active", "--quiet", name], check=False).returncode == 0


def service_metric(name: str, prop: str) -> int | None:
    raw = run(["systemctl", "show", name, "-p", prop, "--value"], check=False).stdout.strip()
    if not raw or raw in {"[not set]", "infinity"}:
        return None
    try:
        return int(raw)
    except ValueError:
        return None


def service_snapshot() -> dict[str, Any]:
    return {
        name: {
            "active": service_active(name),
            "memory_current_bytes": service_metric(name, "MemoryCurrent"),
            "tasks_current": service_metric(name, "TasksCurrent"),
        }
        for name in SERVICES
    }


def assert_services_active(snapshot: dict[str, Any]) -> None:
    inactive = [name for name, row in snapshot.items() if not row["active"]]
    if inactive:
        raise AcceptanceError("inactive services: " + ", ".join(inactive))


def loopback_listener_receipt(port: int) -> dict[str, Any]:
    completed = run(["ss", "-H", "-ltn"])
    listeners: list[str] = []
    for line in completed.stdout.splitlines():
        parts = line.split()
        if len(parts) < 4:
            continue
        local = parts[3]
        if local.endswith(f":{port}"):
            listeners.append(local)
    if not listeners:
        raise AcceptanceError(f"no listening TCP socket found for port {port}")
    unsafe = [
        item for item in listeners
        if not (
            item.startswith("127.0.0.1:")
            or item.startswith("[::1]:")
            or item.startswith("::1:")
        )
    ]
    if unsafe:
        raise AcceptanceError("Chaptera listener is not loopback-only")
    return {"port": port, "listener_count": len(listeners), "loopback_only": True}


def request_json(
    origin: str,
    path: str,
    *,
    method: str = "GET",
    body: bytes | None = None,
    headers: dict[str, str] | None = None,
    timeout: int = 60,
) -> tuple[int, dict[str, str], dict[str, Any]]:
    target = urllib.parse.urljoin(origin.rstrip("/") + "/", path.lstrip("/"))
    request = urllib.request.Request(
        target,
        data=body,
        method=method,
        headers=headers or {},
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
            status = response.status
            response_headers = {k.lower(): v for k, v in response.headers.items()}
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        status = exc.code
        response_headers = {k.lower(): v for k, v in exc.headers.items()}
    try:
        payload = json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        payload = {}
    if not isinstance(payload, dict):
        payload = {}
    return status, response_headers, payload


def request_bytes(
    origin: str,
    path: str,
    *,
    timeout: int = 30,
) -> tuple[int, dict[str, str], bytes]:
    target = urllib.parse.urljoin(origin.rstrip("/") + "/", path.lstrip("/"))
    request = urllib.request.Request(target, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return (
                response.status,
                {k.lower(): v for k, v in response.headers.items()},
                response.read(),
            )
    except urllib.error.HTTPError as exc:
        return exc.code, {k.lower(): v for k, v in exc.headers.items()}, exc.read()


def local_health(port: int) -> dict[str, Any]:
    origin = f"http://127.0.0.1:{port}"
    rows: dict[str, Any] = {}
    for path in ("/live", "/ready"):
        status, _headers, payload = request_json(origin, path, timeout=10)
        if status != 200:
            raise AcceptanceError(f"loopback {path} returned {status}")
        rows[path[1:]] = {
            "status": status,
            "ready": payload.get("ready") if isinstance(payload.get("ready"), bool) else None,
        }
    return rows


def edge_receipt(origin: str) -> dict[str, Any]:
    parsed = urllib.parse.urlparse(origin)
    if parsed.scheme != "https" or not parsed.netloc or parsed.path not in {"", "/"}:
        raise AcceptanceError("--origin must be a bare HTTPS origin")
    status, headers, body = request_bytes(origin, "/")
    if status != 200 or b"Chaptera" not in body or b"Cloud Reader" not in body:
        raise AcceptanceError("public Reader surface failed")
    required = {
        "strict-transport-security": "max-age=",
        "x-content-type-options": "nosniff",
        "referrer-policy": "no-referrer",
        "content-security-policy": "default-src",
    }
    for name, fragment in required.items():
        if fragment.lower() not in headers.get(name, "").lower():
            raise AcceptanceError(f"public edge missing required {name}")
    if "server" in headers:
        raise AcceptanceError("public edge leaked Server header")
    private_statuses = {}
    for path in ("/live", "/ready", "/v1/auth/me"):
        private_statuses[path] = request_bytes(origin, path)[0]
        if private_statuses[path] != 404:
            raise AcceptanceError(f"private path is publicly reachable: {path}")
    return {
        "https": True,
        "surface_status": status,
        "security_headers": {name: True for name in required},
        "server_header_absent": True,
        "private_paths_rejected": private_statuses,
    }


def trace_headers() -> dict[str, str]:
    opaque = hashlib.sha256(os.urandom(32)).hexdigest()[:24]
    return {
        "x-chaptera-trace-version": "chaptera.trace-context.v1",
        "x-chaptera-trace-id": f"trace:host-acceptance-{opaque}",
        "x-chaptera-interaction-id": f"interaction:host-acceptance-{opaque}",
        "x-chaptera-session-incarnation": f"session:host-acceptance-{opaque}",
        "x-chaptera-operation-class": "open",
        "x-chaptera-browser-family": "other",
    }


def guest_mutation_headers(origin: str, csrf: str) -> dict[str, str]:
    # EdgePolicy rejects every POST/PUT/PATCH/DELETE without a same-origin
    # Origin header, even for this source-neutral operator witness.
    return {"origin": origin, "x-csrf-token": csrf, **trace_headers()}


def guest_probe(origin: str, fixture: bytes, expected_sha256: str, *, timeout: int, witness: dict[str, Any] | None = None) -> dict[str, Any]:
    csrf = hashlib.sha256(os.urandom(32)).hexdigest()
    common = guest_mutation_headers(origin, csrf)

    t0 = time.monotonic()
    status, _headers, issued = request_json(
        origin,
        "/v1/reader/guest-sessions",
        method="POST",
        body=json.dumps({"expected_byte_len": len(fixture)}, separators=(",", ":")).encode(),
        headers={**common, "content-type": "application/json"},
        timeout=timeout,
    )
    create_ms = round((time.monotonic() - t0) * 1000, 3)
    if status != 200 or issued.get("protocol_version") != GUEST_PROTOCOL:
        raise AcceptanceError("guest session create failed")
    session_id = issued.get("session_id")
    token = issued.get("access_token")
    if not isinstance(session_id, str) or not session_id or not isinstance(token, str) or not token:
        raise AcceptanceError("guest session identity/token missing")

    expected_prefix = f"/v1/reader/guest-sessions/{session_id}/"
    upload_path = issued.get("upload_path")
    open_path = issued.get("open_path")
    if upload_path != expected_prefix + "content" or open_path != expected_prefix + "open":
        raise AcceptanceError("guest session returned unexpected paths")

    session_headers = {
        **common,
        "x-chaptera-reader-session": token,
    }

    t0 = time.monotonic()
    status, _headers, uploaded = request_json(
        origin,
        upload_path,
        method="PUT",
        body=fixture,
        headers={**session_headers, "content-type": "application/octet-stream"},
        timeout=timeout,
    )
    upload_ms = round((time.monotonic() - t0) * 1000, 3)
    if status != 200 or uploaded.get("session_id") != session_id:
        raise AcceptanceError("guest upload failed")

    t0 = time.monotonic()
    status, _headers, opened = request_json(
        origin,
        open_path,
        method="POST",
        body=b"{}",
        headers={**session_headers, "content-type": "application/json"},
        timeout=timeout,
    )
    open_ms = round((time.monotonic() - t0) * 1000, 3)
    if status != 200 or opened.get("session_id") != session_id:
        raise AcceptanceError("guest open failed")

    classification = opened.get("classification")
    if classification not in STATE_BY_CLASSIFICATION:
        raise AcceptanceError(f"unexpected guest classification: {classification!r}")
    server_sha = opened.get("source_sha256")
    if server_sha != expected_sha256:
        raise AcceptanceError("server source SHA-256 differs from fixture")

    report = opened.get("compatibility_report")
    if not isinstance(report, dict) or report.get("protocol_version") != COMPAT_PROTOCOL:
        raise AcceptanceError("compatibility report missing")
    if report.get("source_sha256") != expected_sha256:
        raise AcceptanceError("compatibility report source identity mismatch")
    if report.get("state") != STATE_BY_CLASSIFICATION[classification]:
        raise AcceptanceError("compatibility report state/classification mismatch")

    scene_path = expected_prefix + "scene"
    t0 = time.monotonic()
    status, _headers, scene_response = request_json(
        origin,
        scene_path,
        headers=session_headers,
        timeout=timeout,
    )
    scene_ms = round((time.monotonic() - t0) * 1000, 3)
    if status != 200 or scene_response.get("session_id") != session_id:
        raise AcceptanceError("saved scene read failed")
    if scene_response.get("classification") != classification:
        raise AcceptanceError("saved scene classification changed")
    saved_report = scene_response.get("compatibility_report")
    if not isinstance(saved_report, dict) or saved_report.get("state") != report.get("state"):
        raise AcceptanceError("saved compatibility report changed")

    scene = opened.get("scene")
    salvage = opened.get("salvage")
    if classification in {"supported", "partial"}:
        if not isinstance(scene, dict) or salvage is not None:
            raise AcceptanceError("supported/partial open has invalid Scene shape")
        page_count = len(scene.get("pages", [])) if isinstance(scene.get("pages"), list) else None
    elif classification == "salvage":
        if not isinstance(salvage, dict) or scene is not None:
            raise AcceptanceError("salvage open has invalid observation shape")
        page_count = None
    else:
        page_count = None

    # Only the explicitly requested operator-side witness retains an
    # ephemeral identity. Never insert it into a source-neutral receipt.
    if witness is not None:
        witness.update({
            "session_id": session_id,
            "token": token,
            "expires_at_ms": issued.get("expires_at_ms"),
        })
    return {
        "classification": classification,
        "compatibility_state": report.get("state"),
        "page_count": page_count,
        "durations_ms": {
            "session_create": create_ms,
            "upload": upload_ms,
            "open": open_ms,
            "scene_read": scene_ms,
        },
    }


# This proof is opt-in and strictly read-only on private storage. The
# production service itself owns expiration, physical deletion and release.
GUEST_TENANT = "tenant:cloud-reader-guest-service"


def private_storage_config(path: pathlib.Path) -> pathlib.Path:
    with path.open("rb") as handle:
        config = tomllib.load(handle)
    if config.get("storage", {}).get("provider") != "filesystem":
        raise AcceptanceError("private expiry proof requires filesystem provider")
    sqlite_value = config.get("sqlite", {}).get("path")
    if not isinstance(sqlite_value, str) or not sqlite_value.startswith("/"):
        raise AcceptanceError("private expiry SQLite path is invalid")
    sqlite_path = pathlib.Path(sqlite_value)
    if not sqlite_path.is_file():
        raise AcceptanceError("private expiry SQLite database is missing")
    return sqlite_path


def private_storage_snapshot(sqlite_path: pathlib.Path, session_id: str) -> dict[str, bool]:
    # The exact session was issued by our in-memory public API witness, not
    # supplied via argv/logs. All binding identity checks fail closed.
    if not re.fullmatch(r"guest:[0-9a-f]{32}", session_id):
        raise AcceptanceError("private expiry witness session shape invalid")
    suffix = session_id.split(":", 1)[1]
    upload = f"guest-upload:{suffix}"
    reservation = f"guest-reservation:{suffix}"
    path = (sqlite_path.parent / "blob-store" / "quarantine" / GUEST_TENANT / upload)
    if path.is_symlink():
        raise AcceptanceError("private expiry quarantine path is a symlink")
    uri = "file:" + urllib.parse.quote(str(sqlite_path), safe="/") + "?mode=ro"
    conn = sqlite3.connect(uri, uri=True, timeout=5)
    try:
        conn.execute("PRAGMA query_only=ON")
        row = conn.execute(
            "SELECT upload_id, reservation_id FROM reader_guest_sessions WHERE session_id=?",
            (session_id.encode("ascii"),),
        ).fetchone()
        if row is not None and row != (upload.encode("ascii"), reservation.encode("ascii")):
            raise AcceptanceError("private expiry session binding mismatch")
        reservation_row = conn.execute(
            "SELECT released_at_ms FROM upload_admission_reservations WHERE reservation_id=?",
            (reservation.encode("ascii"),),
        ).fetchone()
    except sqlite3.Error:
        raise AcceptanceError("private expiry SQLite read failed") from None
    finally:
        conn.close()
    return {
        "session_present": row is not None,
        "quarantine_file_present": path.is_file(),
        "reservation_active": reservation_row is not None and reservation_row[0] is None,
        "reservation_record_present": reservation_row is not None,
    }


def private_expiry_receipt(
    origin: str, sqlite_path: pathlib.Path, witness: dict[str, Any], *, timeout: int,
) -> dict[str, Any]:
    session_id = witness.get("session_id")
    token = witness.get("token")
    expiry = witness.get("expires_at_ms")
    if not isinstance(session_id, str) or not isinstance(token, str) or type(expiry) is not int:
        raise AcceptanceError("private expiry witness is incomplete")
    before = private_storage_snapshot(sqlite_path, session_id)
    # Guest open already released its upload-admission lease. It must not
    # remain active while the source awaits its TTL.
    if not all((
        before["session_present"],
        before["quarantine_file_present"],
        not before["reservation_active"],
        before["reservation_record_present"],
    )):
        raise AcceptanceError("private expiry precondition not physically present")

    remaining_ms = expiry - int(time.time() * 1000)
    if not 0 <= remaining_ms <= 3_600_000:
        raise AcceptanceError("private expiry deadline outside bounded session TTL")
    time.sleep(remaining_ms / 1000.0 + 3)
    status, _headers, _body = request_json(
        origin,
        f"/v1/reader/guest-sessions/{session_id}/scene",
        headers={"x-chaptera-reader-session": token, **trace_headers()},
        timeout=timeout,
    )
    if status not in {404, 410}:
        raise AcceptanceError("private expiry access was not revoked")
    after = private_storage_snapshot(sqlite_path, session_id)
    verified = (
        not after["session_present"]
        and not after["quarantine_file_present"]
        and not after["reservation_active"]
    )
    # No session/upload/reservation identity, file path, token or body in output.
    return {
        "verified": verified,
        "expired_scene_denied": True,
        "sqlite_session_absent": not after["session_present"],
        "quarantine_file_absent": not after["quarantine_file_present"],
        "admission_reservation_inactive": not after["reservation_active"],
    }


def validate_live_doctor_receipt(payload: dict[str, Any]) -> dict[str, Any]:
    # Allowlist only source-neutral readiness booleans, never carry CLI output
    # (including paths, config, tokens or possible future debug fields).
    if not isinstance(payload, dict) or not all((
        payload.get("schema") == "chaptera.doctor-live.v1",
        payload.get("status") == "ready",
        payload.get("ready") is True,
        payload.get("release_match") is True,
        payload.get("loopback") is True,
    )):
        raise AcceptanceError("production live doctor did not verify readiness and SHA")
    return {"verified": True, "runtime_ready": True, "release_match": True}


def live_doctor_receipt(binary: pathlib.Path, config: pathlib.Path) -> dict[str, Any]:
    result = run(
        [str(binary), "--config", str(config), "doctor", "--live"],
        check=False, timeout=12,
    )
    if result.returncode != 0:
        raise AcceptanceError("production live doctor failed")
    try:
        data = json.loads(result.stdout)
    except (ValueError, TypeError):
        raise AcceptanceError("production live doctor receipt is not JSON") from None
    return validate_live_doctor_receipt(data)


def require_probe_equivalence(baseline: dict[str, Any], candidate: dict[str, Any], label: str) -> None:
    for key in ("classification", "compatibility_state", "page_count"):
        if candidate.get(key) != baseline.get(key):
            raise AcceptanceError(f"{label} changed reader {key}")


def concurrent_reader_receipt(
    origin: str,
    fixture: bytes,
    fixture_sha256: str,
    *,
    concurrency: int,
    timeout: int,
) -> dict[str, Any]:
    if not 1 <= concurrency <= 8:
        raise AcceptanceError("--concurrency must be between 1 and 8")
    started = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as executor:
        futures = [
            executor.submit(guest_probe, origin, fixture, fixture_sha256, timeout=timeout)
            for _ in range(concurrency)
        ]
        rows = [future.result() for future in futures]
    return {
        "concurrency": concurrency,
        "success_count": len(rows),
        "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
        "classifications": sorted(row["classification"] for row in rows),
    }


def migration_status(binary: pathlib.Path, config: pathlib.Path) -> dict[str, Any]:
    status = read_json_command(
        [str(binary), "--config", str(config), "migrate", "status"],
        timeout=30,
    )
    required = {"state", "current_version", "target_version", "rollback_previous_binary_safe"}
    if not required.issubset(status):
        raise AcceptanceError("migration status JSON is incomplete")
    return {
        "state": status["state"],
        "current_version": status["current_version"],
        "target_version": status["target_version"],
        "pending_count": len(status.get("pending_versions", [])),
        "rollback_previous_binary_safe": bool(status["rollback_previous_binary_safe"]),
    }


def exact_previous_schema_compatible(
    current_status: dict[str, Any],
    previous_binary: pathlib.Path,
    config: pathlib.Path,
) -> tuple[bool, dict[str, Any] | None]:
    completed = run(
        [str(previous_binary), "--config", str(config), "migrate", "status"],
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        return False, None
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError:
        return False, None
    if not isinstance(value, dict):
        return False, None
    safe = (
        value.get("state") == "current"
        and value.get("current_version") == current_status["current_version"]
        and value.get("target_version") == current_status["current_version"]
        and not value.get("pending_versions")
    )
    public = {
        "state": value.get("state"),
        "current_version": value.get("current_version"),
        "target_version": value.get("target_version"),
        "pending_count": len(value.get("pending_versions", []))
        if isinstance(value.get("pending_versions"), list)
        else None,
    }
    return safe, public


def atomic_switch_current(target: str) -> None:
    current = CHAPTERA_ROOT / "current"
    temp = CHAPTERA_ROOT / f".current.host-acceptance-{os.getpid()}"
    try:
        temp.unlink(missing_ok=True)
        os.symlink(target, temp)
        os.replace(temp, current)
        fd = os.open(CHAPTERA_ROOT, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        temp.unlink(missing_ok=True)


def restart_services() -> float:
    started = time.monotonic()
    run(["systemctl", "restart", *SERVICES], timeout=90)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        snapshot = service_snapshot()
        if all(row["active"] for row in snapshot.values()):
            return round((time.monotonic() - started) * 1000, 3)
        time.sleep(1)
    raise AcceptanceError("Chaptera services did not become active after restart")


def self_test() -> int:
    assert STATE_BY_CLASSIFICATION["partial"] == "needs_review"
    with tempfile.TemporaryDirectory() as raw:
        path = pathlib.Path(raw) / "fixture.pub"
        path.write_bytes(b"abc")
        assert sha256_file(path) == hashlib.sha256(b"abc").hexdigest()
    sample_current = {
        "current_version": 19,
        "target_version": 19,
        "state": "current",
        "pending_count": 0,
        "rollback_previous_binary_safe": False,
    }
    assert sample_current["state"] == "current"
    assert SERVICES == ("chaptera-reader.service",)
    good_doctor = {
        "schema": "chaptera.doctor-live.v1",
        "status": "ready", "ready": True, "release_match": True,
        "loopback": True,
    }
    assert validate_live_doctor_receipt(good_doctor)["verified"]
    for invalid in [
        {**good_doctor, "release_match": False},
        {**good_doctor, "ready": False},
        {**good_doctor, "status": "not_ready"},
    ]:
        try:
            validate_live_doctor_receipt(invalid)
        except AcceptanceError:
            pass
        else:
            raise AssertionError("unready doctor was admitted")
    with tempfile.TemporaryDirectory(prefix="chaptera-private-expiry-test-") as tmp:
        db = pathlib.Path(tmp) / "chaptera.sqlite"
        sid = "guest:" + "a" * 32
        upload = "guest-upload:" + "a" * 32
        reservation = "guest-reservation:" + "a" * 32
        with sqlite3.connect(db) as con:
            con.execute(
                "CREATE TABLE reader_guest_sessions (session_id BLOB, upload_id BLOB, reservation_id BLOB)"
            )
            con.execute(
                "CREATE TABLE upload_admission_reservations (reservation_id BLOB, released_at_ms INTEGER)"
            )
            con.execute(
                "INSERT INTO reader_guest_sessions VALUES (?,?,?)",
                (sid.encode(), upload.encode(), reservation.encode()),
            )
            con.execute(
                "INSERT INTO upload_admission_reservations VALUES (?,NULL)",
                (reservation.encode(),),
            )
        quarantine = db.parent / "blob-store" / "quarantine" / GUEST_TENANT / upload
        quarantine.parent.mkdir(parents=True)
        quarantine.write_bytes(b"synthetic source")
        # Production open_session releases admission BEFORE guest TTL.
        with sqlite3.connect(db) as con:
            con.execute(
                "UPDATE upload_admission_reservations SET released_at_ms=123 WHERE reservation_id=?",
                (reservation.encode(),),
            )
        pre = private_storage_snapshot(db, sid)
        assert pre["session_present"] and pre["quarantine_file_present"]
        assert not pre["reservation_active"] and pre["reservation_record_present"]
        with sqlite3.connect(db) as con:
            con.execute("DELETE FROM reader_guest_sessions WHERE session_id=?", (sid.encode(),))
        quarantine.unlink()
        post = private_storage_snapshot(db, sid)
        assert not post["session_present"] and not post["quarantine_file_present"]
        assert not post["reservation_active"] and post["reservation_record_present"]

    try:
        run([sys.executable, "-c",
             "import sys; sys.stderr.write('sensitive-test-sentinel'); sys.exit(7)"],
            timeout=5)
    except AcceptanceError as exc:
        assert "sensitive-test-sentinel" not in str(exc)
        assert "rc=7" in str(exc)
    else:
        raise AssertionError("failing subprocess was accepted")
    headers = guest_mutation_headers("https://reader.chaptera.online", "csrf-probe")
    assert headers["origin"] == "https://reader.chaptera.online"
    assert headers["x-csrf-token"] == "csrf-probe"
    global CHAPTERA_ROOT
    original_root = CHAPTERA_ROOT
    try:
        with tempfile.TemporaryDirectory(prefix="chaptera-host-acceptance-") as tmp:
            CHAPTERA_ROOT = pathlib.Path(tmp) / "chaptera"
            sha = "a" * 40
            release = CHAPTERA_ROOT / "releases" / sha
            release.mkdir(parents=True)
            (release / "chaptera").write_bytes(b"binary")
            current = CHAPTERA_ROOT / "current"
            current.symlink_to(release)
            assert safe_release_target(current) == str(release)
            relative = CHAPTERA_ROOT / "previous"
            relative.symlink_to(f"releases/{sha}")
            assert safe_release_target(relative) == f"releases/{sha}"
            bad = CHAPTERA_ROOT / "unsafe"
            bad.symlink_to("/tmp/foreign-release")
            try:
                safe_release_target(bad)
            except AcceptanceError:
                pass
            else:
                raise AssertionError("outside release target admitted")
            second = CHAPTERA_ROOT / "releases" / ("b" * 40)
            second.mkdir()
            atomic_switch_current(str(second))
            assert safe_release_target(current) == str(second)
            atomic_switch_current(str(release))
            assert safe_release_target(current) == str(release)
    finally:
        CHAPTERA_ROOT = original_root
    print("cloud reader host acceptance self-test: ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--origin")
    parser.add_argument("--fixture", type=pathlib.Path)
    parser.add_argument("--fixture-sha256")
    parser.add_argument("--expected-binary-sha256")
    parser.add_argument("--chaptera", type=pathlib.Path, default=DEFAULT_CHAPTERA)
    parser.add_argument("--config", type=pathlib.Path, default=DEFAULT_CONFIG)
    parser.add_argument("--app-port", type=int, default=8080)
    parser.add_argument("--concurrency", type=int, default=2)
    parser.add_argument("--timeout-seconds", type=int, default=120)
    parser.add_argument("--exercise-restart", action="store_true")
    parser.add_argument("--exercise-rollback", action="store_true")
    parser.add_argument("--verify-private-expiry", action="store_true")
    parser.add_argument("--out", type=pathlib.Path, default=pathlib.Path("target/cloud-reader-server7/receipt.json"))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    required = {
        "--origin": args.origin,
        "--fixture": args.fixture,
        "--fixture-sha256": args.fixture_sha256,
        "--expected-binary-sha256": args.expected_binary_sha256,
    }
    missing = [name for name, value in required.items() if not value]
    if missing:
        parser.error("required for host run: " + ", ".join(missing))
    if os.geteuid() != 0:
        raise AcceptanceError("physical host acceptance must run as root")
    if not args.config.is_file():
        raise AcceptanceError("Chaptera config is missing")
    if not args.chaptera.is_file():
        raise AcceptanceError("current Chaptera binary is missing")
    if not re.fullmatch(r"[0-9a-f]{64}", args.fixture_sha256):
        raise AcceptanceError("fixture SHA-256 must be lowercase hex")
    if not re.fullmatch(r"[0-9a-f]{64}", args.expected_binary_sha256):
        raise AcceptanceError("binary SHA-256 must be lowercase hex")

    fixture_sha = sha256_file(args.fixture)
    if fixture_sha != args.fixture_sha256:
        raise AcceptanceError("fixture SHA-256 mismatch")
    fixture = args.fixture.read_bytes()
    if not fixture:
        raise AcceptanceError("fixture is empty")

    current_target = safe_release_target(CHAPTERA_ROOT / "current")
    previous_target = (
        safe_release_target(CHAPTERA_ROOT / "previous")
        if (CHAPTERA_ROOT / "previous").exists()
        else None
    )
    current_binary = binary_for_target(current_target)
    binary_sha = sha256_file(current_binary)
    if binary_sha != args.expected_binary_sha256:
        raise AcceptanceError("installed current binary SHA-256 differs from expected release")

    version = run([str(current_binary), "--version"]).stdout.strip()
    status = migration_status(current_binary, args.config)
    if status["state"] != "current" or status["pending_count"] != 0:
        raise AcceptanceError("current release database schema is not current")

    initial_services = service_snapshot()
    assert_services_active(initial_services)
    listener = loopback_listener_receipt(args.app_port)
    local = local_health(args.app_port)
    edge = edge_receipt(args.origin)
    initial_probe = guest_probe(
        args.origin, fixture, fixture_sha, timeout=args.timeout_seconds
    )
    capacity_before = service_snapshot()
    capacity = concurrent_reader_receipt(
        args.origin,
        fixture,
        fixture_sha,
        concurrency=args.concurrency,
        timeout=args.timeout_seconds,
    )
    capacity_after = service_snapshot()
    assert_services_active(capacity_after)

    restart: dict[str, Any] = {"exercised": False}
    if args.exercise_restart:
        restart_ms = restart_services()
        local_health(args.app_port)
        probe = guest_probe(args.origin, fixture, fixture_sha, timeout=args.timeout_seconds)
        require_probe_equivalence(initial_probe, probe, "restart")
        restart = {
            "exercised": True,
            "restart_ms": restart_ms,
            "reader_probe": probe,
            "services": service_snapshot(),
        }

    rollback: dict[str, Any] = {
        "exercised": False,
        "previous_present": previous_target is not None,
    }
    if previous_target is not None:
        previous_binary = binary_for_target(previous_target)
        safe, previous_status = exact_previous_schema_compatible(
            status, previous_binary, args.config
        )
        rollback["previous_schema_compatible"] = safe
        rollback["previous_schema"] = previous_status
        rollback["previous_binary_sha256"] = sha256_file(previous_binary)
    else:
        safe = False

    if args.exercise_rollback:
        if previous_target is None:
            rollback["blocked_reason"] = "previous_release_absent"
        elif not safe:
            rollback["blocked_reason"] = "previous_binary_schema_incompatible"
        else:
            original_target = current_target
            switched = False
            try:
                atomic_switch_current(previous_target)
                switched = True
                rollback_restart_ms = restart_services()
                local_health(args.app_port)
                previous_probe = guest_probe(
                    args.origin, fixture, fixture_sha, timeout=args.timeout_seconds
                )
                require_probe_equivalence(initial_probe, previous_probe, "rollback")
                rollback.update({
                    "exercised": True,
                    "previous_restart_ms": rollback_restart_ms,
                    "previous_reader_probe": previous_probe,
                })
            finally:
                if switched:
                    atomic_switch_current(original_target)
                    restore_restart_ms = restart_services()
                    local_health(args.app_port)
                    restored_probe = guest_probe(
                        args.origin, fixture, fixture_sha, timeout=args.timeout_seconds
                    )
                    require_probe_equivalence(initial_probe, restored_probe, "restore")
                    rollback["restored_current"] = True
                    rollback["restore_restart_ms"] = restore_restart_ms
                    rollback["restored_reader_probe"] = restored_probe

    host_operability_complete = bool(
        args.exercise_restart
        and args.exercise_rollback
        and rollback.get("exercised")
        and rollback.get("restored_current")
    )
    # No private deletion is inferred from HTTP TTL or rollback alone. An
    # explicit operator proof reads exact owned SQLite/quarantine identities
    # and waits until the server, not this harness, removes the test source.
    private_cleanup: dict[str, Any] = {
        "verified": False,
        "reason": "private_expiry_not_requested",
    }
    if args.verify_private_expiry:
        # Use a freshly opened session after the capacity and rollback phases:
        # an early witness could already be expired before this phase begins.
        private_witness: dict[str, Any] = {}
        guest_probe(
            args.origin, fixture, fixture_sha, timeout=args.timeout_seconds,
            witness=private_witness,
        )
        db_path = private_storage_config(args.config)
        private_cleanup = private_expiry_receipt(
            args.origin, db_path, private_witness, timeout=args.timeout_seconds
        )
    private_cleanup_verified = private_cleanup["verified"]
    # Run the exact installed binary against the actual loopback Reader
    # service. The new doctor rejects stale running release SHA and unready
    # required producer ports; this is not inferred from static config.
    doctor_receipt = live_doctor_receipt(current_binary, args.config)
    doctor_verified = doctor_receipt["verified"]
    complete = host_operability_complete and private_cleanup_verified and doctor_verified

    receipt = {
        "protocol_version": PROTOCOL,
        "complete": complete,
        "host_operability_complete": host_operability_complete,
        "doctor": doctor_receipt,
        "private_cleanup": private_cleanup,
        "release": {
            "current_target": current_target,
            "current_binary_sha256": binary_sha,
            "version": version,
            "previous_target_present": previous_target is not None,
        },
        "schema": status,
        "services_initial": initial_services,
        "listener": listener,
        "local_health": local,
        "edge": edge,
        "reader_probe": initial_probe,
        "capacity": {
            "before": capacity_before,
            "exercise": capacity,
            "after": capacity_after,
        },
        "restart": restart,
        "rollback": rollback,
        "fixture": {
            "sha256": fixture_sha,
            "byte_len": len(fixture),
        },
        "claims": {
            "raw_pub_bytes_emitted": False,
            "document_text_emitted": False,
            "guest_credentials_emitted": False,
            "storage_locators_emitted": False,
            "guest_session_state_created_via_public_api": True,
            "database_reset_by_harness": False,
            "schema_migration_run_by_harness": False,
        },
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "protocol_version": PROTOCOL,
        "complete": complete,
        "host_operability_complete": host_operability_complete,
        "private_cleanup_verified": private_cleanup_verified,
        "receipt": str(args.out),
        "classification": initial_probe["classification"],
        "compatibility_state": initial_probe["compatibility_state"],
    }, sort_keys=True))
    return 0 if complete else 3


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AcceptanceError as exc:
        print(f"cloud reader host acceptance: {exc}", file=sys.stderr)
        raise SystemExit(2)
