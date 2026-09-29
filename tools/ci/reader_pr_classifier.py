#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess


VENDOR_SHARED_PREFIXES = (
    "vendor/producer-a/crates/pub-core/",
    "vendor/producer-a/crates/pub-cfb/",
    "vendor/producer-a/crates/pub-contents/",
    "vendor/producer-a/crates/pub-escher/",
    "vendor/producer-a/crates/pub-model/",
    "vendor/producer-a/crates/pub-quill/",
    "vendor/producer-a/crates/pub-reader/",
    "vendor/producer-a/crates/pub-layout/",
    "vendor/producer-a/crates/pub-viewer/",
)

MOBILE_PREFIXES = (
    "apps/chaptera-mobile-android/",
    "crates/chaptera-mobile-reader-core/",
    "crates/chaptera-mobile-reader-jni/",
)

WEB_PREFIXES = (
    "apps/web/",
    "services/editor-api/",
)

PORTABLE_PREFIXES = (
    "apps/web/",
    "apps/chaptera-server/",
    "services/editor-api/",
    "packages/protocol/revision/v1/",
    "packages/product/local-portable/",
)

ROOT_SHARED = {
    "Cargo.toml",
    "Cargo.lock",
    "vendor/producer-a/Cargo.toml",
    "vendor/producer-a/Cargo.lock",
}

DESKTOP_READER_EXACT = {
    "apps/chaptera-desktop/src/main.rs",
    "apps/chaptera-desktop/src/render_backend.rs",
}


def changed_paths(base: str, head: str) -> list[str]:
    return sorted(
        dict.fromkeys(
            path
            for path in subprocess.check_output(
                ["git", "diff", "--name-only", f"{base}...{head}"], text=True
            ).splitlines()
            if path
        )
    )


def _prefix(paths: list[str], prefixes: tuple[str, ...]) -> bool:
    return any(path.startswith(prefix) for path in paths for prefix in prefixes)


def _exact(paths: list[str], exact: set[str]) -> bool:
    return any(path in exact for path in paths)


