#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(".github/workflows")
CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
STORY_ANALYSIS = "vendor/producer-a/crates/pub-reader/src/story_frame_analysis.rs"
NEG_STORY_ANALYSIS = f"!{STORY_ANALYSIS}"

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


# Research-only correlation must not wake unrelated standalone corpus owners.
for name in (
    "quill-story-fdpp-exact.yml",
    "migration-1050-corpus-matrix.yml",
):
    assert NEG_STORY_ANALYSIS in pr_block(name), name

# The central Reader DAG retains compile/lint/source-free proof only.
scope = mod.classify([STORY_ANALYSIS])
expected = {
    "tier_a": True,
    "desktop_rustfmt": False,
    "reader_windows_smoke": False,
    "reader_windows": False,
    "editor_windows": False,
    "visual_oracle": False,
    "cloud_reference": False,
    "virginia_page_role": False,
    "visual_batch01": False,
    "typography_golden": False,
    "android_core": False,
    "android": False,
    "web": False,
    "local_portable": False,
    "installer": False,
    "path_identity": False,
    "update_accept": False,
}
for key, value in expected.items():
    assert scope[key] is value, (key, scope)

print("pub-reader story frame analysis routing contract: PASS")
