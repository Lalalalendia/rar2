#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
GUIDE = "vendor/producer-a/crates/pub-editor/src/ruler_guide_v1.rs"
GUIDE_TEST = "vendor/producer-a/crates/pub-editor/tests/ruler_guide_v1.rs"
SCHEMA = "vendor/producer-a/crates/pub-editor/src/project_schema_v1.rs"
ASSET = "vendor/producer-a/crates/pub-editor/src/editor_asset_v1.rs"
NEW_LEAVES = (GUIDE, GUIDE_TEST, SCHEMA, ASSET)


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


def positive(block: str, path: str) -> bool:
    return f'      - "{path}"' in block


def negative(block: str, path: str) -> bool:
    return f'      - "!{path}"' in block


fast = pr_block("pub-editor-fast-pr.yml")
assert "vendor/producer-a/crates/pub-editor/src/**" in fast
assert "vendor/producer-a/crates/pub-editor/tests/**" in fast

for name in (
    "editor-build-once-pr.yml",
    "w2-longform-fixed-pdf-current-revision.yml",
    "w2-project-fork-receipt-v1.yml",
    "editor-guide-projection-v1.yml",
    "cloud-revision-materializer-v1.yml",
):
    block = pr_block(name)
    assert positive(block, GUIDE), name
    assert positive(block, GUIDE_TEST), name

for name in (
    "editor-build-once-pr.yml",
    "w2-project-fork-receipt-v1.yml",
    "cloud-revision-materializer-v1.yml",
):
    assert positive(pr_block(name), SCHEMA), name

for name in ("editor-build-once-pr.yml", "w2-project-fork-receipt-v1.yml"):
    assert positive(pr_block(name), ASSET), name

for name in (
    "authoring-authored-stack-lifecycle-v1.yml",
    "authoring-authored-stack-runtime-v1.yml",
    "authoring-picture-frame-receipt.yml",
    "chaptera-server-package-integrity-v1.yml",
    "cloud-export-executor-producer-v1.yml",
    "editable-source-image-export-v1.yml",
    "editable-typography-export-v1.yml",
    "editable-typography-scoped-preserved-v2.yml",
    "editor-desktop-textbox-restore-v1.yml",
    "editor-duplicate-rectangle-v1.yml",
    "migration-1050-corpus-matrix.yml",
    "reader-pr-ci.yml",
):
    block = pr_block(name)
    for leaf in NEW_LEAVES:
        assert negative(block, leaf), (name, leaf)

print("pub-editor ruler-guide routing contract: PASS")
