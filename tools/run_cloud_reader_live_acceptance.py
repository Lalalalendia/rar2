#!/usr/bin/env python3
"""CLOUD-READER-LIVE-HTTPS-ACCEPTANCE-01.

Exercises the real Cloud Reader guest HTTP vertical behind the canonical Caddy
recipe. A test-only service stands in for S3-compatible storage, while
Chaptera still owns upload admission, quarantine, structural scanner orchestration,
isolated structural/Scene workers, session state and TTL cleanup.
"""

from __future__ import annotations

import argparse
import hashlib
import http.server
import json
import os
import pathlib
import secrets
import shutil
import socket
import sqlite3
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any
from urllib.parse import urlsplit

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE_CADDY = ROOT / "deploy" / "caddy" / "CloudReader.Caddyfile.example"
EDGE_HOST = "reader.chaptera.test"
S3_HOST = "s3.chaptera.test"
QUARANTINE_BUCKET = "chaptera-quarantine"
PRIVATE_BUCKET = "chaptera-private"
FIXTURE_MAX_BYTES = 8 * 1024 * 1024
TRACE_PROTOCOL_VERSION = "chaptera.trace-context.v1"
TRACE_ID = "trace:cloud-reader-live-00000001"
INTERACTION_ID = "interaction:cloud-reader-live-00000001"
SESSION_INCARNATION = "session:cloud-reader-live-00000001"
TRACE_HEADERS = [
    "x-chaptera-trace-version: " + TRACE_PROTOCOL_VERSION,
    "x-chaptera-trace-id: " + TRACE_ID,
    "x-chaptera-interaction-id: " + INTERACTION_ID,
    "x-chaptera-session-incarnation: " + SESSION_INCARNATION,
    "x-chaptera-operation-class: open",
    "x-chaptera-browser-family: chromium",
]


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def run(
    command: list[str],
    *,
    cwd: pathlib.Path = ROOT,
    env: dict[str, str] | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and completed.returncode != 0:
        raise RuntimeError(
            f"command failed rc={completed.returncode}: {command[0]} "
            f"stderr={completed.stderr[-4000:]!r}"
        )
    return completed


def toml_string(value: str | pathlib.Path) -> str:
    return json.dumps(str(value))


def read_http_chunked(stream) -> bytes:
    data = bytearray()
    while True:
        line = stream.readline()
        if not line:
            raise ConnectionError("chunked request ended before chunk header")
        token = line.split(b";", 1)[0].strip()
        size = int(token, 16)
        if size == 0:
            while True:
                trailer = stream.readline()
                if trailer in (b"\r\n", b"\n", b""):
                    break
            break
        chunk = stream.read(size)
        if len(chunk) != size:
            raise ConnectionError("chunked request ended inside chunk")
        data.extend(chunk)
        ending = stream.read(2)
        if ending != b"\r\n":
            raise ConnectionError("invalid chunk terminator")
    return bytes(data)


def decode_aws_chunked(raw: bytes) -> bytes:
    cursor = 0
    decoded = bytearray()
    while cursor < len(raw):
        end = raw.find(b"\r\n", cursor)
        if end < 0:
            raise ValueError("aws-chunked header terminator missing")
        header = raw[cursor:end]
        size_token = header.split(b";", 1)[0]
        size = int(size_token, 16)
        cursor = end + 2
        if size == 0:
            return bytes(decoded)
        next_cursor = cursor + size
        if next_cursor + 2 > len(raw):
            raise ValueError("aws-chunked payload truncated")
        decoded.extend(raw[cursor:next_cursor])
        if raw[next_cursor:next_cursor + 2] != b"\r\n":
            raise ValueError("aws-chunked payload terminator missing")
        cursor = next_cursor + 2
    raise ValueError("aws-chunked terminal chunk missing")


class FakeS3State:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.objects: dict[tuple[str, str], tuple[bytes, str]] = {}
        self.counts = {"PUT": 0, "HEAD": 0, "GET": 0, "DELETE": 0}

    def put(self, bucket: str, key: str, data: bytes) -> str:
        etag = '"' + hashlib.md5(data, usedforsecurity=False).hexdigest() + '"'
        with self.lock:
            self.objects[(bucket, key)] = (data, etag)
            self.counts["PUT"] += 1
        return etag

    def get(self, method: str, bucket: str, key: str) -> tuple[bytes, str] | None:
        with self.lock:
            self.counts[method] += 1
            return self.objects.get((bucket, key))

    def delete(self, bucket: str, key: str) -> tuple[bytes, str] | None:
        with self.lock:
            self.counts["DELETE"] += 1
            return self.objects.pop((bucket, key), None)

    def snapshot(self) -> dict[str, Any]:
        with self.lock:
            return {
                "object_count": len(self.objects),
                "request_counts": dict(self.counts),
            }


class FakeS3Server(http.server.ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, address, state: FakeS3State):
        super().__init__(address, FakeS3Handler)
        self.state = state


class FakeS3Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "chaptera-acceptance-s3"
    sys_version = ""

    def log_message(self, _format: str, *_args: Any) -> None:
        return

    def _target(self) -> tuple[str, str]:
        host = self.headers.get("Host", "").split(":", 1)[0].lower()
        path = urlsplit(self.path).path.lstrip("/")
        suffix = "." + S3_HOST
        if host.endswith(suffix):
            bucket = host[: -len(suffix)]
            key = path
        else:
            pieces = path.split("/", 1)
            if len(pieces) != 2:
                raise ValueError("S3 path does not contain bucket/key")
            bucket, key = pieces
        if bucket not in {QUARANTINE_BUCKET, PRIVATE_BUCKET} or not key:
            raise ValueError("unexpected S3 bucket/key")
        return bucket, key

    def _body(self) -> bytes:
        transfer = self.headers.get("Transfer-Encoding", "").lower()
        if "chunked" in transfer:
            raw = read_http_chunked(self.rfile)
        else:
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise ConnectionError("request body truncated")
        if "aws-chunked" in self.headers.get("Content-Encoding", "").lower():
            raw = decode_aws_chunked(raw)
        decoded_length = self.headers.get("x-amz-decoded-content-length")
        if decoded_length is not None and len(raw) != int(decoded_length):
            raise ValueError("decoded S3 body length mismatch")
        return raw

    def _empty(self, status: int, *, etag: str | None = None) -> None:
        self.send_response(status)
        if etag is not None:
            self.send_header("ETag", etag)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_PUT(self) -> None:
        try:
            bucket, key = self._target()
            if self.headers.get("If-None-Match") == "*" and self.server.state.get("HEAD", bucket, key) is not None:
                self._empty(412)
                return
            data = self._body()
            etag = self.server.state.put(bucket, key, data)
            self._empty(200, etag=etag)
        except Exception:
            self._empty(500)

    def do_HEAD(self) -> None:
        try:
            bucket, key = self._target()
            found = self.server.state.get("HEAD", bucket, key)
            if found is None:
                self._empty(404)
                return
            data, etag = found
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("ETag", etag)
            self.end_headers()
        except Exception:
            self._empty(500)

    def do_GET(self) -> None:
        try:
            bucket, key = self._target()
            found = self.server.state.get("GET", bucket, key)
            if found is None:
                self._empty(404)
                return
            data, etag = found
            if_match = self.headers.get("If-Match")
            if if_match is not None and if_match != etag:
                self._empty(412)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("ETag", etag)
            self.end_headers()
            self.wfile.write(data)
        except Exception:
            self._empty(500)

    def do_DELETE(self) -> None:
        try:
            bucket, key = self._target()
            found = self.server.state.get("HEAD", bucket, key)
            if found is None:
                self._empty(404)
                return
            _, etag = found
            if_match = self.headers.get("If-Match")
            if if_match is not None and if_match != etag:
                self._empty(412)
                return
            self.server.state.delete(bucket, key)
            self._empty(204)
        except Exception:
            self._empty(500)


class OidcServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, address):
        super().__init__(address, OidcHandler)
        self.issuer = f"http://127.0.0.1:{self.server_address[1]}"


class OidcHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "chaptera-acceptance-oidc"
    sys_version = ""

    def log_message(self, _format: str, *_args: Any) -> None:
        return

    def _json(self, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        issuer = self.server.issuer
        if self.path == "/.well-known/openid-configuration":
            self._json(
                {
                    "issuer": issuer,
                    "authorization_endpoint": issuer + "/authorize",
                    "token_endpoint": issuer + "/token",
                    "jwks_uri": issuer + "/jwks",
                    "response_types_supported": ["code"],
                    "subject_types_supported": ["public"],
                    "id_token_signing_alg_values_supported": ["RS256"],
                    "scopes_supported": ["openid"],
                    "token_endpoint_auth_methods_supported": ["client_secret_basic"],
                    "claims_supported": ["sub", "iss", "aud", "exp", "iat"],
                }
            )
            return
        if self.path == "/jwks":
            self._json({"keys": []})
            return
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()


def write_isolation_wrapper(path: pathlib.Path) -> None:
    path.write_text(
        """#!/usr/bin/env python3
import os
import sys

real = os.environ["CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS"]
trace = os.environ["CHAPTERA_ACCEPTANCE_ISOLATION_TRACE"]
args = sys.argv[1:]
kind = "other"
if "--" in args:
    child = args[args.index("--") + 1:]
    if child:
        executable = os.path.basename(child[0])
        if "untrusted-pub-inspect" in child or executable == "chaptera-untrusted-pub-worker":
            kind = "structural_scan"
        elif "guest-reader-scene" in child:
            kind = "guest_scene"
with open(trace, "a", encoding="utf-8") as handle:
    handle.write(kind + "\\n")
os.execv(sys.executable, [sys.executable, real, *args])
""",
        encoding="utf-8",
    )
    path.chmod(0o700)


def write_config(
    path: pathlib.Path,
    *,
    sqlite_path: pathlib.Path,
    app_port: int,
    oidc_port: int,
    isolation_wrapper: pathlib.Path,
    structural_worker: pathlib.Path,
    scan_root: pathlib.Path,
) -> None:
    path.write_text(
        f"""
environment = "test"
listen = "127.0.0.1:{app_port}"
public_origin = "https://{EDGE_HOST}"

[sqlite]
path = {toml_string(sqlite_path)}
journal_mode = "wal"
synchronous = "full"
busy_timeout_ms = 5000
pool_max = 4

[worker]
heavy_concurrency = 1
light_concurrency = 1
quota_shared_capacity = 2
quota_semantic_headroom = 1
quota_export_cap = 1
quota_background_cap = 1

[storage]
provider = "s3-compatible"
quarantine_namespace = "{QUARANTINE_BUCKET}"
private_namespace = "{PRIVATE_BUCKET}"

[limits]
worker_spool_bytes = 1073741824
min_free_disk_bytes = 1

[upload_admission]
principal_concurrent_cap = 2
tenant_concurrent_cap = 4
principal_bytes_cap = 33554432
tenant_bytes_cap = 67108864
max_single_upload_bytes = {FIXTURE_MAX_BYTES}
lease_seconds = 3600
retention_seconds = 86400

[source_validation]
isolation_python = {toml_string(sys.executable)}
isolation_harness = {toml_string(isolation_wrapper)}
worker_binary = {toml_string(structural_worker)}
worker_wall_timeout_ms = 30000
worker_address_space_mb = 512
worker_cpu_seconds = 20
worker_open_files = 64
worker_output_file_mb = 32
max_file_bytes = {FIXTURE_MAX_BYTES}
max_cfb_entries = 8192
max_declared_stream_bytes = 16777216
temp_root = {toml_string(scan_root)}

[edge]
trusted_proxy_ips = ["127.0.0.1"]
max_header_bytes = 32768
max_api_body_bytes = {FIXTURE_MAX_BYTES}
max_upload_body_bytes = {FIXTURE_MAX_BYTES}
request_timeout_ms = 120000

[auth]
login_flow_ttl_seconds = 600
session_idle_ttl_seconds = 1800
session_absolute_ttl_seconds = 86400

[auth.oidc]
issuer = "http://127.0.0.1:{oidc_port}"
client_id = "chaptera-live-acceptance"
redirect_path = "/v1/auth/callback"
client_secret = {{ source = "env", name = "CHAPTERA_OIDC_CLIENT_SECRET" }}

[cloud_reader_guest]
session_ttl_seconds = 60
max_file_bytes = {FIXTURE_MAX_BYTES}
max_concurrent_uploads = 2
max_reserved_bytes = 16777216
rate_subject_secret = {{ source = "env", name = "CHAPTERA_GUEST_RATE_SECRET" }}
""".lstrip(),
        encoding="utf-8",
    )


def write_caddy(
    path: pathlib.Path,
    *,
    app_port: int,
    http_port: int,
    https_port: int,
) -> None:
    source = SOURCE_CADDY.read_text(encoding="utf-8")
    source = source.replace(
        "{\n    admin off\n}",
        "{\n"
        "    admin off\n"
        f"    http_port {http_port}\n"
        f"    https_port {https_port}\n"
        "}",
        1,
    )
    source = source.replace(
        "{$CHAPTERA_READER_SITE:cloud.example.invalid} {",
        f"{EDGE_HOST} {{\n    tls internal",
        1,
    )
    source = source.replace(
        "{$CHAPTERA_READER_API:127.0.0.1:8080}",
        f"127.0.0.1:{app_port}",
        1,
    )
    path.write_text(source, encoding="utf-8")


def wait_http(port: int, process: subprocess.Popen[str], log_path: pathlib.Path) -> None:
    import urllib.request

    for _ in range(200):
        if process.poll() is not None:
            raise RuntimeError(
                "Chaptera exited before /live became ready: "
                + log_path.read_text(encoding="utf-8", errors="replace")[-4000:]
            )
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/live", timeout=0.5) as response:
                if response.status == 200:
                    return
        except Exception:
            time.sleep(0.1)
    raise RuntimeError("Chaptera /live did not become ready")


def wait_tls(port: int, process: subprocess.Popen[str], log_path: pathlib.Path) -> str:
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    last_error: Exception | None = None
    for _ in range(200):
        if process.poll() is not None:
            raise RuntimeError(
                "Caddy exited before TLS became ready: "
                + log_path.read_text(encoding="utf-8", errors="replace")[-4000:]
            )
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5) as raw:
                with context.wrap_socket(raw, server_hostname=EDGE_HOST) as tls:
                    cert = tls.getpeercert(binary_form=True)
                    return hashlib.sha256(cert).hexdigest()
        except Exception as error:
            last_error = error
            time.sleep(0.1)
    raise RuntimeError(f"Caddy TLS did not become ready: {last_error}")


