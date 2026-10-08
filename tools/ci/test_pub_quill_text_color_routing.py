#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
COLOR = "vendor/producer-a/crates/pub-quill/src/typography/color.rs"
NEG_COLOR = f"!{COLOR}"


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


if not Path(COLOR).is_file():
    raise SystemExit(f"missing extracted Quill text-color owner: {COLOR}")

for workflow in (
    "paragraph-metrics-structural-receipt.yml",
    "quill-story-fdpp-exact.yml",
):
    body = pr_block(workflow)
    if "vendor/producer-a/crates/pub-quill/src/**" not in body:
        raise SystemExit(f"{workflow}: expected broad Quill admission missing")
    if NEG_COLOR not in body:
        raise SystemExit(f"{workflow}: text-color leaf must stay excluded")

reader = pr_block("reader-pr-ci.yml")
if NEG_COLOR in reader:
    raise SystemExit("reader-pr-ci.yml: text-color owner unexpectedly excluded")
if "vendor/producer-a/crates/pub-quill/**" not in reader:
    raise SystemExit("reader-pr-ci.yml: central Reader owner must remain fail-closed")

print("pub-quill text-color routing contract: PASS")
