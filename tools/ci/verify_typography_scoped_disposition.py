#!/usr/bin/env python3
"""Verify request-scoped typography dispositions without relying on source text."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


FAMILY = "story.typography.font_family"
SIZE = "story.typography.font_size"
COLOR = "story.typography.color"
ALIGNMENT = "story.paragraph_alignment"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--proven-family", default="Montserrat")
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()

    probe = json.loads(args.probe.read_text())
    report = json.loads(args.report.read_text())

    proven_origins = {
        item["story_id"]
        for item in probe.get("items", [])
        if item.get("font_family", "").strip() == args.proven_family
    }
    typography_features = {FAMILY, SIZE}

    observed = {
        (item.get("origin"), item["feature"]): item["disposition"]
        for item in report.get("items", [])
        if item.get("feature") in typography_features | {COLOR, ALIGNMENT}
    }

    for origin in sorted(proven_origins):
        for feature in (FAMILY, SIZE):
            disposition = observed.get((origin, feature))
            if disposition != "preserved":
                raise AssertionError(
                    f"{feature} @ {origin} is {disposition!r}, expected preserved"
                )

    leaked = sorted(
        (origin, feature, disposition)
        for (origin, feature), disposition in observed.items()
        if feature in typography_features
        and disposition == "preserved"
        and origin not in proven_origins
    )
    if leaked:
        raise AssertionError(
            f"typography preservation leaked outside proven family: {leaked[:40]!r}"
        )

    unrelated = sorted(
        (origin, feature)
        for (origin, feature), disposition in observed.items()
        if feature in {COLOR, ALIGNMENT} and disposition == "preserved"
    )
    if unrelated:
        raise AssertionError(
            f"color/alignment unexpectedly promoted: {unrelated[:40]!r}"
        )

    promoted = sum(
        disposition == "preserved"
        for (origin, feature), disposition in observed.items()
        if origin in proven_origins and feature in typography_features
    )
    expected_promoted = len(proven_origins) * 2
    if promoted != expected_promoted:
        raise AssertionError(
            f"promoted feature count {promoted} != expected {expected_promoted}"
        )

    receipt = {
        "schema": "chaptera.editable-typography-scoped-disposition-check.v1",
        "source_sha256": probe.get("source_sha256"),
        "proven_family": args.proven_family,
        "proven_story_count": len(proven_origins),
        "promoted_feature_count": promoted,
        "unexpected_preserved_typography_count": len(leaked),
        "unexpected_color_alignment_preserved_count": len(unrelated),
        "story_text_recorded": False,
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
