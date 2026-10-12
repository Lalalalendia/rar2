#!/usr/bin/env python3
"""Physical OIDC Product PUB001/023 acceptance, independent of Editor MoveNode B02.

Reuses the production-like S3-free B02 setup helpers for real PKCE/TLS,
SourceIngress, SQLite, native seccomp worker and Chromium, not mocked routes.
Original PUB, session state, Scene and raster PNGs remain ephemeral.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import sys

import run_secure_origin_real_pub as base
from chaptera_local_oidc import OidcFixture

ROOT = base.ROOT
CASE_ID = os.environ.get("CHAPTERA_PRODUCT_MULTI_ID")
PINNED = {
    "001": ("0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade", 72_704, 1),
    "023": ("6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8", 4_573_696, 7),
}
if CASE_ID not in PINNED:
    raise RuntimeError("physical Product multi-image test requires 001 or 023")
PINNED_SHA, PINNED_BYTES, EXPECTED_IMAGES = PINNED[CASE_ID]
TARGET = ROOT / "target" / ("product-oidc-multiimage-" + CASE_ID)


def run_stage(name: str, command: list[str], env: dict[str, str]) -> None:
    result = subprocess.run(
        command, cwd=ROOT, env=env, capture_output=True, text=True, check=False,
    )
    (TARGET / (name + ".log")).write_text(
        result.stdout + "\n--- stderr ---\n" + result.stderr, encoding="utf-8",
    )
    if result.returncode:
        raise RuntimeError(
            f"physical Product022 {name} failed (rc={result.returncode}); "
            "inspect privacy-scanned runner log"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--fixture-sha256", required=True)
    parser.add_argument("--head-sha", required=True)
    parser.add_argument("--caddy-image", default="caddy:2.10.2-alpine")
    args = parser.parse_args()

    binary = args.binary.resolve()
    fixture = args.fixture.resolve()
    if not binary.is_file() or not fixture.is_file():
        raise RuntimeError("pinned Chaptera binary / PUB022 source unavailable")
    if (args.fixture_sha256 != PINNED_SHA or fixture.stat().st_size != PINNED_BYTES
            or hashlib.sha256(fixture.read_bytes()).hexdigest() != PINNED_SHA):
        raise RuntimeError("physical Product022 source SHA and size differ from Publisher oracle")
    if (not re.fullmatch(r"[0-9a-f]{40}", args.head_sha) or
            subprocess.check_output(["git", "rev-parse", "HEAD"],
                                    cwd=ROOT, text=True).strip() != args.head_sha):
        raise RuntimeError("exact checked-out Product multi-image SHA is required")

    TARGET.mkdir(parents=True, exist_ok=True)
    # Reuse the same validated production-like real-HTTP configuration writer,
    # but isolate filesystem diagnostics and Docker process names from CLOUD-B02.
    base.TARGET = TARGET
    base.CONTAINER_NAME = "chaptera-product-oidc-multiimage-" + CASE_ID
    caddy_config = base.render_caddy()
    subprocess.run(
        ["docker", "rm", "-f", base.CONTAINER_NAME],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    handles = {}
    server = None
    worker = None
    caddy = None
    try:
        with tempfile.TemporaryDirectory(prefix="chaptera-product-oidc-022-") as tmp:
            work = Path(tmp)
            db = work / "chaptera.sqlite"
            (work / "scan").mkdir()
            (work / "baseline").mkdir()
            creds = work / "credentials"
            creds.mkdir()
            (creds / "oidc_client_secret").write_text(
                "chaptera-local-only-secret\n", encoding="utf-8",
            )
            (creds / "oidc_client_secret").chmod(0o600)

            redirect_uri = f"https://{base.EDGE_HOST}:{base.HTTPS_PORT}/v1/auth/callback"
            with OidcFixture(redirect_uri=redirect_uri) as provider:
                config = work / "chaptera.toml"
                config.write_text(
                    base.render_config(
                        base.SOURCE_CONFIG.read_text(encoding="utf-8"),
                        database=db, issuer=provider.issuer, binary=binary, temp=work,
                    ),
                    encoding="utf-8",
                )
                environment = os.environ.copy()
                environment.update({
                    "CREDENTIALS_DIRECTORY": str(creds),
                    "AWS_ACCESS_KEY_ID": "chaptera-product-oidc-022",
                    "AWS_SECRET_ACCESS_KEY": "chaptera-product-oidc-022",
                    "AWS_REGION": "us-east-1",
                    "AWS_EC2_METADATA_DISABLED": "true",
                })
                base.run_checked([str(binary), "--config", str(config), "migrate", "up"],
                                 env=environment)

                def launch(name: str, command: list[str]) -> subprocess.Popen[str]:
                    handle = (TARGET / (name + ".log")).open("w", encoding="utf-8")
                    handles[name] = handle
                    return subprocess.Popen(
                        command, cwd=ROOT, env=environment, stdout=handle,
                        stderr=subprocess.STDOUT, text=True,
                    )

                server = launch(
                    "server", [str(binary), "--config", str(config), "serve"],
                )
                base.wait_live(server)
                worker = launch(
                    "worker", [str(binary), "--config", str(config), "worker"],
                )
                handles["caddy"] = (TARGET / "caddy.log").open("w", encoding="utf-8")
                caddy = subprocess.Popen(
                    [
                        "docker", "run", "--rm", "--name", base.CONTAINER_NAME,
                        "--network", "host",
                        "-v", f"{caddy_config.resolve()}:/etc/caddy/Caddyfile:ro",
                        "--entrypoint", "caddy", args.caddy_image, "run",
                        "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile",
                    ],
                    cwd=ROOT, stdout=handles["caddy"],
                    stderr=subprocess.STDOUT, text=True,
                )
                base.wait_tls(caddy)

                owner_scene = work / "owner-scene.json"
                owner_private = work / "owner-identity.json"
                owner_state = work / "owner-browser-state.json"
                browser_env = environment.copy()
                browser_env.update({
                    "CHAPTERA_SECURE_ORIGIN":
                        f"https://{base.EDGE_HOST}:{base.HTTPS_PORT}",
                    "CHAPTERA_PREVIEW_022_FIXTURE": str(fixture),
                    "CHAPTERA_PREVIEW_022_SHA256": PINNED_SHA,
                    "CHAPTERA_PREVIEW_022_PRIVATE": str(owner_private),
                    "CHAPTERA_PREVIEW_022_SCENE_OUT": str(owner_scene),
                    "CHAPTERA_PREVIEW_022_SUMMARY": str(TARGET / "owner.json"),
                    "CHAPTERA_BROWSER_STORAGE_STATE": str(owner_state),
                    "CHAPTERA_HEAD_SHA": args.head_sha,
                    "CHAPTERA_PRODUCT_MULTI_ID": CASE_ID,
                    "CHAPTERA_PRODUCT_022_CROSS_SUMMARY": str(TARGET / "cross.json"),
                    "CHAPTERA_PRODUCT_022_RESTART_SUMMARY": str(TARGET / "restart.json"),
                })
                run_stage(
                    "owner-browser",
                    ["node", str(ROOT / "tools" /
                                 "run_secure_origin_product_multiimage_browser.mjs")],
                    browser_env,
                )
                if not owner_scene.is_file() or not owner_private.is_file() or not owner_state.is_file():
                    raise RuntimeError("real browser did not materialize Product022 private scene/session")
                if worker.poll() is not None:
                    raise RuntimeError("isolated validation worker terminated during Product022 upload")

                owner_report = json.loads(
                    (TARGET / "owner.json").read_text(encoding="utf-8")
                )
                if (owner_report.get("schema") != "chaptera.product-oidc-multiimage-browser.v1"
                        or owner_report.get("fixture") != CASE_ID
                        or owner_report.get("repository_commit_sha") != args.head_sha
                        or owner_report.get("host_image_count") != EXPECTED_IMAGES
                        or owner_report.get("browser_painted_resources") != EXPECTED_IMAGES
                        or owner_report.get("anonymous_scene_status") != 401
                        or owner_report.get("fidelity_state") != "partial"):
                    raise RuntimeError("Product multi-image real HTTP/Chromium receipt not admitted")
                if worker.poll() is not None or server.poll() is not None:
                    raise RuntimeError("isolated Product server or worker died during image acceptance")

                summary = {
                    "schema": "chaptera.product-oidc-multiimage-physical.v1",
                    "repository_head_sha": args.head_sha,
                    "fixture": CASE_ID,
                    "real_oidc_https_source_ingress": True,
                    "real_isolated_product_worker_host_derived_images": True,
                    "authenticated_product_http_browser_paint": True,
                    "host_image_count": owner_report["host_image_count"],
                    "browser_painted_resources": owner_report["browser_painted_resources"],
                    "anonymous_scene_status": 401,
                    "partial_fidelity_disclosed": True,
                    "publisher_exact_oracle_claim": False,
                    "external_s3_claim": False,
                    "deployed_production_claim": False,
                    "source_or_pixel_artifact_uploaded": False,
                }
                (TARGET / "summary.json").write_text(
                    json.dumps(summary, indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
                print(json.dumps(summary, sort_keys=True))
                return 0
    finally:
        base.terminate(worker)
        base.terminate(server)
        subprocess.run(
            ["docker", "rm", "-f", base.CONTAINER_NAME],
            cwd=ROOT, capture_output=True, text=True, check=False,
        )
        base.terminate(caddy)
        for handle in handles.values():
            handle.close()


if __name__ == "__main__":
    raise SystemExit(main())
