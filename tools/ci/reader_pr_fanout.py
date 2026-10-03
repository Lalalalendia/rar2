#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
import os
from pathlib import Path
import re
import subprocess
from typing import Iterable


EVIDENCE_ONLY_PATHS = {
    "vendor/producer-a/crates/pub-viewer/src/bin/corpus-reader-receipt.rs",
}

TEST_REGION_EVIDENCE_CANDIDATES = {
    "apps/chaptera-server/src/reader_scene_v1.rs",
}

HUNK_HEADER = re.compile(
    r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@",
    re.MULTILINE,
)

SCOPES = (
    "tier_a",
    "reader_windows_smoke",
    "reader_windows",
    "editor_windows",
    "visual_oracle",
    "cloud_reference",
    "virginia_page_role",
    "visual_batch01",
    "typography_golden",
    "android_core",
    "android",
    "web",
    "local_portable",
    "installer",
    "path_identity",
    "update_accept",
)


def changed_paths(base: str, head: str) -> list[str]:
    return sorted(
        dict.fromkeys(
            p
            for p in subprocess.check_output(
                ["git", "diff", "--name-only", f"{base}...{head}"], text=True
            ).splitlines()
            if p
        )
    )


def matches(path: str, patterns: Iterable[str]) -> bool:
    for pattern in patterns:
        if pattern.endswith("/**") and path.startswith(pattern[:-3]):
            return True
        if fnmatch.fnmatchcase(path, pattern):
            return True
    return False


def cfg_test_module_line(source: str) -> int | None:
    lines = source.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != "#[cfg(test)]":
            continue
        for following_index, following in enumerate(
            lines[index + 1 :],
            start=index + 1,
        ):
            stripped = following.strip()
            if not stripped:
                continue
            if stripped.startswith("mod tests {"):
                return following_index + 1
            break
    return None


def diff_hunks_within_test_region(
    diff_text: str,
    *,
    old_test_line: int,
    new_test_line: int,
) -> bool:
    hunks = list(HUNK_HEADER.finditer(diff_text))
    if not hunks:
        return False
    for hunk in hunks:
        old_start = int(hunk.group(1))
        old_count = int(hunk.group(2) or "1")
        new_start = int(hunk.group(3))
        new_count = int(hunk.group(4) or "1")
        if old_count and old_start <= old_test_line:
            return False
        if new_count and new_start <= new_test_line:
            return False
    return True


def test_region_evidence_only_paths(
    base: str,
    head: str,
    paths: list[str],
) -> set[str]:
    evidence_only: set[str] = set()
    for path in paths:
        if path not in TEST_REGION_EVIDENCE_CANDIDATES:
            continue
        try:
            base_source = subprocess.check_output(
                ["git", "show", f"{base}:{path}"], text=True
            )
            head_source = subprocess.check_output(
                ["git", "show", f"{head}:{path}"], text=True
            )
            diff_text = subprocess.check_output(
                ["git", "diff", "--unified=0", f"{base}...{head}", "--", path],
                text=True,
            )
        except subprocess.CalledProcessError:
            continue
        old_test_line = cfg_test_module_line(base_source)
        new_test_line = cfg_test_module_line(head_source)
        if old_test_line is None or new_test_line is None:
            continue
        if diff_hunks_within_test_region(
            diff_text,
            old_test_line=old_test_line,
            new_test_line=new_test_line,
        ):
            evidence_only.add(path)
    return evidence_only


READER_SHARED = (
    "Cargo.toml",
    "Cargo.lock",
    "crates/chaptera-viewer-render-plan/**",
    "vendor/producer-a/Cargo.toml",
    "vendor/producer-a/Cargo.lock",
    "vendor/producer-a/crates/pub-core/**",
    "vendor/producer-a/crates/pub-cfb/**",
    "vendor/producer-a/crates/pub-contents/**",
    "vendor/producer-a/crates/pub-escher/**",
    "vendor/producer-a/crates/pub-model/**",
    "vendor/producer-a/crates/pub-quill/**",
    "vendor/producer-a/crates/pub-reader/**",
    "vendor/producer-a/crates/pub-layout/**",
    "vendor/producer-a/crates/pub-viewer/**",
)