def curl_request(
    *,
    https_port: int,
    method: str,
    path: str,
    output: pathlib.Path,
    headers: list[str] | None = None,
    body_json: dict[str, Any] | None = None,
    upload: pathlib.Path | None = None,
    dump_headers: pathlib.Path | None = None,
) -> tuple[int, bytes]:
    command = [
        "curl",
        "--noproxy",
        "*",
        "--insecure",
        "--silent",
        "--show-error",
        "--resolve",
        f"{EDGE_HOST}:{https_port}:127.0.0.1",
        "--header",
        f"Host: {EDGE_HOST}",
        "--header",
        f"Origin: https://{EDGE_HOST}",
        "--request",
        method,
        "--output",
        str(output),
        "--write-out",
        "%{http_code}",
    ]
    if dump_headers is not None:
        command.extend(["--dump-header", str(dump_headers)])
    for header in headers or []:
        command.extend(["--header", header])
    if method in {"POST", "PUT", "PATCH", "DELETE"}:
        command.extend(["--header", "x-csrf-token: cloud-reader-live-acceptance"])
    if body_json is not None:
        command.extend(
            [
                "--header",
                "Content-Type: application/json",
                "--data-binary",
                json.dumps(body_json, separators=(",", ":")),
            ]
        )
    if upload is not None:
        command.extend(
            [
                "--header",
                "Content-Type: application/octet-stream",
                "--data-binary",
                f"@{upload}",
            ]
        )
    command.append(f"https://{EDGE_HOST}:{https_port}{path}")
    completed = run(command, check=False)
    if completed.returncode != 0:
        raise RuntimeError(f"curl transport failed: {completed.stderr[-2000:]!r}")
    status = int(completed.stdout[-3:])
    return status, output.read_bytes()


