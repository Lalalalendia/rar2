#!/usr/bin/env python3
"""Physical no-login temporary EditorSession CURRENT: exact PUB, HTTPS, TTL."""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import secrets
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time

import run_cloud_reader_live_acceptance as live


def checked(status: int, expected: int, action: str) -> None:
    if status != expected:
        raise AssertionError(f"{action} HTTP {status}, expected {expected}")


def main() -> int:
    ap = argparse.ArgumentParser()
    for key in ("chaptera", "caddy", "fixture", "out"):
        ap.add_argument("--" + key, type=pathlib.Path, required=True)
    ap.add_argument("--fixture-sha256", required=True)
    args = ap.parse_args()

    fixture = args.fixture.resolve()
    expected_hash = live.sha256_file(fixture)
    if expected_hash != args.fixture_sha256 or not 0 < fixture.stat().st_size < live.FIXTURE_MAX_BYTES:
        raise AssertionError("pinned real PUB source identity/size mismatch")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="chaptera-temp-current-"))
    db, config, wrapper = work / "chaptera.sqlite", work / "config.toml", work / "wrapper.py"
    caddyfile, response = work / "Caddyfile", work / "response.json"
    server_log, caddy_log = out / "chaptera.log", out / "caddy.log"
    appport, httpport, httpsport = live.free_port(), live.free_port(), live.free_port()
    oidc = live.OidcServer(("127.0.0.1", 0))
    oidc_thread = threading.Thread(target=oidc.serve_forever, daemon=True)
    oidc_thread.start()

    live.write_isolation_wrapper(wrapper)
    live.write_config(
        config,
        sqlite_path=db,
        app_port=appport,
        oidc_port=int(oidc.server_address[1]),
        isolation_wrapper=wrapper,
        structural_worker=args.chaptera.resolve(),
        scan_root=work / "scan",
    )
    original_config = config.read_text(encoding="utf-8")
    if original_config.count('provider = "s3-compatible"') != 1:
        raise AssertionError("expected exactly one fixture provider declaration")
    config.write_text(
        original_config.replace('provider = "s3-compatible"', 'provider = "filesystem"'),
        encoding="utf-8",
    )
    live.write_caddy(caddyfile, app_port=appport, http_port=httpport, https_port=httpsport)
    site = caddyfile.read_text(encoding="utf-8")
    needle = "@reader_surface path / /index.html"
    if site.count(needle) != 1:
        raise AssertionError("Caddy Reader surface unexpectedly changed")
    site = site.replace(
        needle,
        "@reader_surface path /v1/editor/temporary-sessions "
        "/v1/editor/temporary-sessions/* / /index.html",
    )
    caddyfile.write_text(site, encoding="utf-8")

    env = os.environ.copy()
    env.update(
        {
            "CHAPTERA_GUEST_RATE_SECRET": secrets.token_hex(32),
            "CHAPTERA_OIDC_CLIENT_SECRET": secrets.token_hex(32),
            "CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS": str(
                live.ROOT / "tools" / "migration_pdf_worker_isolation.py"
            ),
            "CHAPTERA_ACCEPTANCE_ISOLATION_TRACE": str(work / "isolation.txt"),
        }
    )
    caddy_env = os.environ.copy()
    caddy_env.update(
        {"XDG_DATA_HOME": str(work / "caddy-data"), "XDG_CONFIG_HOME": str(work / "caddy-config")}
    )
    s_log = server_log.open("w", encoding="utf-8")
    c_log = caddy_log.open("w", encoding="utf-8")
    server: subprocess.Popen[str] | None = None
    proxy: subprocess.Popen[str] | None = None
    token: str | None = None

    def call(method: str, path: str, *, headers: list[str] | None = None, **other):
        return live.curl_request(
            https_port=httpsport,
            method=method,
            path=path,
            output=response,
            headers=headers,
            **other,
        )

    try:
        live.run([str(args.chaptera.resolve()), "--config", str(config), "migrate", "up"], env=env)
        server = subprocess.Popen(
            [str(args.chaptera.resolve()), "--config", str(config), "serve"],
            cwd=live.ROOT, env=env, text=True, stdout=s_log, stderr=subprocess.STDOUT,
        )
        live.wait_http(appport, server, server_log)
        live.run([str(args.caddy.resolve()), "validate", "--config", str(caddyfile),
                  "--adapter", "caddyfile"], env=caddy_env, cwd=work)
        proxy = subprocess.Popen(
            [str(args.caddy.resolve()), "run", "--config", str(caddyfile),
             "--adapter", "caddyfile"], cwd=work, env=caddy_env,
            text=True, stdout=c_log, stderr=subprocess.STDOUT,
        )
        live.wait_tls(httpsport, proxy, caddy_log)

        status, raw = call(
            "POST", "/v1/editor/temporary-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(status, 200, "temporary capability issuance")
        issued = json.loads(raw)
        session_id, token = issued["session_id"], issued["access_token"]
        if not session_id.startswith("tempedit:") or len(token) != 64:
            raise AssertionError("temporary capability protocol mismatch")
        path = f"/v1/editor/temporary-sessions/{session_id}/current"
        private = [f"x-chaptera-reader-session: {token}"]
        checked(call("GET", path)[0], 401, "no-capability current")
        checked(call("GET", path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign-capability current")
        checked(call("PUT", issued["upload_path"], upload=fixture, headers=private)[0],
                200, "exact temporary PUB upload")
        opened_status, opened_raw = call("POST", issued["open_path"], headers=private)
        checked(opened_status, 200, "real isolated Reader open")
        opened = json.loads(opened_raw)
        if opened.get("classification") not in {"supported", "partial"}:
            raise AssertionError("pinned Publisher source is not Reader-eligible")
        if opened.get("source_sha256") != expected_hash:
            raise AssertionError("Reader source hash differs from immutable upload")
        physical_source_files = [
            p for p in (work / "blob-store").rglob("*")
            if p.is_file() and p.stat().st_size == fixture.stat().st_size
            and live.sha256_file(p) == expected_hash
        ]
        if len(physical_source_files) != 1:
            raise AssertionError("the exact quarantined original PUB is not physically present")
        physical_source = physical_source_files[0]
        expected_expiry = int(opened["expires_at_ms"])
        if not 3_000_000 < expected_expiry - int(time.time() * 1000) <= 3_600_000:
            raise AssertionError("accepted upload did not start one-hour TTL")

        current_status, current_raw = call("GET", path, headers=private)
        checked(current_status, 200, "isolated canonical EditorSession current")
        current = json.loads(current_raw)
        if current.get("protocol_version") != "chaptera.temporary-editor-current.v1":
            raise AssertionError("canonical current protocol invalid")
        if current.get("document_id") != session_id or current.get("source_hash") != expected_hash:
            raise AssertionError("canonical current source/session binding mismatch")
        if current.get("revision_cursor") != 0:
            raise AssertionError("read-only current illegally created a revision")
        if not isinstance(current.get("project"), dict) or not isinstance(
            current.get("authoring_graph"), dict
        ):
            raise AssertionError("isolated EditorSession returned no project or geometry")
        if len(current.get("revision_id", "")) != 64 or len(
            current.get("canonical_authoring_revision_id", "")
        ) != 64:
            raise AssertionError("canonical source-derived revision identity invalid")
        if current.get("expires_at_ms") != expected_expiry:
            raise AssertionError("GET current illegally renewed temporary TTL")

        second_status, second_raw = call("GET", path, headers=private)
        checked(second_status, 200, "idempotent current")
        second = json.loads(second_raw)
        for field in ("source_hash", "project", "revision_id", "expires_at_ms"):
            if second.get(field) != current.get(field):
                raise AssertionError("second current read changed " + field)

        status, raw = call(
            "POST", "/v1/reader/guest-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(status, 200, "ordinary Guest Reader session issuance")
        guest = json.loads(raw)
        checked(call(
            "GET", f"/v1/editor/temporary-sessions/{guest['session_id']}/current",
            headers=[f"x-chaptera-reader-session: {guest['access_token']}"],
        )[0], 404, "ordinary guest forbidden from authoring current")

        with sqlite3.connect(db) as conn:
            conn.execute(
                "UPDATE reader_guest_sessions SET expires_at_ms=? WHERE session_id=?",
                (int(time.time() * 1000) - 1, session_id.encode()),
            )
        expired_status = call("GET", path, headers=private)[0]
        if expired_status not in {404, 410}:
            raise AssertionError("expired capability retained document access")
        with sqlite3.connect(db) as conn:
            remaining = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions WHERE session_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
        if remaining:
            raise AssertionError("expired temporary session kept its private metadata")
        if physical_source.exists():
            raise AssertionError("expired temporary session retained original PUB bytes")

        receipt = {
            "schema": "chaptera.tempedit-canonical-current-https.v1",
            "source_sha256": expected_hash, "source_byte_len": fixture.stat().st_size,
            "canonical_current_status": current_status, "revision_cursor": 0,
            "private_capability_required": True, "foreign_token_rejected": True,
            "ordinary_guest_rejected": True, "no_read_ttl_extension": True,
            "expired_access_status": expired_status, "expired_rows": remaining, "physical_original_deleted": True,
            "isolated_real_pub_read": True, "backend_filesystem": True,
            "raw_source_or_bearer_in_receipt": False,
            "mutation_proven": False, "edited_download_proven": False, "v0_go": False,
        }
        (out / "receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(json.dumps({"canonical_current_status": current_status, "v0_go": False}))
    finally:
        live.terminate(proxy)
        live.terminate(server)
        c_log.close()
        s_log.close()
        oidc.shutdown()
        oidc.server_close()
        oidc_thread.join(timeout=2)
        if token is not None:
            for log in (server_log, caddy_log):
                if token in log.read_text(encoding="utf-8", errors="replace"):
                    raise AssertionError("private capability appeared in diagnostic logs")
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
