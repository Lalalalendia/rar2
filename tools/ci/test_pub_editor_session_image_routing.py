#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
IMAGE = "vendor/producer-a/crates/pub-editor/src/session_image.rs"
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

for name in KEEP:
    body = text(name)
    assert NEG_IMAGE not in body, (name, "unexpected exclusion")
    assert (
        IMAGE in body
        or "vendor/producer-a/crates/pub-editor/src/**" in body
        or "vendor/producer-a/crates/pub-editor/**" in body
    ), (name, "image owner not admitted")

for name in EXCLUDE:
    assert NEG_IMAGE in text(name), (name, "missing image-owner exclusion")

print("pub-editor session image routing contract: PASS")