def sqlite_count(path: pathlib.Path, query: str) -> int:
    with sqlite3.connect(path) as connection:
        row = connection.execute(query).fetchone()
    if row is None:
        raise AssertionError("SQLite count query returned no row")
    return int(row[0])


def parse_header_file(path: pathlib.Path) -> dict[str, str]:
    raw = path.read_text(encoding="iso-8859-1").replace("\r\n", "\n")
    blocks = [block for block in raw.split("\n\n") if block.strip()]
    if not blocks:
        return {}
    result: dict[str, str] = {}
    for line in blocks[-1].splitlines()[1:]:
        if ":" in line:
            name, value = line.split(":", 1)
            result[name.strip().lower()] = value.strip()
    return result


def parse_reader_trace_events(path: pathlib.Path) -> list[dict[str, Any]]:
    marker = "chaptera_reader_trace "
    events: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        offset = line.find(marker)
        if offset < 0:
            continue
        event = json.loads(line[offset + len(marker):])
        if event.get("trace_id") == TRACE_ID:
            events.append(event)
    return events


def validate_reader_trace_events(events: list[dict[str, Any]]) -> dict[str, Any]:
    expected_stages = [
        "reader.session_create",
        "reader.upload",
        "reader.scan",
        "reader.structural_scan",
        "reader.scene",
        "reader.open",
        "reader.cleanup",
    ]
    stages = [event.get("metric_labels", {}).get("stage") for event in events]
    if stages != expected_stages:
        raise AssertionError("Reader trace stage sequence drifted: " + repr(stages))

    expected_label_keys = {
        "stage",
        "operation_class",
        "outcome",
        "region",
        "protocol_major",
        "browser_family",
    }
    prohibited = {
        "session_id",
        "upload_id",
        "source_sha256",
        "filename",
        "document_id",
        "story_id",
        "storage_locator",
        "token",
        "ip",
    }
    for event in events:
        if event.get("protocol_version") != TRACE_PROTOCOL_VERSION:
            raise AssertionError("Reader trace protocol drifted")
        if event.get("trace_id") != TRACE_ID:
            raise AssertionError("Reader trace id drifted")
        if event.get("interaction_id") != INTERACTION_ID:
            raise AssertionError("Reader interaction id drifted")
        if event.get("session_incarnation") != SESSION_INCARNATION:
            raise AssertionError("Reader session incarnation drifted")
        labels = event.get("metric_labels")
        if not isinstance(labels, dict) or set(labels) != expected_label_keys:
            raise AssertionError("Reader metric label allowlist drifted: " + repr(labels))
        if prohibited.intersection(labels):
            raise AssertionError("Reader metric labels contain prohibited high-cardinality identity")
        if labels.get("operation_class") != "open":
            raise AssertionError("Reader trace operation class drifted")
        if labels.get("browser_family") != "chromium":
            raise AssertionError("Reader trace browser family drifted")
        if labels.get("protocol_major") != "v1" or labels.get("region") != "unknown":
            raise AssertionError("Reader bounded metric dimensions drifted")
        if labels.get("outcome") != "success":
            raise AssertionError("Reader acceptance stage failed: " + repr(labels))
        duration_ms = event.get("duration_ms")
        if not isinstance(duration_ms, (int, float)) or duration_ms < 0:
            raise AssertionError("Reader trace duration is invalid")

    return {
        "protocol_version": TRACE_PROTOCOL_VERSION,
        "trace_id": TRACE_ID,
        "interaction_id": INTERACTION_ID,
        "session_incarnation": SESSION_INCARNATION,
        "stage_sequence": expected_stages,
        "event_count": len(events),
        "metric_label_keys": sorted(expected_label_keys),
        "contains_document_payload": False,
        "semantic_authority": False,
    }