def classify(paths: list[str], *, force_all: bool = False) -> dict[str, bool]:
    if force_all:
        return {
            "tier_a_required": True,
            "reader_windows": True,
            "desktop_windows": True,
            "visual_oracle": True,
            "typography": True,
            "android_core": True,
            "android_local_open": True,
            "android_render": True,
            "portable": True,
            "web_chromium": True,
            "installer": True,
            "updater": True,
        }

    shared_vendor = _prefix(paths, VENDOR_SHARED_PREFIXES)
    reader_runtime_vendor = _prefix(
        paths,
        (
            "vendor/producer-a/crates/pub-reader/",
            "vendor/producer-a/crates/pub-layout/",
            "vendor/producer-a/crates/pub-viewer/",
        ),
    )
    render_plan = _prefix(paths, ("crates/chaptera-viewer-render-plan/",))
    desktop_reader = _exact(paths, DESKTOP_READER_EXACT)
    mobile = _prefix(paths, MOBILE_PREFIXES)
    web = _prefix(paths, WEB_PREFIXES)
    portable = _prefix(paths, PORTABLE_PREFIXES)

    tier_a_required = (
        shared_vendor
        or render_plan
        or desktop_reader
        or _exact(paths, ROOT_SHARED)
        or _prefix(paths, ("tools/ci/",))
        or _exact(
            paths,
            {
                ".github/workflows/reader-consumer-preflight.yml",
            },
        )
    )

    reader_windows = (
        render_plan
        or desktop_reader
        or _prefix(
            paths,
            (
                "packages/product/reader-portable/",
                "packages/product/desktop-suite/",
                "crates/chaptera-update-engine/",
                "crates/chaptera-update-orchestrator/",
                "crates/chaptera-update-handoff/",
            ),
        )
        or _exact(
            paths,
            {
                "tools/package_reader_portable.py",
                "tools/test_package_reader_portable.py",
                ".github/workflows/chaptera-reader-windows.yml",
            },
        )
    )

    desktop_windows = (
        _prefix(paths, ("apps/chaptera-desktop/", "crates/chaptera-scene-instance/"))
        or render_plan
        or _prefix(
            paths,
            (
                "vendor/producer-a/crates/pub-reader/",
                "vendor/producer-a/crates/pub-viewer/",
                "packages/product/editor-live-trial/",
                "packages/product/desktop-suite/",
                "packages/protocol/editor-agent-control/",
            ),
        )
        or _exact(
            paths,
            {
                "tools/package_editor_live_trial.py",
                "tools/build_editor_live_trial_package_receipt.py",
                "tools/validate_editor_live_trial_package_receipt.py",
                "tools/run_editor_live_trial_package_smoke.py",
                "tools/validate_editor_desktop_ux_snapshots.py",
                "tools/run_local_editor_desktop_vertical.py",
                "tools/validate_chaptera_desktop_suite_manifest.py",
                ".github/workflows/chaptera-desktop-windows.yml",
            },
        )
    )

    visual_oracle = (
        _prefix(
            paths,
            (
                "crates/pub-presentation-profile/",
                "vendor/producer-a/crates/pub-quill/",
                "vendor/producer-a/crates/pub-reader/",
                "vendor/producer-a/crates/pub-layout/",
                "vendor/producer-a/crates/pub-viewer/",
            ),
        )
        or render_plan
        or desktop_reader
        or _exact(
            paths,
            {
                "tools/acquire_carlton_march_pair.py",
                "tools/pdf_reference_diff_v1.py",
                "tools/reader_reference_pdf_raster_v1.py",
                ".github/workflows/carlton-reader-visual-oracle.yml",
            },
        )
    )

    typography = (
        _prefix(
            paths,
            (
                "vendor/producer-a/crates/pub-quill/",
                "vendor/producer-a/crates/pub-reader/",
                "vendor/producer-a/crates/pub-viewer/",
            ),
        )
        or render_plan
        or desktop_reader
        or _exact(paths, {".github/workflows/reader-typography-golden.yml"})
    )

    android_core = (
        reader_runtime_vendor
        or render_plan
        or _prefix(paths, ("crates/chaptera-mobile-reader-core/",))
        or _exact(paths, {".github/workflows/mobile-reader-android-core-portability.yml"})
    )

    android_local_open = (
        mobile
        or _exact(paths, {".github/workflows/mobile-reader-android-local-open.yml"})
    )
    android_render = (
        mobile
        or _exact(paths, {".github/workflows/mobile-reader-android-render.yml"})
    )

    portable_scope = (
        portable
        or _prefix(paths, ("packages/product/local-portable/",))
        or _exact(
            paths,
            {
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
                "deploy/config/chaptera.prod.example.toml",
                ".github/workflows/chaptera-local-portable-windows.yml",
            },
        )
    )

    web_chromium = (
        web
        or _exact(
            paths,
            {
                "tools/run_web_editor_real_acceptance.mjs",
                "tools/sample_newsletter_move_producer.py",
                "tools/resolved_graph_scene_bridge_v1.py",
                "tools/adapt_viewer_scene_v1.py",
                "tools/validate_browser_acceptance_receipt.py",
                "tools/validate_web_acceptance_preflight.py",
                "tools/verify_editable_export_geometry.py",
                ".github/workflows/web-acceptance-real-chromium.yml",
            },
        )
    )

    installer = (
        _prefix(paths, ("packages/product/reader-portable/",))
        or _exact(
            paths,
            {
                "installer/windows/chaptera-reader.iss",
                ".github/workflows/chaptera-reader-installer.yml",
            },
        )
    )

    updater = (
        _prefix(
            paths,
            (
                "crates/chaptera-update-engine/",
                "crates/chaptera-update-orchestrator/",
                "crates/chaptera-update-trust/",
                "crates/chaptera-update-handoff/",
            ),
        )
        or _exact(
            paths,
            {
                "installer/windows/chaptera-reader.iss",
                ".github/workflows/chaptera-win-update-accept.yml",
                ".github/workstream-scopes/chaptera-win-update-accept-01.md",
            },
        )
    )

    return {
        "tier_a_required": tier_a_required,
        "reader_windows": reader_windows,
        "desktop_windows": desktop_windows,
        "visual_oracle": visual_oracle,
        "typography": typography,
        "android_core": android_core,
        "android_local_open": android_local_open,
        "android_render": android_render,
        "portable": portable_scope,
        "web_chromium": web_chromium,
        "installer": installer,
        "updater": updater,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--github-output")
    parser.add_argument("--force-all", action="store_true")
    args = parser.parse_args()

    paths = changed_paths(args.base, args.head)
    fanout = classify(paths, force_all=args.force_all)
    receipt = {
        "schema": "chaptera.reader-pr-classifier.v1",
        "base_sha": args.base,
        "head_sha": args.head,
        "changed_paths": paths,
        "fanout": fanout,
    }

    out = Path(args.receipt)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as fh:
            for key, value in fanout.items():
                fh.write(f"{key}={'true' if value else 'false'}\n")

    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
