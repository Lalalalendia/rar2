#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
IMAGES = "vendor/producer-a/crates/pub-viewer/src/images.rs"
NEG_IMAGES = f"!{IMAGES}"


def text(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")


def pr_block(name: str) -> str:
    body = text(name)
    start = body.index("  pull_request:\n")
    tail = body[start + 1 :]
    boundaries = [
        tail.find(marker)
        for marker in ("\n  push:\n", "\n  schedule:\n", "\n  workflow_dispatch:\n")
        if tail.find(marker) >= 0
    ]
    end = start + 1 + (min(boundaries) if boundaries else len(tail))
    return body[start:end]


if not Path(IMAGES).is_file():
    raise SystemExit(f"missing extracted Viewer image owner: {IMAGES}")

for workflow in (
    "desktop-pub-open-worker.yml",
    "master-projection-active-reader-bridge.yml",
    "publisher-visual-golden-supplemental.yml",
):
    body = pr_block(workflow)
    if NEG_IMAGES in body:
        raise SystemExit(f"{workflow}: Viewer image owner unexpectedly excluded")
    if (
        IMAGES not in body
        and "vendor/producer-a/crates/pub-viewer/src/**" not in body
        and "vendor/producer-a/crates/pub-viewer/**" not in body
    ):
        raise SystemExit(f"{workflow}: Viewer image owner not admitted")

reader = pr_block("reader-pr-ci.yml")
if NEG_IMAGES in reader:
    raise SystemExit("reader-pr-ci.yml: Viewer image owner unexpectedly excluded")
if "vendor/producer-a/crates/pub-viewer/**" not in reader:
    raise SystemExit("reader-pr-ci.yml: central Reader owner must remain fail-closed")

print("pub-viewer image routing contract: PASS")
