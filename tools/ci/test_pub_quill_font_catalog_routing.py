#!/usr/bin/env python3
"""Fail-closed exact Quill FONT leaf owner routing contract."""
from pathlib import Path

from source_fanout_budget import admitted_by_patterns, extract_pull_request_paths

WORKFLOWS = Path(".github/workflows")
FONT = "vendor/producer-a/crates/pub-quill/src/typography/font.rs"
ROOT = "vendor/producer-a/crates/pub-quill/src/typography.rs"
READER = "vendor/producer-a/crates/pub-reader/src/lib.rs"


def patterns(name: str) -> list[str]:
    source = (WORKFLOWS / name).read_text(encoding="utf-8")
    result = extract_pull_request_paths(source, workflow=name)
    if result is None:
        raise SystemExit(f"{name}: missing explicit pull_request.paths")
    return result


if not Path(FONT).is_file():
    raise SystemExit(f"missing Quill FONT owner: {FONT}")

for name in ("paragraph-metrics-structural-receipt.yml", "quill-story-fdpp-exact.yml"):
    owner_paths = patterns(name)
    if admitted_by_patterns(FONT, owner_paths):
        raise SystemExit(f"{name}: unrelated FONT-only leaf must be excluded")
    if not admitted_by_patterns(ROOT, owner_paths):
        raise SystemExit(f"{name}: shared Quill typography root must remain admitted")
    if not admitted_by_patterns(READER, owner_paths) and name == "quill-story-fdpp-exact.yml":
        raise SystemExit(f"{name}: FDPP Reader root must remain admitted")

reader_paths = patterns("reader-pr-ci.yml")
if not admitted_by_patterns(FONT, reader_paths):
    raise SystemExit("Reader PR selective CI must continue to own FONT")
if not admitted_by_patterns(ROOT, reader_paths):
    raise SystemExit("Reader PR selective CI must continue to own shared Quill root")

print("pub-quill FONT catalog routing contract: PASS")
