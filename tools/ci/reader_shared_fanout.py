#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess

SCOPES = (
    "reader_windows",
    "editor_windows",
    "visual_oracle",
    "typography",
    "corpus",
    "android_render",
    "android_local",
    "local_portable",
    "web_acceptance",
)

def _matches(path: str, exact: set[str], prefixes: tuple[str, ...]) -> bool:
    return path in exact or any(path.startswith(prefix) for prefix in prefixes)

def classify(paths: list[str]) -> dict[str, bool]:
    p = sorted(dict.fromkeys(paths))
    out = {scope: False for scope in SCOPES}

    reader_exact = {
        "Cargo.toml", "Cargo.lock",
        "apps/chaptera-desktop/src/render_backend.rs",
        "apps/chaptera-desktop/src/main.rs",
        "vendor/producer-a/Cargo.toml",
        "vendor/producer-a/Cargo.lock",
    }
    reader_prefixes = (
        "crates/chaptera-viewer-render-plan/",
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
    out["reader_windows"] = any(_matches(x, reader_exact, reader_prefixes) for x in p)

    desktop_exact = {
        "apps/chaptera-desktop/src/render_backend.rs",
        "apps/chaptera-desktop/src/main.rs",
    }
    out["editor_windows"] = any(
        _matches(x, desktop_exact, ("crates/chaptera-viewer-render-plan/",)) for x in p
    )

    visual_prefixes = (
        "crates/chaptera-viewer-render-plan/",
        "vendor/producer-a/crates/pub-reader/",
        "vendor/producer-a/crates/pub-viewer/",
    )
    visual = any(_matches(x, desktop_exact, visual_prefixes) for x in p)
    out["visual_oracle"] = visual
    out["corpus"] = visual

    typography_prefixes = visual_prefixes + ("vendor/producer-a/crates/pub-quill/",)
    out["typography"] = any(_matches(x, desktop_exact, typography_prefixes) for x in p)

    out["android_render"] = any(
        x.startswith("crates/chaptera-viewer-render-plan/")
        or x == ".github/workflows/mobile-reader-android-render.yml"
        for x in p
    )
    out["android_local"] = any(
        x == ".github/workflows/mobile-reader-android-local-open.yml" for x in p
    )
    out["local_portable"] = any(
        x == ".github/workflows/chaptera-local-portable-windows.yml" for x in p
    )

    if ".github/workflows/chaptera-reader-windows.yml" in p:
        out["reader_windows"] = True
    if ".github/workflows/chaptera-desktop-windows.yml" in p:
        out["editor_windows"] = True
    if ".github/workflows/carlton-reader-visual-oracle.yml" in p:
        out["visual_oracle"] = True
    if ".github/workflows/reader-typography-golden.yml" in p:
        out["typography"] = True
    if ".github/workflows/reader-corpus-truth-v1.yml" in p:
        out["corpus"] = True

    out["web_acceptance"] = any(
        x.startswith("vendor/producer-a/crates/pub-reader/")
        or x.startswith("vendor/producer-a/crates/pub-viewer/")
        or x == ".github/workflows/web-acceptance-real-chromium.yml"
        for x in p
    )
    return out

def changed_paths(base: str, head: str) -> list[str]:
    return [
        x for x in subprocess.check_output(
            ["git", "diff", "--name-only", f"{base}...{head}"], text=True
        ).splitlines() if x
    ]

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    ap.add_argument("--github-output")
    ap.add_argument("--receipt", required=True)
    args = ap.parse_args()

    paths = changed_paths(args.base, args.head)
    scopes = classify(paths)
    receipt = {
        "schema": "chaptera.reader-shared-fanout.v1",
        "base_sha": args.base,
        "head_sha": args.head,
        "changed_paths": sorted(paths),
        "scopes": scopes,
    }
    out = Path(args.receipt)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt, indent=2, sort_keys=True))

    if args.github_output:
        with Path(args.github_output).open("a", encoding="utf-8") as fh:
            for key in SCOPES:
                fh.write(f"{key}={'true' if scopes[key] else 'false'}\n")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
