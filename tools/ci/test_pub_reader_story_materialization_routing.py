#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
STORY = "vendor/producer-a/crates/pub-reader/src/story_materialization.rs"
NEG_STORY = f"!{STORY}"

spec = importlib.util.spec_from_file_location("reader_pr_fanout", CLASSIFIER)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


def workflow(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")


def event_block(name: str, event: str) -> str:
    text = workflow(name)
    marker = f"  {event}:\n"
    start = text.index(marker)
    tail = text[start + 1 :]
    boundaries = [
        tail.find(next_marker)
        for next_marker in (
            "\n  pull_request:\n",
            "\n  push:\n",
            "\n  schedule:\n",
            "\n  workflow_dispatch:\n",
            "\n  workflow_call:\n",
        )
        if tail.find(next_marker) >= 0
    ]
    end = start + 1 + (min(boundaries) if boundaries else len(tail))
    return text[start:end]


# Story materialization is visible text semantics. It must remain in both PR
# visual coverage and the main-branch Cloud/Batch01 push gates.
assert STORY in event_block("publisher-visual-golden-supplemental.yml", "pull_request")
assert STORY in event_block("cloud-reader-reference-pairs.yml", "push")
assert STORY in event_block("publisher-visual-golden-batch01.yml", "push")

# Quill provenance and migration remain real owners.
quill = event_block("quill-story-fdpp-exact.yml", "pull_request")
assert "vendor/producer-a/crates/pub-reader/src/**" in quill
assert NEG_STORY not in quill

migration = event_block("migration-1050-corpus-matrix.yml", "pull_request")
assert "vendor/producer-a/crates/pub-reader/src/**" in migration
assert NEG_STORY not in migration

# Story text materialization is not an image-export or physical/group/master owner.
image_export = event_block("editable-source-image-export-v1.yml", "pull_request")
assert NEG_STORY in image_export
assert STORY not in event_block("master-projection-active-reader-bridge.yml", "pull_request")
assert STORY not in event_block("grouped-source-stack-order-acceptance.yml", "pull_request")
assert STORY not in event_block("borderart-wire80-census.yml", "pull_request")

scope = mod.classify([STORY])
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

print("pub-reader Story materialization routing contract: PASS")
