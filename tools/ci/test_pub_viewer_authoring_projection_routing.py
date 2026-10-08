#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
AUTHORING = "vendor/producer-a/crates/pub-viewer/src/authoring_projection.rs"
BROAD = "vendor/producer-a/crates/pub-viewer/src/**"
NEG_AUTHORING = f"!{AUTHORING}"


def text(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")


def require(name: str, needle: str) -> None:
    if needle not in text(name):
        raise SystemExit(f"{name}: missing routing owner path: {needle}")


def forbid(name: str, needle: str) -> None:
    if needle in text(name):
        raise SystemExit(f"{name}: unexpected routing path remains: {needle}")


def require_broad_then_exclusion(name: str) -> None:
    body = text(name)
    if BROAD not in body or NEG_AUTHORING not in body:
        raise SystemExit(
            f"{name}: expected broad pub-viewer owner plus authoring exclusion"
        )
    if body.index(BROAD) > body.index(NEG_AUTHORING):
        raise SystemExit(
            f"{name}: authoring exclusion must follow the broad src pattern"
        )


if not Path(AUTHORING).is_file():
    raise SystemExit(f"missing extracted authoring projection owner: {AUTHORING}")

for workflow in (
    "desktop-pub-open-worker.yml",
    "master-projection-active-reader-bridge.yml",
    "publisher-visual-golden-supplemental.yml",
):
    require_broad_then_exclusion(workflow)

# The central Reader classifier remains fail-closed for any real pub-viewer source edit.
require("reader-pr-ci.yml", "vendor/producer-a/crates/pub-viewer/**")
forbid("reader-pr-ci.yml", NEG_AUTHORING)

# The direct current-fixed-output consumer explicitly owns the extracted seam.
require("editor-current-fixed-pdf-resource-input.yml", AUTHORING)
forbid("editor-current-fixed-pdf-resource-input.yml", NEG_AUTHORING)

print("pub-viewer authoring projection routing contract: PASS")
