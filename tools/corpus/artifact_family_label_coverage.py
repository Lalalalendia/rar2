#!/usr/bin/env python3
"""Summarize artifact-family label coverage over joined provenance rows."""
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

from artifact_family import annotate_rows


def build_report(rows: list[dict]) -> dict:
    annotated = annotate_rows(rows)
    families_by_sha: dict[str, set[str]] = defaultdict(set)
    joins_by_sha: dict[str, set[str]] = defaultdict(set)

    for row in annotated:
        sha = str(row.get("sha256") or "").casefold()
        if not sha:
            continue
        joins_by_sha[sha].add(str(row.get("provenance_join") or ""))
        for family in (row.get("artifact_families") or "").split(";"):
            family = family.strip()
            if family:
                families_by_sha[sha].add(family)

    all_shas = set(joins_by_sha)
    labeled = {sha for sha in all_shas if families_by_sha.get(sha)}
    family_counts = Counter()
    for sha in labeled:
        family_counts.update(families_by_sha[sha])

    template_by_sha: dict[str, set[str]] = defaultdict(set)
    for row in annotated:
        sha = str(row.get("sha256") or "").casefold()
        label = str(row.get("template_family") or "").strip()
        if sha and label:
            template_by_sha[sha].add(label)

    join_counts = Counter()
    for sha, joins in joins_by_sha.items():
        for join in joins:
            join_counts[join] += 1

    return {
        "schema": "chaptera.artifact-family-label-coverage.v1",
        "sha_denominator": len(all_shas),
        "labeled_sha_count": len(labeled),
        "unlabeled_sha_count": len(all_shas - labeled),
        "labeled_ratio": len(labeled) / len(all_shas) if all_shas else None,
        "template_family_sha_count": len(template_by_sha),
        "nonexclusive_family_assignment_count": sum(family_counts.values()),
        "family_sha_counts": dict(sorted(family_counts.items())),
        "provenance_join_sha_counts": dict(sorted(join_counts.items())),
        "unlabeled_sha256": sorted(all_shas - labeled),
        "boundary": (
            "Labels are conservative provenance/title-derived research strata. "
            "They are not market-share claims. Unlabeled means unknown, not absence."
        ),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--joined", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    rows = json.loads(args.joined.read_text(encoding="utf-8"))
    report = build_report(rows)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
