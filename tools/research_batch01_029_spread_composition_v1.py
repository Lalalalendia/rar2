#!/usr/bin/env python3
from __future__ import annotations

import argparse
import itertools
import json
from pathlib import Path

from PIL import Image

from cloud_reader_visual_fingerprint_v1 import (
    GRID_H,
    GRID_W,
    compare_grid,
    reference_grid,
    sha256,
)

SCHEMA = "chaptera.batch01-029-spread-composition.v1"
EMU_PER_POINT = 12700.0
FOREGROUND_THRESHOLD = 24


def pil_grid(image: Image.Image) -> bytes:
    return image.convert("RGB").resize((GRID_W, GRID_H), Image.Resampling.BOX).tobytes()


def foreground_metrics(candidate: bytes, reference: bytes, threshold: int = FOREGROUND_THRESHOLD) -> dict:
    if len(candidate) != len(reference):
        raise ValueError("grid length mismatch")
    cells = len(candidate) // 3
    ref_count = 0
    candidate_count = 0
    intersection = 0
    for cell in range(cells):
        base = cell * 3
        ref_fg = max(255 - reference[base + channel] for channel in range(3)) >= threshold
        candidate_fg = max(255 - candidate[base + channel] for channel in range(3)) >= threshold
        ref_count += int(ref_fg)
        candidate_count += int(candidate_fg)
        intersection += int(ref_fg and candidate_fg)
    recall = intersection / ref_count if ref_count else 1.0
    precision = intersection / candidate_count if candidate_count else (1.0 if ref_count == 0 else 0.0)
    f1 = 2.0 * precision * recall / (precision + recall) if precision + recall else 0.0
    missing = 1.0 - recall if ref_count else 0.0
    extra = (candidate_count - intersection) / candidate_count if candidate_count else 0.0
    return {
        "foreground_recall": recall,
        "foreground_precision": precision,
        "foreground_f1": f1,
        "missing_reference_foreground_fraction": missing,
        "extra_candidate_foreground_fraction": extra,
        "foreground_loss": missing + extra,
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("browser_receipt", type=Path)
    ap.add_argument("page_role_receipt", type=Path)
    ap.add_argument("viewer_projection_receipt", type=Path)
    ap.add_argument("reference", type=Path)
    ap.add_argument("fixture")
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    browser = json.loads(args.browser_receipt.read_text(encoding="utf-8"))
    roles = json.loads(args.page_role_receipt.read_text(encoding="utf-8"))
    projection = json.loads(args.viewer_projection_receipt.read_text(encoding="utf-8"))
    reference = json.loads(args.reference.read_text(encoding="utf-8"))

    fixtures = [row for row in browser.get("results", []) if row.get("fixture") == args.fixture]
    pairs = [row for row in reference.get("pairs", []) if row.get("basename") == args.fixture]
    if len(fixtures) != 1 or len(pairs) != 1:
        raise ValueError("fixture/reference row is not unique")
    fixture = fixtures[0]
    pair = pairs[0]
    if fixture.get("rendered") is not True:
        raise ValueError("029 did not render")

    geometry = sorted(fixture.get("page_geometry", []), key=lambda row: row.get("order", -1))
    screenshots = sorted(fixture.get("screenshots", []), key=lambda row: row.get("page", -1))
    if len(geometry) != int(fixture.get("pages", -1)) or len(screenshots) != len(geometry):
        raise ValueError("browser PAGE geometry/screenshot cardinality mismatch")
    if projection.get("schema") != "chaptera.viewer-page-fingerprint-receipt.v1":
        raise ValueError("unsupported Viewer PAGE fingerprint receipt")
    projection_rows = sorted(
        projection.get("per_page", []),
        key=lambda row: row.get("viewer_page_index", -1),
    )
    if int(projection.get("viewer_page_count", -1)) != len(geometry) or len(projection_rows) != len(geometry):
        raise ValueError("Viewer/browser PAGE cardinality mismatch")

    role_rows = sorted(roles.get("pages", []), key=lambda row: row["document_ordinal"])
    by_fingerprint = {}
    for row in role_rows:
        fingerprint = row.get("page_identity_fingerprint_sha256")
        if not isinstance(fingerprint, str) or len(fingerprint) != 64:
            raise ValueError("source PAGE identity fingerprint missing")
        if fingerprint in by_fingerprint:
            raise ValueError("duplicate source PAGE identity fingerprint")
        by_fingerprint[fingerprint] = row

    joined = {}
    for output_index, (page_geometry, shot, projected) in enumerate(
        zip(geometry, screenshots, projection_rows),
        start=1,
    ):
        if shot.get("page") != output_index or projected.get("viewer_page_index") != output_index:
            raise ValueError("Viewer/browser output order drift")
        fingerprint = projected.get("page_identity_fingerprint_sha256")
        if not isinstance(fingerprint, str) or len(fingerprint) != 64:
            raise ValueError("Viewer PAGE fingerprint missing")
        role = by_fingerprint.get(fingerprint)
        if role is None:
            raise ValueError("browser PAGE identity is not present in source-role receipt")
        ordinal = int(role["document_ordinal"])
        png = args.browser_receipt.parent / shot["filename"]
        if sha256(png) != shot["sha256"]:
            raise ValueError("candidate screenshot SHA drift")
        if ordinal in joined:
            raise ValueError("duplicate joined document ordinal")
        joined[ordinal] = {
            "output_page": output_index,
            "identity_fingerprint_sha256": fingerprint,
            "width_emu": int(page_geometry["width_emu"]),
            "height_emu": int(page_geometry["height_emu"]),
            "png": png,
            "role": role,
        }

    expected_ordinals = [0, 1, 2, 3, 4, 5, 6, 8, 9]
    if sorted(joined) != expected_ordinals:
        raise ValueError(f"unexpected exact 029 PAGE ordinals: {sorted(joined)}")

    roots = [joined[0]["role"], joined[1]["role"]]
    if any(row.get("applied_master_seq_num") is not None for row in roots):
        raise ValueError("029 roots unexpectedly have applied masters")
    root_seq = [row["contents_seq_num"] for row in roots]
    expected_applied_pattern = [root_seq[1], root_seq[0], root_seq[1], root_seq[0], root_seq[1]]
    observed_applied_pattern = [joined[ordinal]["role"].get("applied_master_seq_num") for ordinal in range(2, 7)]
    if observed_applied_pattern != expected_applied_pattern:
        raise ValueError("029 primary/secondary applied relation drift")
    if joined[8]["role"].get("applied_master_seq_num") is not None or joined[9]["role"].get("applied_master_seq_num") is not None:
        raise ValueError("029 detached tail relation drift")

    if int(fixture["pages"]) != 9 or int(pair["reference_pages"]) != 2:
        raise ValueError("exact 029 cardinality drift")
    reference_pages = pair.get("pages", [])
    if len(reference_pages) != 2:
        raise ValueError("exact 029 reference fingerprint cardinality drift")
    refs = [reference_grid(row) for row in reference_pages]

    def load_image(ordinal: int) -> Image.Image:
        with Image.open(joined[ordinal]["png"]) as image:
            return image.convert("RGB").copy()

    def surface_grid(ordinals: tuple[int, ...]) -> bytes:
        images = [load_image(ordinal) for ordinal in ordinals]
        if len(images) == 1:
            return pil_grid(images[0])
        heights = {image.height for image in images}
        if len(heights) != 1:
            raise ValueError("half-PAGE raster heights differ")
        canvas = Image.new("RGB", (sum(image.width for image in images), images[0].height), (255, 255, 255))
        x = 0
        for image in images:
            canvas.paste(image, (x, 0))
            x += image.width
        return pil_grid(canvas)

    def surface_extent(ordinals: tuple[int, ...]) -> tuple[float, float]:
        widths = [joined[ordinal]["width_emu"] for ordinal in ordinals]
        heights = [joined[ordinal]["height_emu"] for ordinal in ordinals]
        if len(set(heights)) != 1:
            raise ValueError("half-PAGE source heights differ")
        return sum(widths) / EMU_PER_POINT, heights[0] / EMU_PER_POINT

    def compare_surface(ordinals: tuple[int, ...], reference_index: int) -> dict:
        candidate = surface_grid(ordinals)
        ref = refs[reference_index - 1]
        metrics = compare_grid(candidate, ref)
        foreground = foreground_metrics(candidate, ref)
        width_pt, height_pt = surface_extent(ordinals)
        ref_row = reference_pages[reference_index - 1]
        return {
            "source_document_ordinals": list(ordinals),
            "reference_page": reference_index,
            **metrics,
            **foreground,
            "candidate_media_width_pt": width_pt,
            "candidate_media_height_pt": height_pt,
            "reference_media_width_pt": float(ref_row["media_width_pt"]),
            "reference_media_height_pt": float(ref_row["media_height_pt"]),
            "media_width_delta_pt": width_pt - float(ref_row["media_width_pt"]),
            "media_height_delta_pt": height_pt - float(ref_row["media_height_pt"]),
        }

    def assignment(label: str, layout: tuple[int, int, int, int]) -> dict:
        rows = [
            compare_surface((layout[0], layout[1]), 1),
            compare_surface((layout[2], layout[3]), 2),
        ]
        return {
            "label": label,
            "layout_by_reference": [[layout[0], layout[1]], [layout[2], layout[3]]],
            "mean_foreground_loss": sum(row["foreground_loss"] for row in rows) / 2.0,
            "mean_changed_cell_fraction": sum(row["changed_cell_fraction"] for row in rows) / 2.0,
            "mean_abs_channel_delta": sum(row["mean_abs_channel_delta"] for row in rows) / 2.0,
            "max_abs_media_width_delta_pt": max(abs(row["media_width_delta_pt"]) for row in rows),
            "pages": rows,
        }

    single_controls = {
        "fail_open_prefix": [
            compare_surface((0,), 1),
            compare_surface((1,), 2),
        ],
        "primary_only": [
            compare_surface((3,), 1),
            compare_surface((5,), 2),
        ],
        "secondary_only": [
            compare_surface((2,), 1),
            compare_surface((4,), 2),
        ],
    }

    hypotheses = [
        assignment("secondary_then_primary_adjacent", (2, 3, 4, 5)),
        assignment("primary_then_secondary_adjacent", (3, 2, 5, 4)),
    ]
    discovery = [
        assignment("discovery", permutation)
        for permutation in itertools.permutations((2, 3, 4, 5))
    ]
    discovery.sort(
        key=lambda row: (
            row["mean_foreground_loss"],
            row["mean_changed_cell_fraction"],
            row["mean_abs_channel_delta"],
            row["layout_by_reference"],
        )
    )

    out = {
        "schema": SCHEMA,
        "fixture": args.fixture,
        "source_sha256": fixture.get("source_sha256"),
        "candidate_page_count": int(fixture["pages"]),
        "reference_page_count": int(pair["reference_pages"]),
        "joined_document_ordinals": expected_ordinals,
        "applied_pair_ordinals": [2, 3, 4, 5],
        "terminal_service_ordinal": 6,
        "single_page_controls": single_controls,
        "adjacent_spread_hypotheses": hypotheses,
        "best_discovery_assignments": discovery[:8],
        "claims": {
            "measurement_only": True,
            "raw_page_id_emitted": False,
            "raw_contents_seq_num_emitted": False,
            "story_text_emitted": False,
            "screenshots_emitted": False,
            "publisher_fingerprint_used_as_semantic_authority": False,
            "source_graph_mutated": False,
            "page_identity_join_uses_sha256_of_canonical_page_id": True,
            "spread_composition_is_raster_counterfactual_only": True,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "fixture": args.fixture,
        "single_controls": {
            key: [row["changed_cell_fraction"] for row in rows]
            for key, rows in single_controls.items()
        },
        "spread_hypotheses": [
            {
                "label": row["label"],
                "layout": row["layout_by_reference"],
                "mean_foreground_loss": row["mean_foreground_loss"],
                "mean_changed_cell_fraction": row["mean_changed_cell_fraction"],
                "max_abs_media_width_delta_pt": row["max_abs_media_width_delta_pt"],
            }
            for row in hypotheses
        ],
        "best_discovery": {
            "layout": discovery[0]["layout_by_reference"],
            "mean_foreground_loss": discovery[0]["mean_foreground_loss"],
            "mean_changed_cell_fraction": discovery[0]["mean_changed_cell_fraction"],
            "max_abs_media_width_delta_pt": discovery[0]["max_abs_media_width_delta_pt"],
        },
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