def terminate(process: subprocess.Popen[str] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--chaptera", type=pathlib.Path, required=True)
    parser.add_argument("--structural-worker", type=pathlib.Path, required=True)
    parser.add_argument("--caddy", type=pathlib.Path, required=True)
    parser.add_argument("--fixture", type=pathlib.Path, required=True)
    parser.add_argument("--fixture-sha256", required=True)
    parser.add_argument("--repository-commit", required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()

    chaptera = args.chaptera.resolve()
    structural_worker = args.structural_worker.resolve()
    caddy = args.caddy.resolve()
    fixture = args.fixture.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)

    fixture_sha256 = sha256_file(fixture)
    fixture_bytes = fixture.stat().st_size
    if fixture_sha256 != args.fixture_sha256:
        raise AssertionError("pinned fixture identity drift")
    if fixture_bytes <= 0 or fixture_bytes > FIXTURE_MAX_BYTES:
        raise AssertionError("pinned fixture is outside live acceptance byte envelope")

    work = pathlib.Path(tempfile.mkdtemp(prefix="chaptera-live-reader-"))
    sqlite_path = work / "chaptera.sqlite"
    scan_root = work / "scan"
    config_path = work / "chaptera.toml"
    isolation_wrapper = work / "isolation-wrapper.py"
    isolation_trace = work / "isolation-trace.txt"
    caddy_path = work / "Caddyfile"

    app_port = free_port()
    caddy_http_port = free_port()
    caddy_https_port = free_port()

    s3_state = FakeS3State()
    s3_server = FakeS3Server(("127.0.0.1", 0), s3_state)
    s3_port = int(s3_server.server_address[1])
    s3_thread = threading.Thread(target=s3_server.serve_forever, daemon=True)

    oidc_server = OidcServer(("127.0.0.1", 0))
    oidc_port = int(oidc_server.server_address[1])
    oidc_thread = threading.Thread(target=oidc_server.serve_forever, daemon=True)

    write_isolation_wrapper(isolation_wrapper)
    write_config(
        config_path,
        sqlite_path=sqlite_path,
        app_port=app_port,
        oidc_port=oidc_port,
        isolation_wrapper=isolation_wrapper,
        structural_worker=structural_worker,
        scan_root=scan_root,
    )
    write_caddy(
        caddy_path,
        app_port=app_port,
        http_port=caddy_http_port,
        https_port=caddy_https_port,
    )

    server_log_path = out / "chaptera.log"
    caddy_log_path = out / "caddy.log"
    server_log = server_log_path.open("w", encoding="utf-8")
    caddy_log = caddy_log_path.open("w", encoding="utf-8")

    server: subprocess.Popen[str] | None = None
    caddy_process: subprocess.Popen[str] | None = None
    s3_thread.start()
    oidc_thread.start()

    app_env = os.environ.copy()
    app_env.update(
        {
            "CHAPTERA_GUEST_RATE_SECRET": secrets.token_hex(32),
            "CHAPTERA_OIDC_CLIENT_SECRET": secrets.token_hex(32),
            "CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS": str(
                ROOT / "tools" / "migration_pdf_worker_isolation.py"
            ),
            "CHAPTERA_ACCEPTANCE_ISOLATION_TRACE": str(isolation_trace),
            "AWS_ACCESS_KEY_ID": "chaptera-acceptance",
            "AWS_SECRET_ACCESS_KEY": "chaptera-acceptance",
            "AWS_REGION": "us-east-1",
            "AWS_ENDPOINT_URL": f"http://{S3_HOST}:{s3_port}",
            "AWS_ENDPOINT_URL_S3": f"http://{S3_HOST}:{s3_port}",
        }
    )

    try:
        migrate = run(
            [str(chaptera), "--config", str(config_path), "migrate", "up"],
            env=app_env,
        )
        if migrate.returncode != 0 or not sqlite_path.exists():
            raise AssertionError("migration did not create the acceptance database")

        server = subprocess.Popen(
            [str(chaptera), "--config", str(config_path), "serve"],
            cwd=ROOT,
            env=app_env,
            text=True,
            stdout=server_log,
            stderr=subprocess.STDOUT,
        )
        wait_http(app_port, server, server_log_path)

        caddy_env = os.environ.copy()
        caddy_env.update(
            {
                "XDG_DATA_HOME": str(work / "caddy-data"),
                "XDG_CONFIG_HOME": str(work / "caddy-config"),
            }
        )
        validation = run(
            [str(caddy), "validate", "--config", str(caddy_path), "--adapter", "caddyfile"],
            env=caddy_env,
            cwd=work,
        )
        if validation.returncode != 0:
            raise AssertionError("rendered Cloud Reader Caddyfile did not validate")
        caddy_process = subprocess.Popen(
            [str(caddy), "run", "--config", str(caddy_path), "--adapter", "caddyfile"],
            cwd=work,
            env=caddy_env,
            text=True,
            stdout=caddy_log,
            stderr=subprocess.STDOUT,
        )
        certificate_sha256 = wait_tls(caddy_https_port, caddy_process, caddy_log_path)

        index_path = work / "index.html"
        index_status, index_raw = curl_request(
            https_port=caddy_https_port,
            method="GET",
            path="/",
            output=index_path,
        )
        if index_status != 200:
            raise AssertionError(f"embedded Cloud Reader root returned HTTP {index_status}")
        expected_index = (ROOT / "apps" / "cloud-reader" / "index.html").read_bytes()
        if index_raw != expected_index:
            raise AssertionError("HTTPS root differed from the canonical embedded Cloud Reader index")

        response_path = work / "response.json"
        header_path = work / "headers.txt"
        issued_at_ms = int(time.time() * 1000)
        status, raw = curl_request(
            https_port=caddy_https_port,
            method="POST",
            path="/v1/reader/guest-sessions",
            output=response_path,
            dump_headers=header_path,
            headers=TRACE_HEADERS,
            body_json={"expected_byte_len": fixture_bytes},
        )
        if status != 200:
            raise AssertionError(f"guest session issue returned HTTP {status}")
        issued = json.loads(raw)
        token = issued["access_token"]
        session_id = issued["session_id"]
        expires_at_ms = int(issued["expires_at_ms"])
        headers = parse_header_file(header_path)
        for required in (
            "strict-transport-security",
            "x-content-type-options",
            "referrer-policy",
            "permissions-policy",
        ):
            if required not in headers:
                raise AssertionError(f"HTTPS response missing {required}")
        if headers.get("server") is not None:
            raise AssertionError("public Reader edge leaked Server header")

        session_header = f"x-chaptera-reader-session: {token}"
        upload_status, _ = curl_request(
            https_port=caddy_https_port,
            method="PUT",
            path=issued["upload_path"],
            output=response_path,
            headers=[session_header, *TRACE_HEADERS],
            upload=fixture,
        )
        if upload_status != 200:
            raise AssertionError(f"guest upload returned HTTP {upload_status}")

        open_status, open_raw = curl_request(
            https_port=caddy_https_port,
            method="POST",
            path=issued["open_path"],
            output=response_path,
            headers=[session_header, *TRACE_HEADERS],
        )
        if open_status != 200:
            raise AssertionError(f"guest open returned HTTP {open_status}")
        opened = json.loads(open_raw)
        if opened.get("classification") not in {"supported", "partial"}:
            raise AssertionError(
                f"pinned renderable fixture did not open: {opened.get('classification')!r}"
            )
        if opened.get("source_sha256") != fixture_sha256:
            raise AssertionError("server source identity differs from pinned fixture")
        if opened.get("scene", {}).get("protocol_version") != "chaptera.reader-scene.v1":
            raise AssertionError("open response did not contain Reader Scene V1")
        compatibility = opened.get("compatibility_report")
        if not isinstance(compatibility, dict):
            raise AssertionError("open response did not contain compatibility report")
        if compatibility.get("protocol_version") != "chaptera.reader-compatibility-report.v1":
            raise AssertionError("compatibility report protocol mismatch")
        if compatibility.get("source_sha256") != fixture_sha256:
            raise AssertionError("compatibility report source identity differs from pinned fixture")
        if compatibility.get("engine_classification") != opened["classification"]:
            raise AssertionError("compatibility report classification differs from Reader authority")
        expected_compatibility_state = {
            "supported": "opens_normally",
            "partial": "needs_review",
        }[opened["classification"]]
        if compatibility.get("state") != expected_compatibility_state:
            raise AssertionError("compatibility report customer state differs from Reader classification")
        routes = compatibility.get("output_routes")
        if not isinstance(routes, dict):
            raise AssertionError("compatibility report output routes are missing")
        if routes.get("editable_idml") != "not_verified" or routes.get("editable_odg") != "not_verified":
            raise AssertionError("compatibility report advertised unverified editable migration")

        scene_status, scene_raw = curl_request(
            https_port=caddy_https_port,
            method="GET",
            path=issued["scene_path"],
            output=response_path,
            headers=[session_header, *TRACE_HEADERS],
        )
        if scene_status != 200:
            raise AssertionError(f"guest scene returned HTTP {scene_status}")
        scene = json.loads(scene_raw)
        if scene.get("scene", {}).get("protocol_version") != "chaptera.reader-scene.v1":
            raise AssertionError("scene endpoint did not return Reader Scene V1")
        if scene.get("compatibility_report") != compatibility:
            raise AssertionError("scene endpoint compatibility report differs from open response")

        isolation_kinds = (
            isolation_trace.read_text(encoding="utf-8").splitlines()
            if isolation_trace.exists()
            else []
        )
        if isolation_kinds.count("structural_scan") != 1:
            raise AssertionError("real isolated structural scanner was not exercised exactly once")
        if isolation_kinds.count("guest_scene") != 1:
            raise AssertionError("real isolated guest Scene worker was not exercised exactly once")

        before_storage = s3_state.snapshot()
        if before_storage["object_count"] != 1:
            raise AssertionError("opened guest session must retain exactly one quarantine object until TTL")
        session_rows_before = sqlite_count(
            sqlite_path, "SELECT COUNT(*) FROM reader_guest_sessions"
        )
        active_reservations_before = sqlite_count(
            sqlite_path,
            "SELECT COUNT(*) FROM upload_admission_reservations "
            "WHERE released_at_ms IS NULL",
        )
        if session_rows_before != 1 or active_reservations_before != 0:
            raise AssertionError("pre-expiry guest session/admission state is not converged")

        remaining = (expires_at_ms - int(time.time() * 1000)) / 1000.0
        if remaining > 70:
            raise AssertionError("guest acceptance TTL unexpectedly exceeds bounded test window")
        time.sleep(max(0.0, remaining + 1.5))

        expired_status, expired_raw = curl_request(
            https_port=caddy_https_port,
            method="GET",
            path=issued["scene_path"],
            output=response_path,
            headers=[session_header, *TRACE_HEADERS],
        )
        if expired_status not in {404, 410}:
            raise AssertionError(
                f"expired guest scene expected 404/410, got HTTP {expired_status}"
            )
        try:
            expired_body = json.loads(expired_raw)
            expired_code = expired_body.get("code")
        except Exception:
            expired_code = None

        after_storage = s3_state.snapshot()
        session_rows_after = sqlite_count(
            sqlite_path, "SELECT COUNT(*) FROM reader_guest_sessions"
        )
        active_reservations_after = sqlite_count(
            sqlite_path,
            "SELECT COUNT(*) FROM upload_admission_reservations "
            "WHERE released_at_ms IS NULL",
        )
        if after_storage["object_count"] != 0:
            raise AssertionError("expired guest quarantine object was not physically deleted")
        if session_rows_after != 0:
            raise AssertionError("expired guest session row was not deleted")
        if active_reservations_after != 0:
            raise AssertionError("expired guest upload admission remained active")

        trace_events = parse_reader_trace_events(server_log_path)
        observability_receipt = validate_reader_trace_events(trace_events)

        caddy_version = run([str(caddy), "version"]).stdout.strip()
        receipt = {
            "schema": "chaptera.cloud-reader-live-https-acceptance.v1",
            "task": "CLOUD-READER-LIVE-HTTPS-ACCEPTANCE-01",
            "repository_commit_sha": args.repository_commit,
            "source_fixture": {
                "sha256": fixture_sha256,
                "byte_len": fixture_bytes,
                "raw_bytes_emitted": False,
                "filename_emitted": False,
            },
            "https_edge": {
                "caddy_version": caddy_version,
                "tls_termination": True,
                "certificate_sha256": certificate_sha256,
                "security_headers_present": True,
                "server_header_absent": True,
                "application_listener_loopback": True,
                "embedded_reader_root_status": index_status,
                "embedded_reader_marker_present": True,
            },
            "service_path": {
                "issue_status": status,
                "upload_status": upload_status,
                "open_status": open_status,
                "scene_status": scene_status,
                "classification": opened["classification"],
                "scene_protocol": opened["scene"]["protocol_version"],
                "compatibility_report": {
                    "protocol": compatibility["protocol_version"],
                    "state": compatibility["state"],
                    "source_identity_bound": True,
                    "editable_idml": compatibility["output_routes"]["editable_idml"],
                    "editable_odg": compatibility["output_routes"]["editable_odg"],
                    "limitation_codes": [
                        item.get("code")
                        for item in compatibility.get("limitations", [])
                        if isinstance(item, dict) and isinstance(item.get("code"), str)
                    ],
                },
                "scanner_exact_source_observed": True,
                "isolated_structural_scan_count": isolation_kinds.count("structural_scan"),
                "isolated_guest_scene_count": isolation_kinds.count("guest_scene"),
            },
            "observability": observability_receipt,
            "ttl_cleanup": {
                "issued_at_ms": issued_at_ms,
                "expires_at_ms": expires_at_ms,
                "declared_ttl_ms": expires_at_ms - issued_at_ms,
                "expired_access_status": expired_status,
                "expired_access_code": expired_code,
                "session_rows_before": session_rows_before,
                "session_rows_after": session_rows_after,
                "active_admissions_before": active_reservations_before,
                "active_admissions_after": active_reservations_after,
                "quarantine_objects_before": before_storage["object_count"],
                "quarantine_objects_after": after_storage["object_count"],
                "s3_request_counts": after_storage["request_counts"],
                "backing_state_deleted": True,
            },
            "privacy": {
                "access_token_emitted": False,
                "session_id_emitted": False,
                "raw_story_text_emitted": False,
                "storage_locator_emitted": False,
                "secret_value_emitted": False,
            },
        }
        (out / "receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        print(
            json.dumps(
                {
                    "classification": receipt["service_path"]["classification"],
                    "expired_access_status": expired_status,
                    "backing_state_deleted": True,
                    "trace_event_count": observability_receipt["event_count"],
                    "receipt": str(out / "receipt.json"),
                },
                sort_keys=True,
            )
        )
    finally:
        terminate(caddy_process)
        terminate(server)
        caddy_log.close()
        server_log.close()
        s3_server.shutdown()
        s3_server.server_close()
        oidc_server.shutdown()
        oidc_server.server_close()
        s3_thread.join(timeout=2)
        oidc_thread.join(timeout=2)
        shutil.rmtree(work, ignore_errors=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
