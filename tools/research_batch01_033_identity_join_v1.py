#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path

from PIL import Image

SCHEMA = "chaptera.batch01-033-identity-join.v1"
THRESHOLDS = (12, 24, 48)


def foreground_counts(path: Path) -> dict[str, int]:
    with Image.open(path) as image:
        rgb = image.convert("RGB").resize((64, 64), Image.Resampling.BOX)
        pixels = list(rgb.getdata())
    result = {}
    for threshold in THRESHOLDS:
        result[str(threshold)] = sum(
            1
            for r, g, b in pixels
            if max(255 - r, 255 - g, 255 - b) >= threshold
        )
    return result


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("browser_receipt", type=Path)
    ap.add_argument("page_role_receipt", type=Path)
    ap.add_argument("viewer_projection_receipt", type=Path)
    ap.add_argument("fixture")
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    browser = json.loads(args.browser_receipt.read_text(encoding="utf-8"))
    roles = json.loads(args.page_role_receipt.read_text(encoding="utf-8"))
    projection = json.loads(args.viewer_projection_receipt.read_text(encoding="utf-8"))

    matches = [row for row in browser.get("results", []) if row.get("fixture") == args.fixture]
    if len(matches) != 1:
        raise ValueError("fixture row is not unique")
    fixture = matches[0]
    if fixture.get("rendered") is not True:
        raise ValueError("033 did not render")
    if int(fixture.get("pages", -1)) != 12:
        raise ValueError("exact 033 fail-open page count drift")

    role_rows = sorted(roles.get("pages", []), key=lambda row: row["document_ordinal"])
    if len(role_rows) != 12:
        raise ValueError("exact 033 source PAGE count drift")

    seq_to_ordinal = {
        int(row["contents_seq_num"]): int(row["document_ordinal"])
        for row in role_rows
    }
    fan_in = {int(row["document_ordinal"]): 0 for row in role_rows}
    for row in role_rows:
        master = row.get("applied_master_seq_num")
        if isinstance(master, int) and master in seq_to_ordinal:
            fan_in[seq_to_ordinal[master]] += 1

    source_by_fingerprint = {}
    for row in role_rows:
        fingerprint = row.get("page_identity_fingerprint_sha256")
        if not isinstance(fingerprint, str) or len(fingerprint) != 64:
            raise ValueError("source PAGE fingerprint missing")
        ordinal = int(row["document_ordinal"])
        master = row.get("applied_master_seq_num")
        if master is None:
            relation = "none"
        elif master in seq_to_ordinal:
            relation = f"page_ordinal:{seq_to_ordinal[master]}"
        else:
            relation = "external"
        oid_zero = row.get("oid_dword0") == 0 and row.get("oid_dword1") == 0
        source_by_fingerprint[fingerprint] = {
            "document_ordinal": ordinal,
            "master_relation": relation,
            "master_in_degree": fan_in[ordinal],
            "oid_class": "zero" if oid_zero else "nonzero",
            "shape_child_count": int(row.get("shape_child_count", 0)),
            "group_child_count": int(row.get("group_child_count", 0)),
        }

    geometry = sorted(fixture.get("page_geometry", []), key=lambda row: row.get("order", -1))
    screenshots = sorted(fixture.get("screenshots", []), key=lambda row: row.get("page", -1))
    if len(geometry) != 12 or len(screenshots) != 12:
        raise ValueError("browser PAGE receipt cardinality drift")
    if projection.get("schema") != "chaptera.viewer-page-fingerprint-receipt.v1":
        raise ValueError("unsupported Viewer PAGE fingerprint receipt")
    projection_rows = sorted(
        projection.get("per_page", []),
        key=lambda row: row.get("viewer_page_index", -1),
    )
    if int(projection.get("viewer_page_count", -1)) != 12 or len(projection_rows) != 12:
        raise ValueError("Viewer PAGE fingerprint receipt cardinality drift")

    joined = []
    seen_ordinals = set()
    for output_page, (geo, shot, projected) in enumerate(
        zip(geometry, screenshots, projection_rows),
        start=1,
    ):
        if shot.get("page") != output_page or projected.get("viewer_page_index") != output_page:
            raise ValueError("Viewer/browser output order drift")
        fingerprint = projected.get("page_identity_fingerprint_sha256")
        if not isinstance(fingerprint, str) or len(fingerprint) != 64:
            raise ValueError("Viewer PAGE fingerprint missing")
        source = source_by_fingerprint.get(fingerprint)
        if source is None:
            raise ValueError("browser PAGE identity missing from source receipt")
        ordinal = source["document_ordinal"]
        if ordinal in seen_ordinals:
            raise ValueError("duplicate source ordinal in Viewer output")
        seen_ordinals.add(ordinal)
        png = args.browser_receipt.parent / shot["filename"]
        joined.append({
            "viewer_output_page": output_page,
            "source_document_ordinal": ordinal,
            "source_page_identity_fingerprint_sha256": fingerprint,
            "master_relation": source["master_relation"],
            "master_in_degree": source["master_in_degree"],
            "oid_class": source["oid_class"],
            "source_shape_child_count": source["shape_child_count"],
            "source_group_child_count": source["group_child_count"],
            "foreground_cells": foreground_counts(png),
            "width_emu": int(geo["width_emu"]),
            "height_emu": int(geo["height_emu"]),
        })

    if sorted(seen_ordinals) != list(range(12)):
        raise ValueError(f"033 source ordinal coverage drift: {sorted(seen_ordinals)}")

    roots = [row for row in joined if row["master_relation"] == "none" and row["master_in_degree"] > 0]
    applied = [row for row in joined if row["master_relation"].startswith("page_ordinal:")]
    if len(roots) != 4:
        raise ValueError(f"expected 4 referenced roots, got {len(roots)}")
    root_ordinals = sorted(row["source_document_ordinal"] for row in roots)
    if root_ordinals != [0, 1, 2, 3]:
        raise ValueError(f"unexpected root ordinals: {root_ordinals}")

    one_to_one = []
    for root_ordinal in root_ordinals:
        members = [
            row
            for row in applied
            if row["master_relation"] == f"page_ordinal:{root_ordinal}"
        ]
        one_to_one.append({
            "root_ordinal": root_ordinal,
            "applied_member_ordinals": sorted(row["source_document_ordinal"] for row in members),
        })

    out = {
        "schema": SCHEMA,
        "fixture": args.fixture,
        "source_sha256": fixture.get("source_sha256"),
        "viewer_page_count": 12,
        "joined_rows": joined,
        "root_ordinals": root_ordinals,
        "applied_relation_groups": one_to_one,
        "claims": {
            "measurement_only": True,
            "raw_page_id_emitted": False,
            "raw_contents_seq_num_emitted": False,
            "raw_oid_values_emitted": False,
            "story_text_emitted": False,
            "screenshots_emitted": False,
            "page_identity_join_uses_sha256_of_canonical_page_id": True,
            "viewer_page_selection_changed": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(json.dumps({
        "root_ordinals": root_ordinals,
        "applied_relation_groups": one_to_one,
        "viewer_to_source": [
            {
                "viewer": row["viewer_output_page"],
                "ordinal": row["source_document_ordinal"],
                "relation": row["master_relation"],
                "fg24": row["foreground_cells"]["24"],
            }
            for row in joined
        ],
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
