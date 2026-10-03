#!/usr/bin/env python3
"""Source-safe census of bounded editable typography over an exact PUB corpus."""

from __future__ import annotations

import argparse
import collections
import json
import subprocess
from pathlib import Path


def key_family_size(item: dict) -> str:
    return f"{item['font_family']}|{int(item['font_size_emu'])}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    pubs = sorted(args.corpus_root.glob("*.pub"))
    admitted = 0
    eligible_files = 0
    eligible_stories = 0
    families = collections.Counter()
    family_sizes = collections.Counter()
    montserrat_sources: list[dict] = []

    for path in pubs:
        run = subprocess.run(
            [str(args.probe), str(path)],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
        data = json.loads(run.stdout)
        if data.get("open_state") == "admitted":
            admitted += 1
        items = data.get("items", [])
        if not items:
            continue

        eligible_files += 1
        eligible_stories += len(items)
        for item in items:
            families[item["font_family"]] += 1
            family_sizes[key_family_size(item)] += 1

        montserrat = [
            item for item in items
            if item.get("font_family", "").strip().casefold() == "montserrat"
        ]
        if montserrat:
            sizes = collections.Counter(int(item["font_size_emu"]) for item in montserrat)
            montserrat_sources.append({
                "source_sha256": data["source_sha256"],
                "story_count": len(montserrat),
                "size_emu_counts": {str(size): count for size, count in sorted(sizes.items())},
            })

    montserrat_size_counts = collections.Counter()
    for row in montserrat_sources:
        for size, count in row["size_emu_counts"].items():
            montserrat_size_counts[int(size)] += int(count)

    story_count_per_source = collections.Counter(
        row["story_count"] for row in montserrat_sources
    )

    receipt = {
        "schema": "chaptera.editable-typography-promotion-census.v1",
        "corpus_file_count": len(pubs),
        "admitted_file_count": admitted,
        "eligible_source_file_count": eligible_files,
        "eligible_story_count": eligible_stories,
        "family_story_counts": dict(sorted(families.items())),
        "family_size_story_counts": dict(sorted(family_sizes.items())),
        "montserrat": {
            "source_file_count": len(montserrat_sources),
            "story_count": sum(row["story_count"] for row in montserrat_sources),
            "size_emu_story_counts": {
                str(size): count for size, count in sorted(montserrat_size_counts.items())
            },
            "stories_per_source_histogram": {
                str(count): files
                for count, files in sorted(story_count_per_source.items())
            },
            "single_story_source_count": story_count_per_source.get(1, 0),
            "multi_story_source_count": sum(
                files for count, files in story_count_per_source.items() if count > 1
            ),
            "sources": sorted(montserrat_sources, key=lambda row: row["source_sha256"]),
        },
        "claims": {
            "measurement_only": True,
            "consumer_survival_proven_by_this_receipt": False,
            "loss_report_or_manifest_changed": False,
            "story_text_recorded": False,
        },
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({
        "corpus_file_count": receipt["corpus_file_count"],
        "eligible_source_file_count": receipt["eligible_source_file_count"],
        "eligible_story_count": receipt["eligible_story_count"],
        "montserrat_source_file_count": receipt["montserrat"]["source_file_count"],
        "montserrat_story_count": receipt["montserrat"]["story_count"],
        "montserrat_size_emu_story_counts": receipt["montserrat"]["size_emu_story_counts"],
        "montserrat_stories_per_source_histogram": receipt["montserrat"]["stories_per_source_histogram"],
    }, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
