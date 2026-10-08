#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

CLASSIFIER = Path("tools/ci/reader_pr_fanout.py")
CONTENTS = "vendor/producer-a/crates/pub-reader/src/contents_access.rs"

spec = importlib.util.spec_from_file_location("reader_pr_fanout", CLASSIFIER)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)

scope = mod.classify([CONTENTS])

expected = {
    "tier_a": True,
    "reader_windows_smoke": True,
    "visual_oracle": True,
    "cloud_reference": False,
    "virginia_page_role": False,
    "visual_batch01": False,
    "typography_golden": False,
    "android_core": True,
}
for key, value in expected.items():
    assert scope[key] is value, (key, scope)

# Evidence from comment-only control #1941: these standalone workflows already
# register from their broad pub-reader PR paths and must not be expanded further:
# Quill exact FDPP, Editable Source Image Export, Migration 1050.
# The control did NOT register grouped source-stack, master projection,
# supplemental visual, or BorderArt; this routing change must not add them.
print("pub-reader Contents access routing contract: PASS")
