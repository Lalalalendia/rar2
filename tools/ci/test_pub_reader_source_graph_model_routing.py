#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
MODEL = "vendor/producer-a/crates/pub-reader/src/source_graph_model.rs"
NEG_MODEL = f"!{MODEL}"

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


# This leaf owns source-model declarations, not grouped/master/render behavior.
# Those behavioral acceptances stay attached to their implementation owners.
assert MODEL not in pr_block("grouped-source-stack-order-acceptance.yml")
assert MODEL not in pr_block("master-projection-active-reader-bridge.yml")
assert MODEL not in pr_block("publisher-visual-golden-supplemental.yml")

# Editable source image export and Migration intentionally retain broad Reader
# ownership because asset/export projection consumes PubSourceGraph.
source_image = pr_block("editable-source-image-export-v1.yml")
assert "vendor/producer-a/crates/pub-reader/src/**" in source_image
assert NEG_MODEL not in source_image

migration = pr_block("migration-1050-corpus-matrix.yml")
assert "vendor/producer-a/crates/pub-reader/src/**" in migration
assert NEG_MODEL not in migration

# Story provenance and physical BorderArt census do not own this model seam.
assert NEG_MODEL in pr_block("quill-story-fdpp-exact.yml")
assert MODEL not in pr_block("borderart-wire80-census.yml")

scope = mod.classify([MODEL])
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
    "typography_golden": True,
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

print("pub-reader source graph model routing contract: PASS")
