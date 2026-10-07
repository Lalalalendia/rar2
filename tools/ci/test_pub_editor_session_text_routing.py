#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
SESSION = "vendor/producer-a/crates/pub-editor/src/session_text.rs"
NEG_SESSION = f"!{SESSION}"
TEST = "vendor/producer-a/crates/pub-editor/tests/story_text_session_v1.rs"
NEG_TEST = f"!{TEST}"

KEEP = {
    "cloud-export-executor-producer-v1.yml",
    "editor-desktop-text-session-restore-v1.yml",
    "pub-editor-fast-pr.yml",
    "w2-longform-fixed-pdf-current-revision.yml",
}

EXCLUDE = {
    "authoring-authored-stack-lifecycle-v1.yml",
    "authoring-authored-stack-runtime-v1.yml",
    "authoring-picture-frame-receipt.yml",
    "chaptera-server-package-integrity-v1.yml",
    "editable-source-image-export-v1.yml",
    "editable-typography-export-v1.yml",
    "editable-typography-scoped-preserved-v2.yml",
    "editor-build-once-pr.yml",
    "editor-desktop-textbox-restore-v1.yml",
    "editor-duplicate-rectangle-v1.yml",
    "migration-1050-corpus-matrix.yml",
    "reader-pr-ci.yml",
    "w2-project-fork-receipt-v1.yml",
}

def workflow(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")

def pr_block(name: str) -> str:
    body = workflow(name)
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
    assert NEG_SESSION not in body, (name, "unexpected exclusion")
    assert (
        SESSION in body
        or "vendor/producer-a/crates/pub-editor/src/**" in body
        or "vendor/producer-a/crates/pub-editor/**" in body
    ), (name, "text-session owner not admitted")

for name in EXCLUDE:
    assert NEG_SESSION in pr_block(name), (name, "missing text-session exclusion")

print("pub-editor session text routing contract: PASS")

TEST_EXCLUDE = {
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
}

fast = pr_block("pub-editor-fast-pr.yml")
assert NEG_TEST not in fast
assert (
    TEST in fast
    or "vendor/producer-a/crates/pub-editor/tests/**" in fast
    or "vendor/producer-a/crates/pub-editor/**" in fast
), "fast PR must admit the Story text-session integration test"

for name in TEST_EXCLUDE:
    assert NEG_TEST in pr_block(name), (name, "missing Story text-session test-only exclusion")

print("pub-editor session text test-only routing contract: PASS")
