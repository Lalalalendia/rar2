#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
GROUPED = "vendor/producer-a/crates/pub-reader/src/grouped_projection.rs"
NEG_GROUPED = f"!{GROUPED}"

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


# Grouped geometry changes can alter visible grouped images/primitives/tables and
# whether grouped materialized nodes participate in source stack order.
assert GROUPED in pr_block("publisher-visual-golden-supplemental.yml")
assert GROUPED in pr_block("grouped-source-stack-order-acceptance.yml")

# Migration remains fail-closed because grouped materialization/geometry can
# change editable export. Source Image Export owns payload identity/content
# transform, while grouped geometry is already covered by migration + grouped
# stack + supplemental and must not pay a redundant standalone fanout.
migration = pr_block("migration-1050-corpus-matrix.yml")
assert "vendor/producer-a/crates/pub-reader/src/**" in migration
assert NEG_GROUPED not in migration
assert NEG_GROUPED in pr_block("editable-source-image-export-v1.yml")

# Quill provenance, PAGE-master relation semantics and BorderArt wire census do
# not own grouped ChildAnchor/FSPGR projection.
assert NEG_GROUPED in pr_block("quill-story-fdpp-exact.yml")
assert GROUPED not in pr_block("master-projection-active-reader-bridge.yml")
assert GROUPED not in pr_block("borderart-wire80-census.yml")

scope = mod.classify([GROUPED])
expected = {
    "tier_a": True,
    "desktop_rustfmt": False,
    "reader_windows_smoke": True,
    "reader_windows": False,
    "editor_windows": False,
    "visual_oracle": True,
    "cloud_reference": False,
    "virginia_page_role": False,
    "visual_batch01": False,
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

print("pub-reader grouped projection routing contract: PASS")
