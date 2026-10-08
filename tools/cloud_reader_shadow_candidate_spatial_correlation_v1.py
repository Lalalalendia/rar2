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


def distance_cells(
    box: tuple[float, float, float, float],
    x: float,
    y: float,
    cell_width: float,
    cell_height: float,
) -> float:
    dx = max(box[0] - x, 0.0, x - box[2]) / cell_width
    dy = max(box[1] - y, 0.0, y - box[3]) / cell_height
    return math.hypot(dx, dy)


def distance_band(distance: float) -> str:
    if distance <= 0.5:
        return "le_0.5"
    if distance <= 1.0:
        return "le_1"
    if distance <= 2.0:
        return "le_2"
    if distance <= 4.0:
        return "le_4"
    if distance <= 8.0:
        return "le_8"
    return "gt_8"


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
    parser.add_argument("candidate_map_json", type=Path)
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

    candidate_map = json.loads(args.candidate_map_json.read_text(encoding="utf-8"))
    if candidate_map["candidate"] != "primary_0x02BF_scalar_0x00080000":
        raise ValueError("unexpected candidate-map class")
    candidate_ids = set(candidate_map["candidate_node_ids"])

    width_emu = float(geometry["width_emu"])
    height_emu = float(geometry["height_emu"])
    cell_width = width_emu / GRID_W
    cell_height = height_emu / GRID_H

    all_boxes = []
    candidate_boxes = []
    noncandidate_boxes = []
    candidate_class_counts = Counter()
    candidate_scene_ids = set()

    for node in geometry["nodes"]:
        box = transformed_bbox(node)
        node_id = node["node_id"]
        row = (node["class"], box)
        all_boxes.append(row)
        if node_id in candidate_ids:
            candidate_boxes.append(row)
            candidate_scene_ids.add(node_id)
            candidate_class_counts[node["class"]] += 1
        else:
            noncandidate_boxes.append(row)

    if candidate_scene_ids != candidate_ids:
        missing = sorted(candidate_ids - candidate_scene_ids)
        extra = sorted(candidate_scene_ids - candidate_ids)
        raise ValueError(
            f"candidate node join mismatch missing={len(missing)} extra={len(extra)}"
        )
    if not candidate_boxes or not noncandidate_boxes:
        raise ValueError("candidate/noncandidate comparison requires both cohorts")

    offscene_omission_count = 0
    nearest_cohort = Counter()
    candidate_distance_bands = Counter()
    noncandidate_distance_bands = Counter()
    nearest_candidate_class = Counter()
    nearest_noncandidate_class = Counter()
    candidate_within_1 = 0
    candidate_within_2 = 0
    noncandidate_within_1 = 0
    noncandidate_within_2 = 0

    for cell in range(GRID_W * GRID_H):
        candidate_rgb = rgb(candidate, cell)
        reference_rgb = rgb(reference, cell)
        if max(
            abs(candidate_rgb[channel] - reference_rgb[channel])
            for channel in range(3)
        ) < CELL_DELTA:
            continue
        if not (near_white(candidate_rgb) and not near_white(reference_rgb)):
            continue

        col = cell % GRID_W
        row = cell // GRID_W
        x = (col + 0.5) * cell_width
        y = (row + 0.5) * cell_height
        if any(contains(box, x, y) for _, box in all_boxes):
            continue

        offscene_omission_count += 1
        cand_distance, cand_class = min(
            (distance_cells(box, x, y, cell_width, cell_height), category)
            for category, box in candidate_boxes
        )
        non_distance, non_class = min(
            (distance_cells(box, x, y, cell_width, cell_height), category)
            for category, box in noncandidate_boxes
        )

        candidate_distance_bands[distance_band(cand_distance)] += 1
        noncandidate_distance_bands[distance_band(non_distance)] += 1
        nearest_candidate_class[cand_class] += 1
        nearest_noncandidate_class[non_class] += 1

        if cand_distance <= 1:
            candidate_within_1 += 1
        if cand_distance <= 2:
            candidate_within_2 += 1
        if non_distance <= 1:
            noncandidate_within_1 += 1
        if non_distance <= 2:
            noncandidate_within_2 += 1

        if abs(cand_distance - non_distance) <= 1e-9:
            nearest_cohort["tie"] += 1
        elif cand_distance < non_distance:
            nearest_cohort["candidate"] += 1
        else:
            nearest_cohort["noncandidate"] += 1

    receipt = {
        "schema": "chaptera.shadow-candidate-spatial-correlation.v1",
        "fixture": args.fixture,
        "page": args.page,
        "candidate_class": "primary_0x02BF_scalar_0x00080000",
        "scene_node_count": len(geometry["nodes"]),
        "candidate_scene_node_count": len(candidate_boxes),
        "noncandidate_scene_node_count": len(noncandidate_boxes),
        "candidate_scene_class_counts": dict(sorted(candidate_class_counts.items())),
        "offscene_omission_cell_count": offscene_omission_count,
        "nearest_cohort": dict(sorted(nearest_cohort.items())),
        "candidate_distance_bands": dict(sorted(candidate_distance_bands.items())),
        "noncandidate_distance_bands": dict(sorted(noncandidate_distance_bands.items())),
        "nearest_candidate_class": dict(sorted(nearest_candidate_class.items())),
        "nearest_noncandidate_class": dict(sorted(nearest_noncandidate_class.items())),
        "candidate_within_1_count": candidate_within_1,
        "candidate_within_2_count": candidate_within_2,
        "noncandidate_within_1_count": noncandidate_within_1,
        "noncandidate_within_2_count": noncandidate_within_2,
        "claims": {
            "aggregate_only": True,
            "candidate_node_ids_emitted": False,
            "raw_reference_pixels_emitted": False,
            "raw_story_text_emitted": False,
            "spatial_correlation_is_not_semantic_authority": True,
            "product_semantics_changed": False,
        },
    }
    print("SHADOW_CANDIDATE_SPATIAL_CORRELATION " + json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
