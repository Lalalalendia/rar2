#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
import os
from pathlib import Path
import subprocess
from typing import Iterable


EVIDENCE_ONLY_PATHS = {
    "vendor/producer-a/crates/pub-viewer/src/bin/corpus-reader-receipt.rs",
}

SCOPES = (
    "tier_a",
    "reader_windows_smoke",
    "reader_windows",
    "editor_windows",
    "visual_oracle",
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
    "vendor/producer-a/crates/pub-reader/**",
    "vendor/producer-a/crates/pub-layout/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "crates/chaptera-viewer-render-plan/**",
    "apps/chaptera-desktop/src/render_backend.rs",
    "apps/chaptera-desktop/src/main.rs",
    "tools/acquire_carlton_march_pair.py",
    "tools/pdf_reference_diff_v1.py",
    "tools/reader_reference_pdf_raster_v1.py",
    "tools/reader_page_role_observation.py",
    ".github/workflows/carlton-reader-visual-oracle.yml",
)

TYPOGRAPHY_GOLDEN = (
    "vendor/producer-a/crates/pub-quill/**",
    "vendor/producer-a/crates/pub-reader/**",
    "vendor/producer-a/crates/pub-viewer/**",
    "crates/chaptera-viewer-render-plan/**",
    "apps/chaptera-desktop/src/render_backend.rs",
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
}


def classify(paths: list[str]) -> dict[str, bool]:
    semantic_paths = [path for path in paths if path not in EVIDENCE_ONLY_PATHS]
    mapping = {
        "tier_a": TIER_A,
        "reader_windows_smoke": READER_WINDOWS_SMOKE,
        "reader_windows": READER_WINDOWS,
        "editor_windows": EDITOR_WINDOWS,
        "visual_oracle": VISUAL_ORACLE,
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
    scopes = classify(paths)
    receipt = {
        "schema": "chaptera.reader-pr-fanout.v1",
        "base_sha": args.base,
        "head_sha": args.head,
        "changed_paths": paths,
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
