#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
GEOMETRY = "vendor/producer-a/crates/pub-editor/src/session_geometry.rs"
NEG_GEOMETRY = f"!{GEOMETRY}"


def workflow(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")


def pr_block(name: str) -> str:
    text = workflow(name)
    start = text.index("  pull_request:\n")
    tail = text[start + 1 :]
    boundaries = [
        tail.find(marker)
        for marker in ("\n  push:\n", "\n  schedule:\n", "\n  workflow_dispatch:\n")
        if tail.find(marker) >= 0
    ]
    end = start + 1 + (min(boundaries) if boundaries else len(tail))
    return text[start:end]


for name in (
    "authoring-move-nodes-v1.yml",
    "editor-resize-node.yml",
    "resize-nodes-v1.yml",
):
    assert GEOMETRY in pr_block(name), name

assert "vendor/producer-a/crates/pub-editor/src/**" in pr_block("pub-editor-fast-pr.yml")

for name in (
    "authoring-authored-stack-lifecycle-v1.yml",
    "authoring-authored-stack-runtime-v1.yml",
    "authoring-picture-frame-receipt.yml",
    "chaptera-server-package-integrity-v1.yml",
    "cloud-export-executor-producer-v1.yml",
    "editable-source-image-export-v1.yml",
    "editable-typography-export-v1.yml",
    "editable-typography-scoped-preserved-v2.yml",
    "editor-build-once-pr.yml",
    "editor-desktop-textbox-restore-v1.yml",
    "editor-duplicate-rectangle-v1.yml",
    "migration-1050-corpus-matrix.yml",
    "reader-pr-ci.yml",
    "w2-longform-fixed-pdf-current-revision.yml",
    "w2-project-fork-receipt-v1.yml",
):
    assert NEG_GEOMETRY in pr_block(name), name

print("pub-editor session geometry routing contract: PASS")
