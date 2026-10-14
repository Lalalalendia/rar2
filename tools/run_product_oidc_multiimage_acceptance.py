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


# Only constants or safe numeric HTTP codes may escape the ephemeral browser log.
# This is not an error-body exporter; source IDs, URLs, cookies and tracebacks
# must never become GitHub log lines or downloadable diagnostic receipts.
PUBLIC_SCENE_FAILURE_CODES = frozenset({
    "redacted", "grant_missing", "grant_expired", "capability_denied",
    "product_replay_worker_failed", "product_replay_worker_timeout",
    "product_reader_safe_preview_invalid", "product_replay_receipt_invalid",
    "product_replay_receipt_identity_mismatch", "reader_scene_projection_failed",
    "revision_materialization_failed", "stale_revision",
})
OWNER_FAILURE_SIGNATURES = (
    ("page.waitForURL: Timeout", "ingress_navigation_timeout"),
    ("page.waitForResponse: Timeout", "product_http_response_timeout"),
    ("page.waitForFunction: Timeout", "chromium_paint_timeout"),
    ("authenticated Product HTTP did not preserve pinned source and current revision",
     "source_or_revision_contract"),
    ("pinned Product fixture does not admit its exact image count", "image_count_contract"),
    ("Product image resource identity is missing or duplicated", "image_id_contract"),
    ("Product host preview URL/MIME invalid", "image_mime_contract"),
    ("Product host derivative exceeds cap or has invalid magic", "image_decode_cap"),
    ("Product derived image does not belong to a Scene node", "image_node_binding"),
    ("Product Scene size cap violated", "scene_size_cap"),
    ("host preview lost its explicit partial color-fidelity disposition", "fidelity_disclosure"),
    ("authenticated Product source images failed Chromium paint", "chromium_image_paint"),
    ("browser displayed a non-host image URL", "external_image_denied"),
    ("anonymous Product Reader request did not return 401", "anonymous_denial"),
    ("real OIDC browser session not preserved", "oidc_session_contract"),
    ("real Publisher 022 source identity mismatch", "source_identity_contract"),
)


def classify_owner_failure(private_log: str) -> dict[str, object]:
    """No caller-controlled value survives this fixed-envelope classifier."""
    match = re.search(
        r"UI Product HTTP failed: current=(\d{3}) scene=(\d{3}) "
        r"fixed_code=([a-z0-9_]{1,80})", private_log,
    )
    if match:
        current = int(match.group(1))
        scene = int(match.group(2))
        return {
            "failure_code": "product_http_denial",
            "current_http_status": current if 100 <= current <= 599 else None,
            "scene_http_status": scene if 100 <= scene <= 599 else None,
            "public_error_code": (
                match.group(3) if match.group(3) in PUBLIC_SCENE_FAILURE_CODES
                else "redacted"
            ),
        }
    for literal, code in OWNER_FAILURE_SIGNATURES:
        if literal in private_log:
            return {
                "failure_code": code,
                "current_http_status": None,
                "scene_http_status": None,
                "public_error_code": "redacted",
            }
    return {
        "failure_code": "unclassified_browser_failure",
        "current_http_status": None,
        "scene_http_status": None,
        "public_error_code": "redacted",
    }


