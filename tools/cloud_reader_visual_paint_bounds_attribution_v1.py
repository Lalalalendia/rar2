#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path

from cloud_reader_visual_fingerprint_v1 import (
    CELL_DELTA,
    GRID_H,
    GRID_W,
    image_grid,
    reference_grid,
)

WHITE_FLOOR = 245


def transformed_bbox(box: dict, transform: dict | None) -> tuple[float, float, float, float]:
    x0 = float(box["x"])
    y0 = float(box["y"])
    x1 = x0 + float(box["width"])
    y1 = y0 + float(box["height"])
    if not transform:
        return (x0, y0, x1, y1)
    a = float(transform["a"])
    b = float(transform["b"])
    c = float(transform["c"])
    d = float(transform["d"])
    tx = float(transform["tx"])
    ty = float(transform["ty"])
    corners = [
        (a * x + c * y + tx, b * x + d * y + ty)
        for x, y in ((x0, y0), (x1, y0), (x0, y1), (x1, y1))
    ]
    xs = [point[0] for point in corners]
    ys = [point[1] for point in corners]
    return (min(xs), min(ys), max(xs), max(ys))


def contains(box: tuple[float, float, float, float], x: float, y: float) -> bool:
    return box[0] <= x <= box[2] and box[1] <= y <= box[3]


def rgb(grid: bytes, cell: int) -> tuple[int, int, int]:
    base = cell * 3
    return (grid[base], grid[base + 1], grid[base + 2])


def near_white(value: tuple[int, int, int]) -> bool:
    return min(value) >= WHITE_FLOOR


def membership(boxes: dict[str, list[tuple[float, float, float, float]]], x: float, y: float) -> tuple[str, ...]:
    return tuple(
        name
        for name in sorted(boxes)
        if any(contains(box, x, y) for box in boxes[name])
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("candidate_png", type=Path)
    parser.add_argument("reference_fingerprint", type=Path)
    parser.add_argument("geometry_json", type=Path)
    parser.add_argument("fixture")
    parser.add_argument("page", type=int)
    args = parser.parse_args()

    candidate = image_grid(args.candidate_png)
    reference_payload = json.loads(args.reference_fingerprint.read_text(encoding="utf-8"))
    pairs = [pair for pair in reference_payload["pairs"] if pair["basename"] == args.fixture]
    if len(pairs) != 1:
        raise ValueError(f"fixture is not unique in reference: {args.fixture!r}")
    pages = [page for page in pairs[0]["pages"] if int(page["page"]) == args.page]
    if len(pages) != 1:
        raise ValueError(f"reference page is not unique: {args.page}")
    reference = reference_grid(pages[0])

    geometry = json.loads(args.geometry_json.read_text(encoding="utf-8"))
    if geometry["fixture"] != args.fixture or int(geometry["page"]) != args.page:
        raise ValueError("geometry identity mismatch")
    width_emu = float(geometry["width_emu"])
    height_emu = float(geometry["height_emu"])
    if width_emu <= 0 or height_emu <= 0:
        raise ValueError("page geometry must be positive")

    semantic_boxes: dict[str, list[tuple[float, float, float, float]]] = {}
    paint_boxes: dict[str, list[tuple[float, float, float, float]]] = {}
    node_counts = Counter()
    stroke_nodes = Counter()
    for node in geometry["nodes"]:
        category = str(node["class"])
        semantic_boxes.setdefault(category, []).append(
            transformed_bbox(node["semantic_bounds"], node.get("transform"))
        )
        paint_boxes.setdefault(category, []).append(
            transformed_bbox(node["paint_bounds"], node.get("transform"))
        )
        node_counts[category] += 1
        if float(node.get("stroke_width_emu") or 0) > 0:
            stroke_nodes[category] += 1

    old_counts = Counter()
    paint_counts = Counter()
    old_errors: dict[str, Counter] = {}
    paint_errors: dict[str, Counter] = {}
    old_none_omission = 0
    paint_none_omission = 0
    reclassified_none_omission = Counter()
    changed_total = 0

    for cell in range(GRID_W * GRID_H):
        candidate_rgb = rgb(candidate, cell)
        reference_rgb = rgb(reference, cell)
        if max(abs(candidate_rgb[i] - reference_rgb[i]) for i in range(3)) < CELL_DELTA:
            continue

        changed_total += 1
        if near_white(candidate_rgb) and not near_white(reference_rgb):
            error_type = "omission"
        elif not near_white(candidate_rgb) and near_white(reference_rgb):
            error_type = "excess"
        else:
            error_type = "both_nonwhite_or_tone"

        col = cell % GRID_W
        row = cell // GRID_W
        x = (col + 0.5) * width_emu / GRID_W
        y = (row + 0.5) * height_emu / GRID_H

        old_membership = membership(semantic_boxes, x, y)
        paint_membership = membership(paint_boxes, x, y)
        old_key = "+".join(old_membership) if old_membership else "none"
        paint_key = "+".join(paint_membership) if paint_membership else "none"

        old_counts[old_key] += 1
        paint_counts[paint_key] += 1
        old_errors.setdefault(old_key, Counter())[error_type] += 1
        paint_errors.setdefault(paint_key, Counter())[error_type] += 1

        if old_key == "none" and error_type == "omission":
            old_none_omission += 1
            if paint_key == "none":
                paint_none_omission += 1
            else:
                reclassified_none_omission[paint_key] += 1

    receipt = {
        "schema": "chaptera.visual-paint-bounds-attribution.v1",
        "fixture": args.fixture,
        "page": args.page,
        "grid": [GRID_W, GRID_H],
        "changed_cell_count": changed_total,
        "changed_cell_fraction": changed_total / (GRID_W * GRID_H),
        "node_counts": dict(sorted(node_counts.items())),
        "stroke_node_counts": dict(sorted(stroke_nodes.items())),
        "semantic_membership_counts": dict(
            sorted(old_counts.items(), key=lambda item: (-item[1], item[0]))
        ),
        "paint_membership_counts": dict(
            sorted(paint_counts.items(), key=lambda item: (-item[1], item[0]))
        ),
        "semantic_membership_error_types": {
            key: dict(sorted(old_errors[key].items()))
            for key, _ in sorted(old_counts.items(), key=lambda item: (-item[1], item[0]))
        },
        "paint_membership_error_types": {
            key: dict(sorted(paint_errors[key].items()))
            for key, _ in sorted(paint_counts.items(), key=lambda item: (-item[1], item[0]))
        },
        "semantic_none_omission_count": old_none_omission,
        "paint_none_omission_count": paint_none_omission,
        "semantic_none_omission_reclassified_by_paint_bounds": dict(
            sorted(reclassified_none_omission.items(), key=lambda item: (-item[1], item[0]))
        ),
        "claims": {
            "renderer_true_text_bounds": True,
            "renderer_true_picture_bounds": True,
            "shape_stroke_half_width_extent": True,
            "raw_candidate_pixels_emitted": False,
            "raw_reference_pixels_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    print("PAINT_BOUNDS_ATTRIBUTION " + json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
