#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
TYPOGRAPHY = "vendor/producer-a/crates/pub-reader/src/typography_projection.rs"
NEG_TYPOGRAPHY = f"!{TYPOGRAPHY}"

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


# Typography projection is not a generic Quill provenance or image-export owner.
assert NEG_TYPOGRAPHY in pr_block("quill-story-fdpp-exact.yml")
assert NEG_TYPOGRAPHY in pr_block("editable-source-image-export-v1.yml")

# Migration still exercises editable-output projection, so keep it fail-closed.
migration = pr_block("migration-1050-corpus-matrix.yml")
assert "vendor/producer-a/crates/pub-reader/src/**" in migration
assert NEG_TYPOGRAPHY not in migration

# Supplemental visual is a true direct typography consumer.
assert TYPOGRAPHY in pr_block("publisher-visual-golden-supplemental.yml")

scope = mod.classify([TYPOGRAPHY])
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

print("pub-reader typography projection routing contract: PASS")