def run_stage(name: str, command: list[str], env: dict[str, str]) -> None:
    result = subprocess.run(
        command, cwd=ROOT, env=env, capture_output=True, text=True, check=False,
    )
    private_log = result.stdout + "\n--- stderr ---\n" + result.stderr
    (TARGET / (name + ".log")).write_text(private_log, encoding="utf-8")
    if result.returncode:
        failure = (
            classify_owner_failure(private_log)
            if name == "owner-browser"
            else {
                "failure_code": "bounded_stage_failure",
                "current_http_status": None,
                "scene_http_status": None,
                "public_error_code": "redacted",
            }
        )
        # On failure this remains a source-neutral numeric/enum receipt.
        # The existing fail-closed leak scan examines the private log too.
        receipt = {
            "schema": "chaptera.product-oidc-multiimage-failure.v1",
            "fixture": CASE_ID,
            "repository_commit_sha": env.get("CHAPTERA_HEAD_SHA", "unverified"),
            **failure,
            "source_or_pixel_artifact_uploaded": False,
        }
        (TARGET / "owner.json").write_text(
            json.dumps(receipt, sort_keys=True) + "\n", encoding="utf-8",
        )
        raise RuntimeError(
            f"physical Product multi-image {CASE_ID} {name} failed "
            f"(rc={result.returncode}); fixed_failure={failure['failure_code']} "
            f"current_status={failure['current_http_status']} "
            f"scene_status={failure['scene_http_status']} "
            f"public_code={failure['public_error_code']}; private logs not uploaded"
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
                # The ephemeral Caddy process can emit session/cookie material
                # in reverse-proxy error diagnostics. Do not persist raw edge
                # stdout/stderr or forward them into public CI; TLS readiness
                # and browser HTTPS assertions are independent health evidence.
                caddy = subprocess.Popen(
                    [
                        "docker", "run", "--rm", "--name", base.CONTAINER_NAME,
                        "--network", "host",
                        "-v", f"{caddy_config.resolve()}:/etc/caddy/Caddyfile:ro",
                        "--entrypoint", "caddy", args.caddy_image, "run",
                        "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile",
                    ],
                    cwd=ROOT, stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL, text=True,
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
                # Compare the very same authenticated Product Scene against
                # the frozen Publisher PDF, with an image-off negative control.
                # Raw Scene and Chromium PNG files live only in work/tmp.
                visual_dir = work / ("product-publisher-ab-" + CASE_ID)
                visual_env = browser_env.copy()
                visual_env.update({
                    "CHAPTERA_PREVIEW_PROBE_SCENE_OUT": str(owner_scene),
                    "CHAPTERA_PREVIEW_BROWSER_OUTPUT": str(visual_dir),
                    "REPOSITORY_COMMIT_SHA": args.head_sha,
                })
                run_stage(
                    "product-publisher-ab-browser",
                    ["node", str(ROOT / "apps" / "cloud-reader" /
                                 "product-safe-preview-multiimage-browser.test.mjs")],
                    visual_env,
                )
                visual_report_path = work / "publisher-visual.json"
                run_stage(
                    "product-publisher-ab-score",
                    [sys.executable, str(ROOT / "tools" /
                         "reader_safe_preview_visual_multiimage.py"),
                     CASE_ID,
                     str(ROOT / "tools" / "corpus" / "receipts" /
                         "publisher-visual-golden-batch-01-fingerprint-v1.json"),
                     str(visual_dir), str(visual_report_path), args.head_sha],
                    visual_env,
                )
                visual_report = json.loads(visual_report_path.read_text(encoding="utf-8"))
                if (visual_report.get("fixture") != CASE_ID
                        or visual_report.get("repository_head_sha") != args.head_sha
                        or visual_report.get("host_owned_image_count") != EXPECTED_IMAGES
                        or visual_report.get("same_source_scene_head_and_browser") is not True):
                    raise RuntimeError("Product 001/023 Publisher visual A/B evidence invalid")
                if worker.poll() is not None or server.poll() is not None:
                    raise RuntimeError("isolated Product server or worker died during image acceptance")

                # Actual independently issued OIDC identity from the same
                # local test provider: no forged browser cookie or grant.
                first_subject, first_email = provider.subject, provider.email
                provider.subject = "product-" + CASE_ID + "-foreign-oidc-subject"
                provider.email = "product-" + CASE_ID + "-foreign@chaptera.invalid"
                try:
                    run_stage(
                        "cross-principal",
                        ["node", str(ROOT / "tools" /
                                     "run_product_oidc_multiimage_cross_browser.mjs")],
                        browser_env,
                    )
                finally:
                    provider.subject, provider.email = first_subject, first_email
                cross = json.loads((TARGET / "cross.json").read_text(encoding="utf-8"))
                if (cross.get("schema") != "chaptera.product-oidc-multiimage-cross-principal.v1"
                        or cross.get("fixture") != CASE_ID
                        or cross.get("foreign_scene_http_status") != 403
                        or cross.get("missing_scene_http_status") != 403
                        or cross.get("absent_document_denial_indistinguishable") is not True
                        or cross.get("source_revision_page_image_disclosure") is not False):
                    raise RuntimeError("physical foreign-principal Product denial not admitted")

                # Durable SQLite/BlobStore are preserved but both actual
                # server and isolated worker processes must die and restart.
                base.terminate(worker)
                worker = None
                base.terminate(server)
                server = None
                for name in ("worker", "server"):
                    handles[name].close()
                    del handles[name]
                server = launch(
                    "server-restart", [str(binary), "--config", str(config), "serve"],
                )
                base.wait_live(server)
                worker = launch(
                    "worker-restart", [str(binary), "--config", str(config), "worker"],
                )
                run_stage(
                    "owner-restart",
                    ["node", str(ROOT / "tools" /
                                 "run_product_oidc_multiimage_restart_browser.mjs")],
                    browser_env,
                )
                restart = json.loads((TARGET / "restart.json").read_text(encoding="utf-8"))
                if (restart.get("schema") != "chaptera.product-oidc-multiimage-restart.v1"
                        or restart.get("fixture") != CASE_ID
                        or restart.get("original_oidc_owner_session_preserved") is not True
                        or restart.get("server_and_isolated_worker_restarted") is not True
                        or restart.get("source_revision_page_and_resource_ids_stable") is not True
                        or restart.get("safely_reencoded_image_count") != EXPECTED_IMAGES
                        or restart.get("chromium_images_decoded_and_painted") != EXPECTED_IMAGES):
                    raise RuntimeError("physical restarted Product image admission not proven")
                if worker.poll() is not None or server.poll() is not None:
                    raise RuntimeError("restarted Product server or worker terminated")

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
                    "real_second_oidc_principal_denied": True,
                    "cross_principal_denial_indistinguishable": True,
                    "original_oidc_session_survives_server_worker_restart": True,
                    "restart_images_decoded_and_painted": EXPECTED_IMAGES,
                    "partial_fidelity_disclosed": True,
                    "publisher_exact_oracle_claim": False,
                    "publisher_64x64_visual_ab_measured": True,
                    "publisher_64x64_changed_without_images":
                        visual_report["chromium_64x64_changed_without_images"],
                    "publisher_64x64_changed_with_images":
                        visual_report["chromium_64x64_changed_with_images"],
                    "publisher_64x64_image_effect":
                        visual_report["chromium_64x64_net_improvement"],
                    "publisher_64x64_image_improvement_observed":
                        visual_report["visual_improvement_observed"],
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