TIER_A = READER_SHARED + (
    "apps/chaptera-desktop/**",
    "vendor/producer-a/crates/pub-editor/**",
    "tools/reader_active_content_guard.py",
    "tools/ci/reader_consumer_preflight.py",
    "tools/ci/test_reader_consumer_preflight.py",
    "tools/ci/reader_pr_fanout.py",
    "tools/ci/test_reader_pr_fanout.py",
    ".github/workflows/reader-pr-ci.yml",
    ".github/workflows/reader-consumer-preflight.yml",
)

READER_DESKTOP = (
    "apps/chaptera-desktop/Cargo.toml",
    "apps/chaptera-desktop/src/main.rs",
    "apps/chaptera-desktop/src/diagnostic_sweep.rs",
    "apps/chaptera-desktop/src/image_decode_adapter.rs",
    "apps/chaptera-desktop/src/product_smoke.rs",
    "apps/chaptera-desktop/src/reader_product_ui.rs",
    "apps/chaptera-desktop/src/render_backend.rs",
)

READER_WINDOWS_SMOKE = READER_SHARED + (
    ".github/workflows/chaptera-reader-windows-smoke.yml",
)

READER_WINDOWS = READER_DESKTOP + (
    "crates/chaptera-update-engine/**",
    "crates/chaptera-update-orchestrator/**",
    "crates/chaptera-update-handoff/**",
    "packages/product/reader-portable/**",
    "packages/product/desktop-suite/**",
    "tools/package_reader_portable.py",
    "tools/test_package_reader_portable.py",
    ".github/workflows/chaptera-reader-windows.yml",
)

EDITOR_WINDOWS = (
    "Cargo.toml",
    "Cargo.lock",
    "crates/chaptera-scene-instance/**",
    "packages/product/editor-live-trial/**",
    "packages/protocol/editor-agent-control/**",
    "packages/product/desktop-suite/**",
    "tools/package_editor_live_trial.py",
    "tools/build_editor_live_trial_package_receipt.py",
    "tools/validate_editor_live_trial_package_receipt.py",
    "tools/run_editor_live_trial_package_smoke.py",
    "tools/validate_editor_desktop_ux_snapshots.py",
    "tools/run_local_editor_desktop_vertical.py",
    "tools/validate_chaptera_desktop_suite_manifest.py",
    ".github/workflows/chaptera-desktop-windows.yml",
)

