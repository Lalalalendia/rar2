#!/usr/bin/env python3
"""Source-safe one-file Chaptera Cloud Reader compatibility bridge.

The public checker must never interpret Publisher bytes or issue compatibility
verdicts of its own. It uses an isolated Cloud guest session, verifies the exact
source hash, and forwards only the canonical service-owned report.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import sys
from urllib import error, parse, request

GUEST_SCHEMA = "chaptera.reader-guest-session.v1"
REPORT_SCHEMA = "chaptera.reader-compatibility-report.v1"
MAX_FILE_BYTES = 64 * 1024 * 1024
MAX_OPEN_RESPONSE_BYTES = 20 * 1024 * 1024
MAX_REPORT_BYTES = 64 * 1024
STATE_BY_CLASSIFICATION = {
    "supported": "opens_normally",
    "partial": "needs_review",
    "salvage": "opens_with_salvage",
    "unsupported": "unsupported",
}
NEXT_BY_STATE = {
    "opens_normally": "migration_pilot_preview",
    "needs_review": "review_preview_before_migration",
    "opens_with_salvage": "rescue_review",
    "unsupported": "unsupported_or_manual_review",
}
ID_PATTERN = re.compile(r"[A-Za-z0-9_:-]{1,160}\Z")
TOKEN_PATTERN = re.compile(r"[0-9a-fA-F]{64}\Z")
SHA_PATTERN = re.compile(r"[0-9a-f]{64}\Z")


class BridgeError(Exception):
    pass


class NoRedirect(request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


OPENER = request.build_opener(NoRedirect)


def cloud_origin(value: str) -> str:
    url = parse.urlsplit(value)
    if (url.scheme != "https" or url.hostname != "reader.chaptera.online"
            or url.port not in (None, 443)
            or url.username or url.password or url.path not in ("", "/")
            or url.query or url.fragment):
        raise BridgeError("cloud_origin_invalid")
    return "https://reader.chaptera.online"


def http_bytes(origin: str, path: str, method: str, body: bytes,
               headers: dict[str, str], max_response: int) -> bytes:
    if not path.startswith("/v1/reader/guest-sessions") or "?" in path or "#" in path:
        raise BridgeError("guest_endpoint_invalid")
    req = request.Request(
        origin + path,
        data=body if method != "GET" else None,
        method=method,
        headers=headers,
    )
    try:
        with OPENER.open(req, timeout=100) as resp:
            if resp.status < 200 or resp.status >= 300:
                raise BridgeError("cloud_http_failed")
            data = resp.read(max_response + 1)
            if len(data) > max_response:
                raise BridgeError("cloud_response_oversized")
            return data
    except (error.HTTPError, error.URLError, TimeoutError, OSError):
        raise BridgeError("cloud_http_failed") from None


def json_guest(origin: str, path: str, method: str, body: bytes,
               headers: dict[str, str], max_response: int = MAX_REPORT_BYTES) -> dict:
    raw = http_bytes(origin, path, method, body, headers, max_response)
    try:
        value = json.loads(raw)
    except (UnicodeError, ValueError):
        raise BridgeError("guest_json_invalid") from None
    if not isinstance(value, dict):
        raise BridgeError("guest_json_invalid")
    return value


def canonical_report_for(source: bytes, origin: str) -> dict:
    if not source or len(source) > MAX_FILE_BYTES:
        raise BridgeError("source_size_invalid")
    sha = hashlib.sha256(source).hexdigest()
    csrf = secrets.token_hex(16)
    base_headers = {
        "x-csrf-token": csrf,
        "content-type": "application/json",
        "cache-control": "no-store",
    }

    issued = json_guest(
        origin, "/v1/reader/guest-sessions", "POST",
        json.dumps({"expected_byte_len": len(source)}, separators=(",", ":")).encode(),
        base_headers,
    )
    sid = issued.get("session_id")
    token = issued.get("access_token")
    if (issued.get("protocol_version") != GUEST_SCHEMA
        or not isinstance(sid, str) or not ID_PATTERN.fullmatch(sid)
        or not isinstance(token, str) or not TOKEN_PATTERN.fullmatch(token)):
        raise BridgeError("guest_issue_protocol_invalid")

    # Server-returned route strings are advisory only: never follow arbitrary
    # paths or URLs. Construct endpoints from the validated opaque session id.
    root = f"/v1/reader/guest-sessions/{sid}"
    if issued.get("upload_path") != root + "/content" or issued.get("open_path") != root + "/open":
        raise BridgeError("guest_issue_path_invalid")

    authorized = dict(base_headers)
    authorized["x-chaptera-reader-session"] = token
    authorized["content-type"] = "application/octet-stream"
    uploaded = json_guest(origin, root + "/content", "PUT", source, authorized)
    if (uploaded.get("protocol_version") != GUEST_SCHEMA
        or uploaded.get("session_id") != sid
        or uploaded.get("state") != "stored"):
        raise BridgeError("guest_upload_protocol_invalid")

    authorized["content-type"] = "application/json"
    opened = json_guest(
        origin, root + "/open", "POST", b"{}", authorized, MAX_OPEN_RESPONSE_BYTES
    )
    classification = opened.get("classification")
    report = opened.get("compatibility_report")
    if (opened.get("protocol_version") != GUEST_SCHEMA
        or opened.get("session_id") != sid
        or opened.get("source_sha256") != sha
        or classification not in STATE_BY_CLASSIFICATION
        or not isinstance(report, dict)
        or report.get("protocol_version") != REPORT_SCHEMA
        or report.get("source_sha256") != sha
        or report.get("engine_classification") != classification
        or report.get("state") != STATE_BY_CLASSIFICATION[classification]
        or report.get("recommended_next_step") != NEXT_BY_STATE[report["state"]]):
        raise BridgeError("canonical_report_protocol_mismatch")

    # Only service-owned canonical data is returned. The larger /open response
    # may contain private stories and Scene; they MUST NOT appear in result.json.
    encoded = json.dumps(report, separators=(",", ":"), ensure_ascii=True).encode("utf-8")
    if len(encoded) > MAX_REPORT_BYTES:
        raise BridgeError("canonical_report_oversized")
    return report


def failure_receipt() -> dict:
    # Transport/internal failure only; never mislabel as unsupported PUB.
    return {
        "compatibility": "failed",
        "summary": "The checker could not complete this request reliably.",
        "diagnosticsCode": "pub_check.worker_failure",
    }


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: worker.py EXACT_SOURCE.pub OUTPUT.json", file=sys.stderr)
        return 2
    source_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])
    try:
        if not source_path.is_file() or source_path.stat().st_size > MAX_FILE_BYTES:
            raise BridgeError("source_size_invalid")
        source = source_path.read_bytes()
        origin = cloud_origin(os.environ.get(
            "PUB_CHECK_CLOUD_READER_ORIGIN", "https://reader.chaptera.online"
        ))
        result = canonical_report_for(source, origin)
        code = 0
    except (BridgeError, OSError, ValueError):
        result = failure_receipt()
        code = 1
    output_path.write_text(json.dumps(result, separators=(",", ":")), encoding="utf-8")
    print("cloud compatibility bridge: canonical receipt ready" if code == 0
          else "cloud compatibility bridge: fail closed (no document verdict)")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
