#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
IMAGE = "vendor/producer-a/crates/pub-editor/src/session_image.rs"
IMAGE_CORE_PREFIX = "vendor/producer-a/crates/pub-editor-image-core/"
NEG_IMAGE = f"!{IMAGE}"

KEEP = {
    "authoring-picture-frame-receipt.yml",
    "editable-source-image-export-v1.yml",
    "editor-fixed-output-current-image-resources.yml",
    "pub-editor-fast-pr.yml",
    "w2-longform-fixed-pdf-current-revision.yml",
}

EXCLUDE = {
    "authoring-authored-stack-lifecycle-v1.yml",
    "authoring-authored-stack-runtime-v1.yml",
    "chaptera-server-package-integrity-v1.yml",
    "cloud-export-executor-producer-v1.yml",
    "editable-typography-export-v1.yml",
    "editable-typography-scoped-preserved-v2.yml",
    "editor-build-once-pr.yml",
    "editor-desktop-textbox-restore-v1.yml",
    "editor-duplicate-rectangle-v1.yml",
    "migration-1050-corpus-matrix.yml",
    "reader-pr-ci.yml",
    "w2-project-fork-receipt-v1.yml",
}


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


for name in KEEP:
    body = pr_block(name)
    assert NEG_IMAGE not in body, (name, "unexpected exclusion")
    assert (
        IMAGE in body
        or "vendor/producer-a/crates/pub-editor/src/**" in body
        or "vendor/producer-a/crates/pub-editor/**" in body
    ), (name, "image owner not admitted")
    if name != "pub-editor-fast-pr.yml":
        assert IMAGE_CORE_PREFIX not in body, (name, "image core must not own adapter acceptance")

fast = pr_block("pub-editor-fast-pr.yml")
assert "vendor/producer-a/crates/pub-editor-image-core/src/**" in fast
assert "vendor/producer-a/crates/pub-editor-image-core/Cargo.toml" in fast

for name in EXCLUDE:
    body = pr_block(name)
    assert NEG_IMAGE in body, (name, "missing image-owner exclusion")
    assert IMAGE_CORE_PREFIX not in body, (name, "image core leaked into unrelated owner")

print("pub-editor session image routing contract: PASS")