VISUAL_ORACLE = (
    "crates/pub-presentation-profile/**",
    "crates/pub-model/**",
    "vendor/producer-a/crates/pub-quill/**",
    "vendor/producer-a/crates/pub-reader/**",
    "vendor/producer-a/crates/pub-layout/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "crates/chaptera-viewer-render-plan/**",
    "apps/chaptera-desktop/src/render_backend.rs",
    "apps/chaptera-desktop/src/source_font.rs",
    "apps/chaptera-desktop/src/main.rs",
    "tools/acquire_carlton_march_pair.py",
    "tools/pdf_reference_diff_v1.py",
    "tools/reader_reference_pdf_raster_v1.py",
    "tools/reader_page_role_observation.py",
    ".github/workflows/carlton-reader-visual-oracle.yml",
)

CLOUD_REFERENCE = (
    "apps/cloud-reader/**",
    "apps/chaptera-server/src/reader_scene_v1.rs",
    "apps/chaptera-server/src/guest_reader_worker.rs",
    "crates/chaptera-viewer-render-plan/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "vendor/producer-a/crates/pub-reader/src/lib.rs",
    "vendor/producer-a/crates/pub-reader/src/bin/reference_fill_state_census.rs",
    "vendor/producer-a/crates/pub-reader/tests/table_cell_paint_join_probe.rs",
    "tools/acquire_carlton_march_pair.py",
    "tools/acquire_virginia_pub_pdf_pairs.py",
    "tools/cloud_reader_reference_pdf_raster_v1.py",
    "tools/cloud_reader_manual_oracle_bundle_v1.py",
    "tools/test_cloud_reader_manual_oracle_bundle_v1.py",
    "tools/pdf_reference_diff_v1.py",
    "tools/census_embedded_eot.py",
    ".github/workflows/cloud-reader-reference-pairs.yml",
)

VIRGINIA_PAGE_ROLE = (
    "vendor/producer-a/crates/pub-reader/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "crates/pub-presentation-profile/**",
    "tools/acquire_virginia_pub_pdf_pairs.py",
    "tools/validate_virginia_page_roles.py",
    ".github/workflows/viewer-page-role-virginia.yml",
)

VISUAL_BATCH01 = (
    ".github/workflows/publisher-visual-golden-batch01.yml",
    "tools/cloud_reader_visual_fingerprint_v1.py",
    "tools/corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json",
    "tools/corpus/receipts/publisher-visual-golden-batch-01-pairs.csv",
    "apps/cloud-reader/**",
    "apps/chaptera-server/src/reader_scene_v1.rs",
    "apps/chaptera-server/src/guest_reader_worker.rs",
    "crates/chaptera-viewer-render-plan/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "vendor/producer-a/crates/pub-reader/src/lib.rs",
)

TYPOGRAPHY_GOLDEN = (
    "vendor/producer-a/crates/pub-quill/**",
    "vendor/producer-a/crates/pub-reader/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "crates/chaptera-viewer-render-plan/**",
    "apps/chaptera-desktop/src/render_backend.rs",
    "apps/chaptera-desktop/src/source_font.rs",
    "apps/chaptera-desktop/src/main.rs",
    ".github/workflows/reader-typography-golden.yml",
)

ANDROID_CORE = READER_SHARED + (
    "crates/chaptera-mobile-reader-core/**",
    ".github/workflows/mobile-reader-android-core-portability.yml",
)

ANDROID = (
    "apps/chaptera-mobile-android/**",
    "crates/chaptera-mobile-reader-core/**",
    "crates/chaptera-mobile-reader-jni/**",
    ".github/workflows/mobile-reader-android-local-open.yml",
    ".github/workflows/mobile-reader-android-render.yml",
)

WEB = (
    "services/editor-api/web_real_acceptance_service.py",
    "tools/run_web_editor_real_acceptance.mjs",
    "apps/web/editor-shell-http-harness.html",
    "tools/sample_newsletter_move_producer.py",
    "tools/resolved_graph_scene_bridge_v1.py",
    "tools/adapt_viewer_scene_v1.py",
    "tools/validate_browser_acceptance_receipt.py",
    "tools/validate_web_acceptance_preflight.py",
    "tools/verify_editable_export_geometry.py",
    ".github/workflows/web-acceptance-real-chromium.yml",
)

LOCAL_PORTABLE = (
    "Start-Chaptera-Local.cmd",
    "tools/run_local_full_stack.py",
    "tools/run_local_real_editor.py",
    "tools/run_cloud_config_receipt.py",
    "tools/package_local_portable.py",
    "tools/adapt_viewer_scene_v1.py",
    "tools/resolved_graph_scene_bridge_v1.py",
    "tools/sample_newsletter_move_producer.py",
    "tools/scene_v1.py",
    "tools/validate_export_preview.py",
    "tools/verify_editable_export_geometry.py",
    "services/editor-api/**",
    "apps/web/**",
    "apps/chaptera-server/**",
    "packages/protocol/revision/v1/**",
    "packages/product/local-portable/**",
    "deploy/config/chaptera.prod.example.toml",
    ".github/workflows/chaptera-local-portable-windows.yml",
)

INSTALLER = (
    "packages/product/reader-portable/**",
    "installer/windows/chaptera-reader.iss",
    ".github/workflows/chaptera-reader-installer.yml",
)

PATH_IDENTITY = (
    "crates/chaptera-suite-handoff/**",
    "apps/chaptera-desktop/src/suite_handoff_cli.rs",
    "apps/chaptera-desktop/src/diagnostic_sweep.rs",
    "apps/chaptera-rescue/**",
    ".github/workflows/chaptera-win-path-identity.yml",
)

UPDATE_ACCEPT = (
    "crates/chaptera-update-engine/**",
    "crates/chaptera-update-orchestrator/**",
    "crates/chaptera-update-trust/**",
    "crates/chaptera-update-handoff/**",
    "apps/chaptera-desktop/src/main.rs",
    "installer/windows/chaptera-reader.iss",
    ".github/workflows/chaptera-win-update-accept.yml",
    ".github/workstream-scopes/chaptera-win-update-accept-01.md",
)

SHARED_DESKTOP_FILES = {
    "apps/chaptera-desktop/src/main.rs",
    "apps/chaptera-desktop/src/render_backend.rs",
    "apps/chaptera-desktop/src/diagnostic_sweep.rs",
    "apps/chaptera-desktop/src/image_decode_adapter.rs",
    "apps/chaptera-desktop/src/product_smoke.rs",
    "apps/chaptera-desktop/src/reader_product_ui.rs",
    "apps/chaptera-desktop/src/fallback_font.rs",
    "apps/chaptera-desktop/src/source_font.rs",
}


def classify(
    paths: list[str],
    dynamic_evidence_only_paths: set[str] | None = None,
) -> dict[str, bool]:
    evidence_only = EVIDENCE_ONLY_PATHS | (dynamic_evidence_only_paths or set())
    semantic_paths = [path for path in paths if path not in evidence_only]
    mapping = {
        "tier_a": TIER_A,
        "reader_windows_smoke": READER_WINDOWS_SMOKE,
        "reader_windows": READER_WINDOWS,
        "editor_windows": EDITOR_WINDOWS,
        "visual_oracle": VISUAL_ORACLE,
        "cloud_reference": CLOUD_REFERENCE,
        "virginia_page_role": VIRGINIA_PAGE_ROLE,
        "visual_batch01": VISUAL_BATCH01,
        "typography_golden": TYPOGRAPHY_GOLDEN,
        "android_core": ANDROID_CORE,
        "android": ANDROID,
        "web": WEB,
        "local_portable": LOCAL_PORTABLE,
        "installer": INSTALLER,
        "path_identity": PATH_IDENTITY,
        "update_accept": UPDATE_ACCEPT,
    }
    result = {
        scope: any(matches(path, patterns) for path in semantic_paths)
        for scope, patterns in mapping.items()
    }
    # The historical Virginia gate explicitly excluded probe-only binaries.
    # Preserve that boundary after routing it through the central PR DAG.
    virginia_paths = [
        path
        for path in semantic_paths
        if not path.startswith("vendor/producer-a/crates/pub-reader/src/bin/")
        and not path.startswith("vendor/producer-a/crates/pub-viewer/src/bin/")
    ]
    result["virginia_page_role"] = any(
        matches(path, VIRGINIA_PAGE_ROLE) for path in virginia_paths
    )
    # Full Reader Windows product/package acceptance supersedes the bounded
    # shared-core smoke when both surfaces are touched.
    if result["reader_windows"]:
        result["reader_windows_smoke"] = False

    # Full Editor Windows acceptance is product-surface validation, not a tax on
    # shared Reader/render plumbing. Shared desktop seams stay covered by Tier A.
    if any(
        path.startswith("apps/chaptera-desktop/")
        and path not in SHARED_DESKTOP_FILES
        for path in semantic_paths
    ):
        result["editor_windows"] = True
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--github-output")
    args = parser.parse_args()

    paths = changed_paths(args.base, args.head)
    dynamic_evidence_only = test_region_evidence_only_paths(args.base, args.head, paths)
    scopes = classify(paths, dynamic_evidence_only)
    receipt = {
        "schema": "chaptera.reader-pr-fanout.v1",
        "base_sha": args.base,
        "head_sha": args.head,
        "changed_paths": paths,
        "evidence_only_paths": sorted(
            (EVIDENCE_ONLY_PATHS & set(paths)) | dynamic_evidence_only
        ),
        "scopes": scopes,
    }

    out = Path(args.receipt)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt, indent=2, sort_keys=True))

    output_path = args.github_output or os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as fh:
            for scope in SCOPES:
                fh.write(f"{scope}={'true' if scopes[scope] else 'false'}\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
