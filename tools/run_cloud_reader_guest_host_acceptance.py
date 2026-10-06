#!/usr/bin/env python3
"""Minimal physical acceptance for the public Chaptera guest Reader."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request

SERVICE_DEFAULT = "chaptera-reader.service"
PORT_DEFAULT = 8080
TOKEN_HEADER = "x-chaptera-reader-session"
GUEST_PROTOCOL = "chaptera.reader-guest-session.v1"
SCENE_PROTOCOL = "chaptera.reader-scene.v1"


class AcceptanceError(RuntimeError):
    pass


def run(command: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    if check and completed.returncode != 0:
        raise AcceptanceError(
            f"command failed rc={completed.returncode}: {command[0]}: {completed.stderr.strip()[-500:]}"
        )
    return completed


def systemd_metric(service: str, prop: str) -> int | None:
    raw = run(["systemctl", "show", service, "-p", prop, "--value"], check=False).stdout.strip()
    if not raw or raw in {"[not set]", "infinity"}:
        return None
    try:
        return int(raw)
    except ValueError:
        return None


def require_service(service: str) -> dict[str, int | bool | None]:
    active = run(["systemctl", "is-active", "--quiet", service], check=False).returncode == 0
    if not active:
        raise AcceptanceError(f"{service} is not active")
    return {
        "active": True,
        "memory_current_bytes": systemd_metric(service, "MemoryCurrent"),
        "tasks_current": systemd_metric(service, "TasksCurrent"),
    }


def require_loopback_listener(port: int) -> list[str]:
    rows = []
    for line in run(["ss", "-H", "-ltn"]).stdout.splitlines():
        parts = line.split()
        if len(parts) >= 4 and parts[3].endswith(f":{port}"):
            rows.append(parts[3])
    if not rows:
        raise AcceptanceError(f"no TCP listener on port {port}")
    unsafe = [
        value
        for value in rows
        if not (
            value.startswith("127.0.0.1:")
            or value.startswith("[::1]:")
            or value.startswith("::1:")
        )
    ]
    if unsafe:
        raise AcceptanceError(f"Reader listener is not loopback-only: {unsafe}")
    return rows


def request(
    origin: str,
    path: str,
    *,
    method: str = "GET",
    body: bytes | None = None,
    headers: dict[str, str] | None = None,
    timeout: int = 120,
) -> tuple[int, dict[str, str], bytes]:
    url = urllib.parse.urljoin(origin.rstrip("/") + "/", path.lstrip("/"))
    req = urllib.request.Request(url, data=body, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return (
                response.status,
                {k.lower(): v for k, v in response.headers.items()},
                response.read(),
            )
    except urllib.error.HTTPError as exc:
        return (
            exc.code,
            {k.lower(): v for k, v in exc.headers.items()},
            exc.read(),
        )


def request_json(
    origin: str,
    path: str,
    *,
    method: str = "GET",
    body: bytes | None = None,
    headers: dict[str, str] | None = None,
) -> tuple[int, dict[str, str], dict]:
    status, response_headers, raw = request(
        origin, path, method=method, body=body, headers=headers
    )
    try:
        payload = json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        payload = {}
    if not isinstance(payload, dict):
        payload = {}
    return status, response_headers, payload


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--origin", default="https://reader.chaptera.online")
    parser.add_argument("--fixture", required=True, type=pathlib.Path)
    parser.add_argument("--service", default=SERVICE_DEFAULT)
    parser.add_argument("--port", default=PORT_DEFAULT, type=int)
    parser.add_argument("--out", type=pathlib.Path)
    args = parser.parse_args()

    fixture = args.fixture.read_bytes()
    if not fixture:
        raise AcceptanceError("fixture is empty")
    fixture_sha256 = hashlib.sha256(fixture).hexdigest()

    service_before = require_service(args.service)
    listeners = require_loopback_listener(args.port)

    status, headers, index = request(args.origin, "/")
    if status != 200 or b"Chaptera" not in index:
        raise AcceptanceError(f"Reader index failed: HTTP {status}")
    if "content-security-policy" not in headers:
        raise AcceptanceError("Reader response is missing Content-Security-Policy")

    csrf_token = "chaptera-host-acceptance"
    create_body = json.dumps({"expected_byte_len": len(fixture)}).encode("utf-8")
    status, _, issued = request_json(
        args.origin,
        "/v1/reader/guest-sessions",
        method="POST",
        body=create_body,
        headers={
            "Content-Type": "application/json",
            "Origin": args.origin,
            "X-CSRF-Token": csrf_token,
        },
    )
    if status != 200 or issued.get("protocol_version") != GUEST_PROTOCOL:
        raise AcceptanceError(f"guest session creation failed: HTTP {status}")

    session_id = issued.get("session_id")
    token = issued.get("access_token")
    upload_path = issued.get("upload_path")
    open_path = issued.get("open_path")
    scene_path = issued.get("scene_path")
    if not all(isinstance(value, str) and value for value in [session_id, token, upload_path, open_path, scene_path]):
        raise AcceptanceError("guest session response is incomplete")

    session_headers = {
        TOKEN_HEADER: token,
        "Content-Type": "application/octet-stream",
        "Content-Length": str(len(fixture)),
        "Origin": args.origin,
        "X-CSRF-Token": csrf_token,
    }
    status, _, uploaded = request_json(
        args.origin,
        upload_path,
        method="PUT",
        body=fixture,
        headers=session_headers,
    )
    if status != 200 or uploaded.get("state") != "stored":
        raise AcceptanceError(f"guest upload failed: HTTP {status}")

    status, _, opened = request_json(
        args.origin,
        open_path,
        method="POST",
        body=b"{}",
        headers={
            TOKEN_HEADER: token,
            "Content-Type": "application/json",
            "Origin": args.origin,
            "X-CSRF-Token": csrf_token,
        },
    )
    classification = opened.get("classification")
    if status != 200 or classification not in {"supported", "partial", "salvage"}:
        raise AcceptanceError(
            f"guest open failed: HTTP {status}, classification={classification!r}, terminal={opened.get('terminal_code')!r}"
        )
    if opened.get("source_sha256") != fixture_sha256:
        raise AcceptanceError("Reader source SHA-256 differs from uploaded fixture")

    status, _, scene = request_json(
        args.origin,
        scene_path,
        headers={TOKEN_HEADER: token},
    )
    if status != 200 or scene.get("protocol_version") != GUEST_PROTOCOL:
        raise AcceptanceError(f"guest scene endpoint failed: HTTP {status}")
    scene_payload = scene.get("scene")
    salvage_payload = scene.get("salvage")
    protocol = None
    if isinstance(scene_payload, dict):
        protocol = scene_payload.get("protocol_version")
    elif isinstance(salvage_payload, dict):
        protocol = salvage_payload.get("protocol_version")
    if classification != "salvage" and protocol != SCENE_PROTOCOL:
        raise AcceptanceError(f"Reader Scene protocol mismatch: {protocol!r}")

    service_after = require_service(args.service)
    receipt = {
        "protocol_version": "chaptera.cloud-reader-guest-host-acceptance.v1",
        "accepted_at_unix_ms": int(time.time() * 1000),
        "origin": args.origin,
        "service": args.service,
        "loopback_port": args.port,
        "loopback_listeners": listeners,
        "fixture": {
            "byte_len": len(fixture),
            "sha256": fixture_sha256,
        },
        "result": {
            "classification": classification,
            "source_identity_bound": True,
            "scene_protocol": protocol,
        },
        "resources": {
            "before": service_before,
            "after": service_after,
        },
    }

    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
