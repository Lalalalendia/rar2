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

CLASSES = ("picture", "text", "ellipse", "other_painted", "empty_emf")
WHITE_FLOOR = 245


def transformed_bbox(node: dict) -> tuple[float, float, float, float]:
    bounds = node["bounds"]
    x0 = float(bounds["x"])
    y0 = float(bounds["y"])
    x1 = x0 + float(bounds["width"])
    y1 = y0 + float(bounds["height"])
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


def rgb(grid: bytes, cell: int) -> tuple[int, int, int]:
    base = cell * 3
    return (grid[base], grid[base + 1], grid[base + 2])


def near_white(value: tuple[int, int, int]) -> bool:
    return min(value) >= WHITE_FLOOR


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

    boxes = {name: [] for name in CLASSES}
    for node in geometry["nodes"]:
        category = node["class"]
        if category not in boxes:
            raise ValueError(f"unsupported node class: {category!r}")
        boxes[category].append(transformed_bbox(node))

    changed_total = 0
    error_types = Counter()
    combination_counts = Counter()
    combination_error_types = {}
    covered_counts = Counter()
    changed_by_class = Counter()
    error_by_class = {name: Counter() for name in CLASSES}

    for cell in range(GRID_W * GRID_H):
        col = cell % GRID_W
        row = cell // GRID_W
        x = (col + 0.5) * width_emu / GRID_W
        y = (row + 0.5) * height_emu / GRID_H
        membership = tuple(
            name for name in CLASSES if any(contains(box, x, y) for box in boxes[name])
        )
        for name in membership:
            covered_counts[name] += 1

        candidate_rgb = rgb(candidate, cell)
        reference_rgb = rgb(reference, cell)
        deltas = [
            abs(candidate_rgb[channel] - reference_rgb[channel])
            for channel in range(3)
        ]
        if max(deltas) < CELL_DELTA:
            continue

        changed_total += 1
        if near_white(candidate_rgb) and not near_white(reference_rgb):
            error_type = "omission"
        elif not near_white(candidate_rgb) and near_white(reference_rgb):
            error_type = "excess"
        else:
            error_type = "both_nonwhite_or_tone"
        error_types[error_type] += 1

        key = "+".join(membership) if membership else "none"
        combination_counts[key] += 1
        combination_error_types.setdefault(key, Counter())[error_type] += 1
        for name in membership:
            changed_by_class[name] += 1
            error_by_class[name][error_type] += 1

    class_rows = {}
    for name in CLASSES:
        covered = covered_counts[name]
        changed = changed_by_class[name]
        class_rows[name] = {
            "node_count": len(boxes[name]),
            "covered_cell_count": covered,
            "changed_cell_count": changed,
            "changed_over_covered": changed / covered if covered else None,
            "error_types": dict(sorted(error_by_class[name].items())),
        }

    receipt = {
        "schema": "chaptera.visual-spatial-attribution.v1",
        "fixture": args.fixture,
        "page": args.page,
        "grid": [GRID_W, GRID_H],
        "cell_delta": CELL_DELTA,
        "white_floor": WHITE_FLOOR,
        "changed_cell_count": changed_total,
        "changed_cell_fraction": changed_total / (GRID_W * GRID_H),
        "error_types": dict(sorted(error_types.items())),
        "classes": class_rows,
        "changed_membership_combinations": dict(
            sorted(combination_counts.items(), key=lambda item: (-item[1], item[0]))
        ),
        "changed_membership_combination_error_types": {
            key: dict(sorted(combination_error_types[key].items()))
            for key, _ in sorted(
                combination_counts.items(), key=lambda item: (-item[1], item[0])
            )
        },
        "claims": {
            "aggregate_only": True,
            "raw_candidate_pixels_emitted": False,
            "raw_reference_pixels_emitted": False,
            "raw_story_text_emitted": False,
            "overlap_counts_are_not_causal_attribution": True,
        },
    }
    print("SPATIAL_RESIDUAL_ATTRIBUTION " + json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
