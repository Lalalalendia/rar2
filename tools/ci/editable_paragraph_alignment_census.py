#!/usr/bin/env python3
"""Source-safe census of bounded full-Story paragraph alignment over an exact PUB corpus."""

from __future__ import annotations

import argparse
import collections
import json
import subprocess
from pathlib import Path


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
    alignment_counts = collections.Counter()
    source_rows: list[dict] = []
    edit_invalidation_files = 0

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

        count = int(data.get("eligible_count", 0))
        counts = {
            key: int(value)
            for key, value in data.get("alignment_counts", {}).items()
        }
        assert count == sum(counts.values()), (path, count, counts)
        if count == 0:
            continue

        eligible_files += 1
        eligible_stories += count
        alignment_counts.update(counts)
        if data.get("same_length_edit_invalidation_proven") is True:
            edit_invalidation_files += 1
        source_rows.append({
            "source_sha256": data["source_sha256"],
            "story_count": count,
            "alignment_counts": dict(sorted(counts.items())),
            "same_length_edit_invalidation_proven": data.get(
                "same_length_edit_invalidation_proven"
            ),
        })

    stories_per_source = collections.Counter(
        row["story_count"] for row in source_rows
    )
    alignment_source_counts = {
        alignment: sum(
            1 for row in source_rows
            if int(row["alignment_counts"].get(alignment, 0)) > 0
        )
        for alignment in sorted(alignment_counts)
    }

    receipt = {
        "schema": "chaptera.editable-paragraph-alignment-census.v1",
        "corpus_file_count": len(pubs),
        "admitted_file_count": admitted,
        "eligible_source_file_count": eligible_files,
        "eligible_story_count": eligible_stories,
        "alignment_story_counts": dict(sorted(alignment_counts.items())),
        "alignment_source_file_counts": alignment_source_counts,
        "stories_per_source_histogram": {
            str(story_count): file_count
            for story_count, file_count in sorted(stories_per_source.items())
        },
        "edit_invalidation_proven_source_count": edit_invalidation_files,
        "sources": sorted(source_rows, key=lambda row: row["source_sha256"]),
        "claims": {
            "measurement_only": True,
            "consumer_survival_proven_by_this_receipt": False,
            "loss_report_or_manifest_changed": False,
            "story_ids_recorded": False,
            "story_text_recorded": False,
            "supported_alignment_class": ["center", "right"],
        },
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({
        "corpus_file_count": receipt["corpus_file_count"],
        "admitted_file_count": receipt["admitted_file_count"],
        "eligible_source_file_count": receipt["eligible_source_file_count"],
        "eligible_story_count": receipt["eligible_story_count"],
        "alignment_story_counts": receipt["alignment_story_counts"],
        "alignment_source_file_counts": receipt["alignment_source_file_counts"],
        "stories_per_source_histogram": receipt["stories_per_source_histogram"],
        "edit_invalidation_proven_source_count": receipt[
            "edit_invalidation_proven_source_count"
        ],
    }, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
