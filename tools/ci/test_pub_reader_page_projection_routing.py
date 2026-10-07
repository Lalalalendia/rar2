#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
PAGE = "vendor/producer-a/crates/pub-reader/src/page_projection.rs"
NEG_PAGE = f"!{PAGE}"

spec = importlib.util.spec_from_file_location("reader_pr_fanout", CLASSIFIER)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


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


# Do not add new standalone PR consumers after the split: the source-fanout
# ratchet forbids widening an existing leaf. Master bridge compatibility is
# compiled inside Tier A; visual/page semantics stay on central owned scopes.
for name in (
    "master-projection-active-reader-bridge.yml",
    "publisher-visual-golden-supplemental.yml",
):
    assert PAGE not in pr_block(name), name

# Unrelated broad subscribers must fence the page-only seam.
for name in (
    "editable-source-image-export-v1.yml",
    "quill-story-fdpp-exact.yml",
):
    assert NEG_PAGE in pr_block(name), name

# Corpus/page-membership owners remain fail-closed for this semantic seam.
assert NEG_PAGE not in pr_block("migration-1050-corpus-matrix.yml")
assert PAGE in workflow("cloud-reader-reference-pairs.yml")
assert PAGE in workflow("publisher-visual-golden-batch01.yml")

scope = mod.classify([PAGE])
expected = {
    "tier_a": True,
    "reader_windows_smoke": True,
    "reader_windows": False,
    "editor_windows": False,
    "visual_oracle": True,
    "cloud_reference": True,
    "virginia_page_role": True,
    "visual_batch01": True,
    "typography_golden": False,
    "android_core": True,
    "android": False,
    "web": False,
    "local_portable": False,
    "installer": False,
    "path_identity": False,
    "update_accept": False,
}
for key, value in expected.items():
    assert scope[key] is value, (key, scope)

print("pub-reader page projection routing contract: PASS")
