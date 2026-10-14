#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
SOURCE_PAINT = "vendor/producer-a/crates/pub-reader/src/source_paint_order.rs"
NEG_SOURCE_PAINT = f"!{SOURCE_PAINT}"

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


# Direct semantic owners retained after extraction.
for name in (
    "grouped-source-stack-order-acceptance.yml",
    "master-projection-active-reader-bridge.yml",
    "publisher-visual-golden-supplemental.yml",
):
    assert SOURCE_PAINT in pr_block(name), name

# Unrelated historical broad subscribers must fence the leaf.
for name in (
    "editable-source-image-export-v1.yml",
    "quill-story-fdpp-exact.yml",
    "migration-1050-corpus-matrix.yml",
):
    assert NEG_SOURCE_PAINT in pr_block(name), name

# Reusable/main-push visual consumers remain explicit owners.
assert SOURCE_PAINT in workflow("cloud-reader-reference-pairs.yml")
assert SOURCE_PAINT in workflow("publisher-visual-golden-batch01.yml")

scope = mod.classify([SOURCE_PAINT])
expected = {
    "tier_a": True,
    "desktop_rustfmt": False,
    "reader_windows_smoke": True,
    "reader_windows": False,
    "editor_windows": False,
    "visual_oracle": True,
    "cloud_reference": True,
    "virginia_page_role": False,
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

print("pub-reader source paint order routing contract: PASS")
