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


def intersects(
    box: tuple[float, float, float, float],
    cell: tuple[float, float, float, float],
) -> bool:
    return box[0] < cell[2] and box[2] > cell[0] and box[1] < cell[3] and box[3] > cell[1]


def rgb(grid: bytes, cell: int) -> tuple[int, int, int]:
    base = cell * 3
    return (grid[base], grid[base + 1], grid[base + 2])


def near_white(value: tuple[int, int, int]) -> bool:
    return min(value) >= WHITE_FLOOR


def key_for(names: tuple[str, ...]) -> str:
    return "+".join(names) if names else "none"


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

    boxes: dict[str, list[tuple[float, float, float, float]]] = {}
    for node in geometry["nodes"]:
        paint_bounds = node.get("paint_bounds")
        if paint_bounds is None:
            continue
        category = str(node["class"])
        boxes.setdefault(category, []).append(
            transformed_bbox(paint_bounds, node.get("transform"))
        )

    changed_total = 0
    omission_total = 0
    center_none_omission = 0
    area_none_omission = 0
    reclassified = Counter()
    center_membership = Counter()
    area_membership = Counter()
    remaining_gap_bands = Counter()
    remaining_nearest_class = Counter()
    remaining_cells = set()
    remaining_reference_rgb_sum = [0, 0, 0]
    remaining_reference_rgb_quantized = Counter()

    cell_width = width_emu / GRID_W
    cell_height = height_emu / GRID_H

    for cell_index in range(GRID_W * GRID_H):
        candidate_rgb = rgb(candidate, cell_index)
        reference_rgb = rgb(reference, cell_index)
        if max(abs(candidate_rgb[i] - reference_rgb[i]) for i in range(3)) < CELL_DELTA:
            continue
        changed_total += 1

        omission = near_white(candidate_rgb) and not near_white(reference_rgb)
        if omission:
            omission_total += 1

        col = cell_index % GRID_W
        row = cell_index // GRID_W
        x0 = col * cell_width
        y0 = row * cell_height
        x1 = (col + 1) * cell_width
        y1 = (row + 1) * cell_height
        center_x = (x0 + x1) / 2
        center_y = (y0 + y1) / 2
        cell_rect = (x0, y0, x1, y1)

        center_names = tuple(
            name for name in sorted(boxes)
            if any(contains(box, center_x, center_y) for box in boxes[name])
        )
        area_names = tuple(
            name for name in sorted(boxes)
            if any(intersects(box, cell_rect) for box in boxes[name])
        )
        center_key = key_for(center_names)
        area_key = key_for(area_names)
        center_membership[center_key] += 1
        area_membership[area_key] += 1

        if omission and not center_names:
            center_none_omission += 1
            if not area_names:
                area_none_omission += 1
                remaining_cells.add((col, row))
                remaining_reference_rgb_sum[0] += reference_rgb[0]
                remaining_reference_rgb_sum[1] += reference_rgb[1]
                remaining_reference_rgb_sum[2] += reference_rgb[2]
                remaining_reference_rgb_quantized[
                    tuple((channel // 32) * 32 for channel in reference_rgb)
                ] += 1
                nearest = None
                nearest_classes = []
                for name in sorted(boxes):
                    for box in boxes[name]:
                        gap_x = max(box[0] - x1, x0 - box[2], 0.0) / cell_width
                        gap_y = max(box[1] - y1, y0 - box[3], 0.0) / cell_height
                        distance = max(gap_x, gap_y)
                        if nearest is None or distance < nearest - 1e-12:
                            nearest = distance
                            nearest_classes = [name]
                        elif abs(distance - nearest) <= 1e-12 and name not in nearest_classes:
                            nearest_classes.append(name)
                if nearest is None:
                    remaining_gap_bands[">unbounded"] += 1
                    remaining_nearest_class["none"] += 1
                else:
                    if nearest <= 0.5:
                        band = "<=0.5"
                    elif nearest <= 1.0:
                        band = "(0.5,1]"
                    elif nearest <= 2.0:
                        band = "(1,2]"
                    elif nearest <= 4.0:
                        band = "(2,4]"
                    else:
                        band = ">4"
                    remaining_gap_bands[band] += 1
                    remaining_nearest_class["+".join(nearest_classes)] += 1
            else:
                reclassified[area_key] += 1

    occupied_columns = sorted({col for col, _ in remaining_cells})
    occupied_rows = sorted({row for _, row in remaining_cells})
    if remaining_cells:
        min_col = min(col for col, _ in remaining_cells)
        max_col = max(col for col, _ in remaining_cells)
        min_row = min(row for _, row in remaining_cells)
        max_row = max(row for _, row in remaining_cells)
        remaining_bbox = {
            "min_col": min_col,
            "max_col": max_col,
            "min_row": min_row,
            "max_row": max_row,
            "width_cells": max_col - min_col + 1,
            "height_cells": max_row - min_row + 1,
        }
    else:
        remaining_bbox = None

    edge_bands = Counter()
    quadrants = Counter()
    for col, row in remaining_cells:
        edge_distance = min(col, row, GRID_W - 1 - col, GRID_H - 1 - row)
        if edge_distance <= 1:
            edge_bands["<=1"] += 1
        elif edge_distance <= 2:
            edge_bands["(1,2]"] += 1
        elif edge_distance <= 4:
            edge_bands["(2,4]"] += 1
        elif edge_distance <= 8:
            edge_bands["(4,8]"] += 1
        else:
            edge_bands[">8"] += 1
        horizontal = "left" if col < GRID_W / 2 else "right"
        vertical = "top" if row < GRID_H / 2 else "bottom"
        quadrants[f"{vertical}_{horizontal}"] += 1

    unseen = set(remaining_cells)
    component_sizes = []
    component_bboxes = []
    while unseen:
        seed = unseen.pop()
        stack = [seed]
        component = [seed]
        while stack:
            col, row = stack.pop()
            for neighbor in ((col - 1, row), (col + 1, row), (col, row - 1), (col, row + 1)):
                if neighbor in unseen:
                    unseen.remove(neighbor)
                    stack.append(neighbor)
                    component.append(neighbor)
        component_sizes.append(len(component))
        component_bboxes.append({
            "size": len(component),
            "min_col": min(col for col, _ in component),
            "max_col": max(col for col, _ in component),
            "min_row": min(row for _, row in component),
            "max_row": max(row for _, row in component),
        })
    component_sizes.sort(reverse=True)
    component_bboxes.sort(key=lambda item: (-item["size"], item["min_row"], item["min_col"]))

    reference_rgb_mean = (
        [
            round(channel_sum / len(remaining_cells), 3)
            for channel_sum in remaining_reference_rgb_sum
        ]
        if remaining_cells
        else None
    )
    reference_rgb_quantized_top = [
        {"rgb": list(rgb_key), "count": count}
        for rgb_key, count in remaining_reference_rgb_quantized.most_common(12)
    ]

    payload = {
        "schema": "chaptera.visual-paint-cell-intersection.v1",
        "fixture": args.fixture,
        "page": args.page,
        "grid": [GRID_W, GRID_H],
        "changed_cell_count": changed_total,
        "omission_cell_count": omission_total,
        "center_none_omission_count": center_none_omission,
        "area_intersection_none_omission_count": area_none_omission,
        "center_none_omission_reclassified": center_none_omission - area_none_omission,
        "center_none_omission_reclassified_fraction": (
            (center_none_omission - area_none_omission) / center_none_omission
            if center_none_omission else 0.0
        ),
        "reclassified_by_area_membership": dict(
            sorted(reclassified.items(), key=lambda item: (-item[1], item[0]))
        ),
        "remaining_area_none_gap_bands_chebyshev_cells": dict(
            sorted(remaining_gap_bands.items())
        ),
        "remaining_area_none_nearest_paint_class": dict(
            sorted(remaining_nearest_class.items(), key=lambda item: (-item[1], item[0]))
        ),
        "remaining_area_none_topology": {
            "occupied_column_count": len(occupied_columns),
            "occupied_row_count": len(occupied_rows),
            "bbox": remaining_bbox,
            "page_edge_bands": dict(sorted(edge_bands.items())),
            "quadrants": dict(sorted(quadrants.items())),
            "component_count": len(component_sizes),
            "component_sizes_desc": component_sizes,
            "largest_components": component_bboxes[:12],
            "reference_rgb_mean": reference_rgb_mean,
            "reference_rgb_quantized_top": reference_rgb_quantized_top,
        },
        "center_membership_counts": dict(
            sorted(center_membership.items(), key=lambda item: (-item[1], item[0]))
        ),
        "area_membership_counts": dict(
            sorted(area_membership.items(), key=lambda item: (-item[1], item[0]))
        ),
        "claims": {
            "cell_area_matches_box_downsampling_support": True,
            "renderer_true_paint_bounds": True,
            "paint_geometry_approximated_by_transformed_aabb": True,
            "raw_candidate_pixels_emitted": False,
            "raw_reference_pixels_emitted": False,
        },
    }
    print("PAINT_CELL_INTERSECTION " + json.dumps(payload, sort_keys=True))


if __name__ == "__main__":
    main()
