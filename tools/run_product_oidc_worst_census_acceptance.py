#!/usr/bin/env python3
"""Physical OIDC Product PUB020/026/043 census, independent of visual 49/111 claim.

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

from product_full_cohort_shards_v1 import build as load_exact_product_cohort

ROOT = base.ROOT
CASE_ID = os.environ.get("CHAPTERA_PRODUCT_MULTI_ID")
COHORT = load_exact_product_cohort()
CASES = {item["id"]: item
         for shard in COHORT["shards"] for item in shard["fixtures"]}
if CASE_ID not in CASES:
    raise RuntimeError("physical Product input not in pinned 39-document eligible roster")
CASE_PLAN = CASES[CASE_ID]
PINNED_SHA, PINNED_BYTES, EXPECTED_PAGES = (
    CASE_PLAN["source_sha256"], CASE_PLAN["source_bytes"], CASE_PLAN["pages"])
TARGET = ROOT / "target" / ("product-oidc-worst-census-" + CASE_ID)


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
# Public codes are from literal program stages, never extracted dynamically
# from user-controlled error messages, document IDs or private server logs.
PRODUCT_VISUAL_PHASE_CODES = {
    "scene_input": "visual_scene_input",
    "ablation_ready": "visual_image_ablation",
    "chromium_init": "visual_chromium_init",
    "before_navigate": "visual_before_navigation",
    "before_render": "visual_before_render",
    "before_page_membership": "visual_before_page_identity",
    "before_raster": "visual_before_raster",
    "after_navigate": "visual_after_navigation",
    "after_render": "visual_after_render",
    "after_page_membership": "visual_after_page_identity",
    "after_raster": "visual_after_raster",
    "final_checks": "visual_final_checks",
    "finished": "visual_finished",
}
OWNER_HTTP_PHASE_CODES = frozenset({
    "http_both_unanswered", "http_both_unrequested", "http_both_mixed",
    "http_current_unanswered", "http_current_unrequested",
    "http_scene_unanswered", "http_scene_unrequested",
})


def read_private_fixed_code(path_string: str | None, allowed: set[str] | frozenset[str],
                            default: str) -> str:
    """Return only a fixed allowlisted enum, never text read from an artifact.

    The sidecar is written by the physical browser into its private temporary
    directory. It is never uploaded, echoed or included in the score.
    """
    if not path_string:
        return default
    try:
        path = Path(path_string)
        if not path.is_file() or path.stat().st_size > 64:
            return default
        value = path.read_text(encoding="ascii").strip()
        return value if value in allowed else default
    except (OSError, UnicodeError, ValueError):
        return default


def fixed_failure(code: str) -> dict[str, object]:
    return {
        "failure_code": code,
        "current_http_status": None,
        "scene_http_status": None,
        "public_error_code": "redacted",
    }


def classify_visual_failure(phase_path: str | None) -> dict[str, object]:
    phase = read_private_fixed_code(
        phase_path, set(PRODUCT_VISUAL_PHASE_CODES), "unclassified")
    return fixed_failure(
        PRODUCT_VISUAL_PHASE_CODES.get(phase, "visual_unclassified"))


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


def classify_owner_failure(private_log: str, http_phase_path: str | None = None) -> dict[str, object]:
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
    if "Product owner HTTP response not observed" in private_log:
        code = read_private_fixed_code(
            http_phase_path, OWNER_HTTP_PHASE_CODES, "unclassified_browser_failure")
        return fixed_failure(code)
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
            classify_owner_failure(
                private_log, env.get("CHAPTERA_PRODUCT_HTTP_RESPONSE_PHASE_FILE"))
            if name == "owner-browser"
            else classify_visual_failure(
                env.get("CHAPTERA_PRODUCT_VISUAL_PHASE_FILE"))
            if name == "product-worst-visual-browser"
            else fixed_failure("bounded_stage_failure")
        )
        # On failure this remains a source-neutral numeric/enum receipt.
        # The existing fail-closed leak scan examines the private log too.
        receipt = {
            "schema": "chaptera.product-oidc-worst-census-failure.v1",
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
    base.CONTAINER_NAME = "chaptera-product-oidc-worst-census-" + CASE_ID
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
            # Source-neutral, signed by the frozen 49/111 reference loader above.
            # Kept ephemeral and rechecked against the actual PUB by both browsers.
            case_plan_path = work / "product-case-plan.json"
            case_plan_path.write_text(
                json.dumps(CASE_PLAN, sort_keys=True), encoding="utf-8")
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
                    "CHAPTERA_PRODUCT_CASE_PLAN": str(case_plan_path),
                    "CHAPTERA_PRODUCT_HTTP_RESPONSE_PHASE_FILE": str(work / "owner-http-phase.txt"),
                    "CHAPTERA_PRODUCT_022_CROSS_SUMMARY": str(TARGET / "cross.json"),
                    "CHAPTERA_PRODUCT_022_RESTART_SUMMARY": str(TARGET / "restart.json"),
                })
                run_stage(
                    "owner-browser",
                    ["node", str(ROOT / "tools" /
                                 "run_secure_origin_product_worst_browser.mjs")],
                    browser_env,
                )
                if not owner_scene.is_file() or not owner_private.is_file() or not owner_state.is_file():
                    raise RuntimeError("real browser did not materialize Product022 private scene/session")
                if worker.poll() is not None:
                    raise RuntimeError("isolated validation worker terminated during Product022 upload")

                owner_report = json.loads(
                    (TARGET / "owner.json").read_text(encoding="utf-8")
                )
                if (owner_report.get("schema") != "chaptera.product-oidc-worst-census-browser.v1"
                        or owner_report.get("fixture") != CASE_ID
                        or owner_report.get("repository_commit_sha") != args.head_sha
                        or owner_report.get("document_page_count") != EXPECTED_PAGES
                        or type(owner_report.get("host_image_count")) is not int
                        or not 0 <= owner_report["host_image_count"] <= 96
                        or owner_report.get("browser_painted_resources") != owner_report["host_image_count"]
                        or owner_report.get("anonymous_scene_status") != 401
                        or owner_report.get("fidelity_state") not in ("supported", "partial")):
                    raise RuntimeError("Product worst fixture OIDC/HTTPS/Chromium receipt not admitted")
                if worker.poll() is not None or server.poll() is not None:
                    raise RuntimeError("isolated Product service stopped during worst-document census")
                # Same physical OIDC Product Scene, rendered at canonical 144 DPI
                # in Chromium with and without ONLY host-derived image bytes.
                # Private Scene, source and both PNG arms are never uploaded.
                visual_dir = work / ("publisher-visual-" + CASE_ID)
                visual_env = browser_env.copy()
                visual_env.update({
                    "CHAPTERA_PREVIEW_PROBE_SCENE_OUT": str(owner_scene),
                    "CHAPTERA_PREVIEW_BROWSER_OUTPUT": str(visual_dir),
                    "REPOSITORY_COMMIT_SHA": args.head_sha,
                    "CHAPTERA_PRODUCT_VISUAL_PHASE_FILE": str(work / "product-visual-phase.txt"),
                })
                run_stage(
                    "product-worst-visual-browser",
                    ["node", str(ROOT / "apps" / "cloud-reader" /
                                 "product-worst-visual-browser.test.mjs")],
                    visual_env,
                )
                report_path = work / "publisher-comparison.json"
                run_stage(
                    "product-worst-visual-score",
                    [sys.executable, str(ROOT / "tools" / "reader_product_worst_visual_64.py"),
                     "--case", CASE_ID,
                     "--fingerprint", str(ROOT / "tools" / "corpus" / "receipts" /
                       "publisher-visual-golden-batch-01-fingerprint-v1.json"),
                     "--pairs", str(ROOT / "tools" / "corpus" / "receipts" /
                       "publisher-visual-golden-batch-01-pairs.csv"),
                     "--frozen", str(ROOT / "tools" / "corpus" / "receipts" /
                       "publisher-lo-t999-49-111-frozen-v1.json"),
                     "--browser-dir", str(visual_dir),
                     "--out", str(report_path),
                     "--head", args.head_sha],
                    visual_env,
                )
                comparison = json.loads(report_path.read_text(encoding="utf-8"))
                if (comparison.get("schema") != "chaptera.product-worst-publisher-ab-score.v1"
                        or comparison.get("fixture") != CASE_ID
                        or comparison.get("repository_commit_sha") != args.head_sha
                        or comparison.get("ranked_pages") != EXPECTED_PAGES):
                    raise RuntimeError("real Product Publisher visual receipt identity invalid")
                (TARGET / "publisher-comparison.json").write_text(
                    json.dumps(comparison, indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
                summary = {
                    "schema": "chaptera.product-oidc-worst-census-physical.v1",
                    "repository_head_sha": args.head_sha,
                    "fixture": CASE_ID,
                    "real_oidc_https_source_ingress": True,
                    "real_isolated_product_worker_host_derived_images": True,
                    "authenticated_product_http_browser_paint": True,
                    "original_pub_sha256": PINNED_SHA,
                    "expected_publisher_pdf_pages": EXPECTED_PAGES,
                    "product_scene_pages": owner_report["document_page_count"],
                    "product_scene_nodes": owner_report["node_count"],
                    "node_kinds": owner_report["node_kinds"],
                    "host_owned_image_count": owner_report["host_image_count"],
                    "browser_painted_unique_resources": owner_report["browser_painted_resources"],
                    "image_count_source_expectation_proven": False,
                    "descriptor_only_resource_count": owner_report["descriptor_only_resource_count"],
                    "fidelity_state": owner_report["fidelity_state"],
                    "fidelity_reason_count": owner_report["fidelity_reason_count"],
                    "anonymous_scene_status": 401,
                    "real_publisher_visual_a_b": True,
                    "publisher_comparison_page_count": comparison["ranked_pages"],
                    "publisher_product_page_wins": comparison["product_page_wins"],
                    "publisher_lo_page_wins": comparison["libreoffice_page_wins"],
                    "publisher_near_ties": comparison["near_ties"],
                    "publisher_exact_oracle_claim": False,
                    "full_product_49_111_score": False,
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
