#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
from collections import Counter
from pathlib import Path

from cloud_reader_visual_fingerprint_v1 import (
    CELL_DELTA,
    GRID_H,
    GRID_W,
    image_grid,
    reference_grid,
)

CLASSES = ("picture", "text", "ellipse", "other_painted", "empty_emf")
WHITE_FLOOR = 245
DISTANCE_THRESHOLDS = (0.5, 1.0, 2.0, 4.0, 8.0)


def raw_bbox(node: dict) -> tuple[float, float, float, float]:
    bounds = node["bounds"]
    x0 = float(bounds["x"])
    y0 = float(bounds["y"])
    return (
        x0,
        y0,
        x0 + float(bounds["width"]),
        y0 + float(bounds["height"]),
    )


def transformed_bbox(node: dict) -> tuple[float, float, float, float]:
    x0, y0, x1, y1 = raw_bbox(node)
    transform = node.get("transform")
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


def distance_cells(
    box: tuple[float, float, float, float],
    x: float,
    y: float,
    cell_width: float,
    cell_height: float,
) -> float:
    if x < box[0]:
        dx = box[0] - x
    elif x > box[2]:
        dx = x - box[2]
    else:
        dx = 0.0

    if y < box[1]:
        dy = box[1] - y
    elif y > box[3]:
        dy = y - box[3]
    else:
        dy = 0.0

    return math.hypot(dx / cell_width, dy / cell_height)


def rgb(grid: bytes, cell: int) -> tuple[int, int, int]:
    base = cell * 3
    return (grid[base], grid[base + 1], grid[base + 2])


def near_white(value: tuple[int, int, int]) -> bool:
    return min(value) >= WHITE_FLOOR


def distance_band(distance: float) -> str:
    for threshold in DISTANCE_THRESHOLDS:
        if distance <= threshold:
            return f"le_{threshold:g}"
    return "gt_8"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("candidate_png", type=Path)
    parser.add_argument("reference_fingerprint", type=Path)
    parser.add_argument("geometry_json", type=Path)
    parser.add_argument("fixture")
    parser.add_argument("page", type=int)
    args = parser.parse_args()

    candidate = image_grid(args.candidate_png)
    reference_payload = json.loads(
        args.reference_fingerprint.read_text(encoding="utf-8")
    )
    pairs = [
        pair for pair in reference_payload["pairs"] if pair["basename"] == args.fixture
    ]
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
    cell_width = width_emu / GRID_W
    cell_height = height_emu / GRID_H

    transformed = []
    raw = []
    for node in geometry["nodes"]:
        category = node["class"]
        if category not in CLASSES:
            raise ValueError(f"unsupported node class: {category!r}")
        transformed.append((category, transformed_bbox(node)))
        raw.append((category, raw_bbox(node)))

    changed_total = 0
    omission_total = 0
    offscene_omission_total = 0
    raw_membership_counts = Counter()
    nearest_class_counts = Counter()
    distance_band_counts = Counter()
    offscene_rows = set()
    offscene_cols = set()
    min_row = GRID_H
    max_row = -1
    min_col = GRID_W
    max_col = -1

    for cell in range(GRID_W * GRID_H):
        candidate_rgb = rgb(candidate, cell)
        reference_rgb = rgb(reference, cell)
        deltas = [
            abs(candidate_rgb[channel] - reference_rgb[channel])
            for channel in range(3)
        ]
        if max(deltas) < CELL_DELTA:
            continue
        changed_total += 1
        if not (near_white(candidate_rgb) and not near_white(reference_rgb)):
            continue
        omission_total += 1

        col = cell % GRID_W
        row = cell // GRID_W
        x = (col + 0.5) * cell_width
        y = (row + 0.5) * cell_height
        transformed_membership = tuple(
            category
            for category in CLASSES
            if any(
                item_category == category and contains(box, x, y)
                for item_category, box in transformed
            )
        )
        if transformed_membership:
            continue

        offscene_omission_total += 1
        offscene_rows.add(row)
        offscene_cols.add(col)
        min_row = min(min_row, row)
        max_row = max(max_row, row)
        min_col = min(min_col, col)
        max_col = max(max_col, col)

        raw_membership = tuple(
            category
            for category in CLASSES
            if any(
                item_category == category and contains(box, x, y)
                for item_category, box in raw
            )
        )
        raw_key = "+".join(raw_membership) if raw_membership else "none"
        raw_membership_counts[raw_key] += 1

        nearest_distance = math.inf
        nearest_class = "none"
        for category, box in raw:
            distance = distance_cells(box, x, y, cell_width, cell_height)
            if distance < nearest_distance:
                nearest_distance = distance
                nearest_class = category
        if math.isfinite(nearest_distance):
            distance_band_counts[distance_band(nearest_distance)] += 1
            nearest_class_counts[nearest_class] += 1
        else:
            distance_band_counts["no_geometry"] += 1
            nearest_class_counts["none"] += 1

    span = None
    if offscene_omission_total:
        span = {
            "min_row": min_row,
            "max_row": max_row,
            "min_col": min_col,
            "max_col": max_col,
            "unique_row_count": len(offscene_rows),
            "unique_col_count": len(offscene_cols),
            "row_span_fraction": (max_row - min_row + 1) / GRID_H,
            "col_span_fraction": (max_col - min_col + 1) / GRID_W,
        }

    receipt = {
        "schema": "chaptera.offscene-spatial-distance.v1",
        "fixture": args.fixture,
        "page": args.page,
        "grid": [GRID_W, GRID_H],
        "changed_cell_count": changed_total,
        "omission_cell_count": omission_total,
        "offscene_omission_cell_count": offscene_omission_total,
        "offscene_raw_membership": dict(
            sorted(raw_membership_counts.items(), key=lambda item: (-item[1], item[0]))
        ),
        "offscene_nearest_raw_class": dict(
            sorted(nearest_class_counts.items(), key=lambda item: (-item[1], item[0]))
        ),
        "offscene_nearest_distance_cells": {
            key: distance_band_counts.get(key, 0)
            for key in ("le_0.5", "le_1", "le_2", "le_4", "le_8", "gt_8", "no_geometry")
        },
        "offscene_grid_span": span,
        "claims": {
            "aggregate_only": True,
            "raw_candidate_pixels_emitted": False,
            "raw_reference_pixels_emitted": False,
            "raw_story_text_emitted": False,
            "distance_is_diagnostic_not_source_semantics": True,
        },
    }
    print("OFFSCENE_SPATIAL_DISTANCE " + json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
