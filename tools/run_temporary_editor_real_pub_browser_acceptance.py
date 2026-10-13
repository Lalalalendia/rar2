#!/usr/bin/env python3
"""Real Publisher guest editor UI through actual Caddy/TLS and isolated Rust workers.

The Chromium script uses native browser file upload, pointer events, download
and reload. No test substitutes an HTTP response or performs a manual MoveNode.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import secrets
import shutil
import subprocess
import tempfile
import threading

import run_cloud_reader_live_acceptance as live


def main() -> int:
    ap = argparse.ArgumentParser()
    for key in ("chaptera", "caddy", "fixture", "out"):
        ap.add_argument("--" + key, required=True, type=pathlib.Path)
    ap.add_argument("--fixture-sha256", required=True)
    ap.add_argument("--target", choices=("idml", "pdf"), default="idml")
    args = ap.parse_args()
    binary = args.chaptera.resolve()
    caddy = args.caddy.resolve()
    fixture = args.fixture.resolve()
    out = args.out.resolve()
    if not binary.is_file() or not caddy.is_file():
        raise AssertionError("canonical Chaptera binary and pinned Caddy required")
    if (live.sha256_file(fixture) != args.fixture_sha256 or
            fixture.stat().st_size != 291840):
        raise AssertionError("public pinned SampleNewsletter.pub fixture invalid")
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="chaptera-temp-guest-browser-"))
    db, config = work / "chaptera.sqlite", work / "config.toml"
    wrapper = work / "wrapper.py"
    caddyfile = work / "Caddyfile"
    server_log, caddy_log = out / "chaptera.log", out / "caddy.log"
    appport, httpport, httpsport = (
        live.free_port(), live.free_port(), live.free_port()
    )
    oidc = live.OidcServer(("127.0.0.1", 0))
    oidc_thread = threading.Thread(target=oidc.serve_forever, daemon=True)
    oidc_thread.start()
    server = proxy = None
    server_stream = caddy_stream = None
    try:
        live.write_isolation_wrapper(wrapper)
        live.write_config(
            config, sqlite_path=db, app_port=appport,
            oidc_port=int(oidc.server_address[1]),
            isolation_wrapper=wrapper, structural_worker=binary,
            scan_root=work / "scan",
        )
        source = config.read_text(encoding="utf-8")
        if source.count('provider = "s3-compatible"') != 1:
            raise AssertionError("fixture must override exactly one storage provider")
        origin_setting = 'public_origin = "https://' + live.EDGE_HOST + '"'
        if source.count(origin_setting) != 1:
            raise AssertionError("expected one canonical public browser origin")
        # Chromium sends Origin and Host with the ephemeral TLS port. The
        # real Rust edge enforces an exact scheme/host/port + CSRF equality.
        source = source.replace(
            origin_setting,
            'public_origin = "https://' + live.EDGE_HOST + f':{httpsport}"',
        )
        config.write_text(
            source.replace('provider = "s3-compatible"', 'provider = "filesystem"'),
            encoding="utf-8",
        )
        live.write_caddy(caddyfile, app_port=appport, http_port=httpport,
                         https_port=httpsport)
        caddy_config = caddyfile.read_text(encoding="utf-8")
        required = (
            "header_up X-Forwarded-Host {hostport}",
            "/editor/temporary-preview",
            "/editor/temporary-edit-move-client-v1.mjs",
            "/editor/reader-scene-editor-interaction-v1.mjs",
            "/cloud-reader/render-v1.mjs",
            "/v1/editor/temporary-sessions",
            "/v1/editor/temporary-sessions/*",
        )
        for path in required:
            if path not in caddy_config:
                raise AssertionError("deployed Caddy gate excludes guest path " + path)
        if "/editor/*" in caddy_config.split("@reader_surface path", 1)[-1]:
            raise AssertionError("wildcard Product Editor static exposure forbidden")
        env = os.environ.copy()
        env.update({
            "CHAPTERA_GUEST_RATE_SECRET": secrets.token_hex(32),
            "CHAPTERA_OIDC_CLIENT_SECRET": secrets.token_hex(32),
            "CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS":
                str(live.ROOT / "tools" / "migration_pdf_worker_isolation.py"),
            "CHAPTERA_ACCEPTANCE_ISOLATION_TRACE":
                str(work / "isolation.txt"),
        })
        caddy_env = os.environ.copy()
        caddy_env.update({
            "XDG_DATA_HOME": str(work / "caddy-data"),
            "XDG_CONFIG_HOME": str(work / "caddy-config"),
        })
        live.run([str(binary), "--config", str(config), "migrate", "up"],
                 env=env)
        server_stream = server_log.open("w", encoding="utf-8")
        server = subprocess.Popen(
            [str(binary), "--config", str(config), "serve"],
            env=env, cwd=live.ROOT, text=True,
            stdout=server_stream, stderr=subprocess.STDOUT,
        )
        live.wait_http(appport, server, server_log)
        live.run([str(caddy), "validate", "--config", str(caddyfile),
                  "--adapter", "caddyfile"], env=caddy_env, cwd=work)
        caddy_stream = caddy_log.open("w", encoding="utf-8")
        proxy = subprocess.Popen(
            [str(caddy), "run", "--config", str(caddyfile),
             "--adapter", "caddyfile"], env=caddy_env, cwd=work,
            text=True, stdout=caddy_stream, stderr=subprocess.STDOUT,
        )
        certificate_hash = live.wait_tls(httpsport, proxy, caddy_log)
        if not certificate_hash or len(certificate_hash) != 64:
            raise AssertionError("guest public TLS listener not established")
        origin = f"https://{live.EDGE_HOST}:{httpsport}"
        command = [
            "node", str(live.ROOT / "tools" /
                        "run_temporary_editor_real_pub_browser.mjs"),
            "--origin", origin,
            "--fixture", str(fixture),
            "--fixture-sha256", args.fixture_sha256,
            "--out", str(out / "receipt.json"),
            "--target", args.target,
        ]
        result = subprocess.run(
            command, env=env, cwd=live.ROOT, text=True,
            capture_output=True, timeout=540, check=False,
        )
        if result.returncode != 0:
            # Never echo raw source bytes, private HTTP payloads or bearer
            # material in the public CI progress output.
            raise AssertionError(
                "physical Chromium guest acceptance failed; see chaptera/Caddy "
                "logs and source-safe diagnostic: " +
                (result.stderr or result.stdout)[-1800:]
            )
        receipt = json.loads((out / "receipt.json").read_text(encoding="utf-8"))
        target = args.target
        expected_geometry = (
            "pdf_rectangle_stream" if target == "pdf" else "idml_path_geometry"
        )
        if (receipt.get("status") != "PASS" or
                receipt.get("real_pub") is not True or
                receipt.get("real_chromium") is not True or
                receipt.get("export_target") != target or
                receipt.get("browser_pointer_commits") != 1 or
                receipt.get("source_sha256") != args.fixture_sha256 or
                receipt.get(f"changed_{target}_geometry_proven") is not True or
                receipt.get(f"changed_{target}_identity_proven") is not True or
                receipt.get(f"changed_{target}_geometry_form") != expected_geometry or
                receipt.get("same_browser_session_delete_proven") is not True or
                receipt.get("revoked_old_current_scene_export_denied") is not True or
                receipt.get("temporary_bearer_cleared_after_delete") is not True or
                receipt.get("loss_report_checked") is not True or
                receipt.get("bearer_absent_from_urls") is not True or
                receipt.get("original_pub_immutable") is not True or
                receipt.get("v0_go") is not False):
            raise AssertionError("full physical browser receipt invalid")
        if target == "pdf" and (
            receipt.get("pristine_pdf_without_edit_proven") is not True
            or receipt.get("pristine_pdf_unchanged_revision_proven") is not True
            or receipt.get("pristine_pdf_private_report_proven") is not True
            or not isinstance(receipt.get("pristine_pdf_sha256"), str)
            or len(receipt["pristine_pdf_sha256"]) != 64
            or not isinstance(receipt.get("pristine_pdf_pages"), int)
            or receipt["pristine_pdf_pages"] < 1
        ):
            raise AssertionError("real-browser untouched PUB baseline PDF proof is missing")
        print(json.dumps({
            "schema": receipt["schema"],
            "status": receipt["status"],
            "real_pub_browser_https": True,
            "browser_pointer_commits": receipt["browser_pointer_commits"],
            "target": target,
            "edited_artifact_sha256":
                receipt[f"changed_{target}_download_sha256"],
            "downloaded_consumer_geometry_proven": True,
            "v0_go": False,
        }, sort_keys=True))
        return 0
    finally:
        live.terminate(proxy)
        live.terminate(server)
        if caddy_stream is not None:
            caddy_stream.close()
        if server_stream is not None:
            server_stream.close()
        oidc.shutdown()
        oidc.server_close()
        oidc_thread.join(timeout=2)
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
