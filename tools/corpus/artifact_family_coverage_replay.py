#!/usr/bin/env python3
"""Replay artifact-family coverage over an existing corpus artifact.

This deliberately re-runs the current conservative classifier over manifest
provenance while reusing an already verified capability census. It allows
taxonomy/coverage changes to be audited without reacquiring PUB bytes.
"""
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

from artifact_family import annotate_rows


def build_report(manifest_rows: list[dict], inventory: dict) -> dict:
    annotated = annotate_rows(manifest_rows)

    families_by_sha: dict[str, set[str]] = defaultdict(set)
    for row in annotated:
        sha = row.get("sha256")
        if not sha:
            continue
        for family in (row.get("artifact_families") or "").split(";"):
            family = family.strip()
            if family:
                families_by_sha[sha].add(family)

    capability_files = []
    seen = set()
    for row in inventory.get("files", []):
        sha = row.get("sha256")
        identity = ("sha256", sha) if sha else ("path", row.get("relative_path", ""))
        if identity in seen:
            continue
        seen.add(identity)
        if row.get("capabilities"):
            capability_files.append(row)

    family_counts = Counter()
    unlabeled = []
    multi_family_files = 0
    for row in capability_files:
        sha = row.get("sha256")
        families = sorted(families_by_sha.get(sha, ())) if sha else []
        if not families:
            unlabeled.append(
                {
                    "sha256": sha,
                    "relative_path": row.get("relative_path", ""),
                }
            )
            continue
        if len(families) > 1:
            multi_family_files += 1
        family_counts.update(families)

    denominator = len(capability_files)
    labeled = denominator - len(unlabeled)
    return {
        "schema_version": "publisher-artifact-family-coverage-replay-v0.1",
        "denominator_capability_files": denominator,
        "labeled_capability_files": labeled,
        "unlabeled_capability_files": len(unlabeled),
        "labeled_ratio": labeled / denominator if denominator else None,
        "multi_family_capability_files": multi_family_files,
        "nonexclusive_family_assignment_count": sum(family_counts.values()),
        "family_file_counts": dict(sorted(family_counts.items())),
        "unlabeled_identities": sorted(
            unlabeled,
            key=lambda item: (
                item.get("relative_path", ""),
                item.get("sha256") or "",
            ),
        ),
        "boundary": (
            "This report replays the current provenance/title-backed classifier "
            "over an existing manifest and reuses only capability-file membership "
            "from the supplied census. It is not a new acquisition run and broad "
            "artifact families are non-exclusive."
        ),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--capability-census", required=True, type=Path)
    args = parser.parse_args()

    manifest_rows = json.loads(args.manifest.read_text(encoding="utf-8"))
    inventory = json.loads(args.capability_census.read_text(encoding="utf-8"))
    print(json.dumps(build_report(manifest_rows, inventory), indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
