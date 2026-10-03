#!/usr/bin/env python3
from __future__ import annotations

import argparse
import itertools
import json
from pathlib import Path

from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid, sha256

SCHEMA = "chaptera.batch01-cross-page-matrix.v1"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("browser_receipt", type=Path)
    ap.add_argument("reference", type=Path)
    ap.add_argument("fixture")
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    browser = json.loads(args.browser_receipt.read_text(encoding="utf-8"))
    reference = json.loads(args.reference.read_text(encoding="utf-8"))

    fixtures = [row for row in browser.get("results", []) if row.get("fixture") == args.fixture]
    pairs = [row for row in reference.get("pairs", []) if row.get("basename") == args.fixture]
    if len(fixtures) != 1 or len(pairs) != 1:
        raise ValueError("fixture/reference row is not unique")

    fixture = fixtures[0]
    pair = pairs[0]
    if fixture.get("rendered") is not True:
        raise ValueError("target fixture did not render")

    candidate_pages = int(fixture["pages"])
    reference_pages = int(pair["reference_pages"])
    shots = fixture.get("screenshots", [])
    if len(shots) != candidate_pages:
        raise ValueError("candidate screenshot count mismatch")
    if len(pair.get("pages", [])) != reference_pages:
        raise ValueError("reference fingerprint page count mismatch")

    candidate_grids = []
    for index, shot in enumerate(shots):
        png = args.browser_receipt.parent / shot["filename"]
        if sha256(png) != shot["sha256"]:
            raise ValueError(f"candidate PNG identity drift at page {index + 1}")
        candidate_grids.append(image_grid(png))
    reference_grids = [reference_grid(page) for page in pair["pages"]]

    matrix = []
    by_pair = {}
    for candidate_index, candidate in enumerate(candidate_grids, start=1):
        for reference_index, ref in enumerate(reference_grids, start=1):
            metrics = compare_grid(candidate, ref)
            row = {
                "candidate_page": candidate_index,
                "reference_page": reference_index,
                **metrics,
            }
            matrix.append(row)
            by_pair[(candidate_index, reference_index)] = row

    best_per_reference = []
    for reference_index in range(1, reference_pages + 1):
        rows = [row for row in matrix if row["reference_page"] == reference_index]
        rows.sort(key=lambda row: (row["changed_cell_fraction"], row["mean_abs_channel_delta"], row["candidate_page"]))
        best_per_reference.append({
            "reference_page": reference_index,
            "best_candidates": rows[:5],
        })

    def assignment_score(candidate_indices: tuple[int, ...]) -> tuple[float, float]:
        rows = [
            by_pair[(candidate_page, reference_index)]
            for reference_index, candidate_page in enumerate(candidate_indices, start=1)
        ]
        return (
            sum(row["changed_cell_fraction"] for row in rows) / len(rows),
            sum(row["mean_abs_channel_delta"] for row in rows) / len(rows),
        )

    monotonic = []
    for combo in itertools.combinations(range(1, candidate_pages + 1), reference_pages):
        changed, mean_abs = assignment_score(combo)
        monotonic.append((changed, mean_abs, combo))
    monotonic.sort()

    unconstrained = []
    for perm in itertools.permutations(range(1, candidate_pages + 1), reference_pages):
        changed, mean_abs = assignment_score(perm)
        unconstrained.append((changed, mean_abs, perm))
    unconstrained.sort()

    def assignment_row(item: tuple[float, float, tuple[int, ...]]) -> dict:
        changed, mean_abs, candidate_indices = item
        return {
            "candidate_pages_by_reference": list(candidate_indices),
            "mean_changed_cell_fraction": changed,
            "mean_abs_channel_delta": mean_abs,
            "pages": [
                by_pair[(candidate_page, reference_index)]
                for reference_index, candidate_page in enumerate(candidate_indices, start=1)
            ],
        }

    out = {
        "schema": SCHEMA,
        "fixture": args.fixture,
        "source_sha256": fixture.get("source_sha256"),
        "candidate_page_count": candidate_pages,
        "reference_page_count": reference_pages,
        "best_per_reference": best_per_reference,
        "best_monotonic_assignments": [assignment_row(row) for row in monotonic[:10]],
        "best_unconstrained_assignments": [assignment_row(row) for row in unconstrained[:10]],
        "matrix": matrix,
        "claims": {
            "measurement_only": True,
            "fingerprint_used_as_semantic_authority": False,
            "pdf_used_as_visual_validation_only": True,
            "viewer_page_selection_changed": False,
            "raw_pub_bytes_emitted": False,
            "raw_pdf_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "fixture": args.fixture,
        "candidate_pages": candidate_pages,
        "reference_pages": reference_pages,
        "best_monotonic": out["best_monotonic_assignments"][0],
        "best_unconstrained": out["best_unconstrained_assignments"][0],
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
