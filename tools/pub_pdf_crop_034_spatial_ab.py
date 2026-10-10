#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import zlib

import fitz
from PIL import Image

GRID_W = 64
GRID_H = 64
RASTER_DPI = 144
CELL_DELTA = 12
NUM = r"[-+]?(?:[0-9]+(?:[.][0-9]*)?|[.][0-9]+)"
CLIPPED_IMAGE = re.compile(
    rf"({NUM})\s+({NUM})\s+({NUM})\s+({NUM})\s+re\s+W\s+n\s+"
    rf"{NUM}\s+0\s+0\s+-?{NUM}\s+{NUM}\s+{NUM}\s+cm\s+/\S+\s+Do",
    re.MULTILINE,
)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as src:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def page_grid(page: fitz.Page) -> bytes:
    pix = page.get_pixmap(dpi=RASTER_DPI, colorspace=fitz.csRGB, alpha=False)
    image = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
    return image.resize((GRID_W, GRID_H), Image.Resampling.BOX).tobytes()


def reference_page(path: Path, pub_sha256: str, page_number: int) -> tuple[bytes, str]:
    data = json.loads(path.read_text(encoding="utf-8"))
    pairs = [pair for pair in data.get("pairs", []) if pair.get("pub_sha256") == pub_sha256]
    if len(pairs) != 1:
        raise ValueError("reference PUB identity must resolve exactly once")
    pages = pairs[0].get("pages", [])
    if not 1 <= page_number <= len(pages):
        raise ValueError("reference page outside registered range")
    page = pages[page_number - 1]
    raw = zlib.decompress(base64.b64decode(page["rgb_zlib_base64"]))
    if len(raw) != GRID_W * GRID_H * 3:
        raise ValueError("reference grid byte length mismatch")
    digest = hashlib.sha256(raw).hexdigest()
    if digest != page["grid_sha256"]:
        raise ValueError("reference grid SHA mismatch")
    return raw, digest


def changed_mask(candidate: bytes, reference: bytes) -> list[bool]:
    result = []
    for cell in range(GRID_W * GRID_H):
        base = cell * 3
        local = max(
            abs(candidate[base + channel] - reference[base + channel])
            for channel in range(3)
        )
        result.append(local >= CELL_DELTA)
    return result


def clipped_image_rect(pdf: fitz.Document, page_index: int) -> tuple[float, float, float, float]:
    page = pdf[page_index]
    streams = []
    for xref in page.get_contents():
        streams.append(pdf.xref_stream(xref).decode("latin1", errors="ignore"))
    content = "\n".join(streams)
    matches = list(CLIPPED_IMAGE.finditer(content))
    if len(matches) != 1:
        raise ValueError(f"expected exactly one clipped image use, observed {len(matches)}")
    x, y, w, h = (float(value) for value in matches[0].groups())
    if w <= 0 or h <= 0:
        raise ValueError("clipped image frame has non-positive extent")
    return x, y, w, h


def region_for_cell(
    row: int,
    col: int,
    page_width: float,
    page_height: float,
    rect: tuple[float, float, float, float],
) -> str:
    x, y, w, h = rect
    x2, y2 = x + w, y + h
    cell_w = page_width / GRID_W
    cell_h = page_height / GRID_H
    center_x = (col + 0.5) * cell_w
    center_y_top = (row + 0.5) * cell_h
    center_y_pdf = page_height - center_y_top

    inside = x <= center_x <= x2 and y <= center_y_pdf <= y2
    band_x = cell_w
    band_y = cell_h
    near_vertical = (
        y - band_y <= center_y_pdf <= y2 + band_y
        and min(abs(center_x - x), abs(center_x - x2)) <= band_x
    )
    near_horizontal = (
        x - band_x <= center_x <= x2 + band_x
        and min(abs(center_y_pdf - y), abs(center_y_pdf - y2)) <= band_y
    )
    if near_vertical or near_horizontal:
        return "frame_edge_band"
    if inside:
        return "frame_interior"
    return "outside_frame"


def classify_delta(
    baseline_mask: list[bool],
    candidate_mask: list[bool],
    page_width: float,
    page_height: float,
    rect: tuple[float, float, float, float],
) -> dict:
    regression = {"frame_edge_band": 0, "frame_interior": 0, "outside_frame": 0}
    improvement = {"frame_edge_band": 0, "frame_interior": 0, "outside_frame": 0}
    for cell, (before, after) in enumerate(zip(baseline_mask, candidate_mask)):
        if before == after:
            continue
        row, col = divmod(cell, GRID_W)
        region = region_for_cell(row, col, page_width, page_height, rect)
        if not before and after:
            regression[region] += 1
        elif before and not after:
            improvement[region] += 1
    return {
        "regression_only_cells": regression,
        "improvement_only_cells": improvement,
        "regression_only_total": sum(regression.values()),
        "improvement_only_total": sum(improvement.values()),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline-pdf", required=True, type=Path)
    parser.add_argument("--candidate-pdf", required=True, type=Path)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--pub-sha256", required=True)
    parser.add_argument("--page", required=True, type=int)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    reference, reference_sha = reference_page(args.reference, args.pub_sha256, args.page)
    with fitz.open(args.baseline_pdf) as baseline_pdf, fitz.open(args.candidate_pdf) as candidate_pdf:
        page_index = args.page - 1
        if baseline_pdf.page_count <= page_index or candidate_pdf.page_count <= page_index:
            raise ValueError("requested page missing from candidate or baseline")
        baseline_page = baseline_pdf[page_index]
        candidate_page = candidate_pdf[page_index]
        if (
            abs(float(baseline_page.rect.width) - float(candidate_page.rect.width)) > 0.001
            or abs(float(baseline_page.rect.height) - float(candidate_page.rect.height)) > 0.001
        ):
            raise ValueError("baseline/candidate media extent mismatch")

        baseline_grid = page_grid(baseline_page)
        candidate_grid = page_grid(candidate_page)
        baseline_mask = changed_mask(baseline_grid, reference)
        candidate_mask = changed_mask(candidate_grid, reference)
        rect = clipped_image_rect(candidate_pdf, page_index)
        delta = classify_delta(
            baseline_mask,
            candidate_mask,
            float(candidate_page.rect.width),
            float(candidate_page.rect.height),
            rect,
        )

    receipt = {
        "schema": "chaptera.pub-pdf-crop-034-spatial-ab.v1",
        "pub_sha256": args.pub_sha256,
        "page": args.page,
        "baseline_pdf_sha256": sha256(args.baseline_pdf),
        "candidate_pdf_sha256": sha256(args.candidate_pdf),
        "reference_grid_sha256": reference_sha,
        "baseline_changed_cell_count": sum(baseline_mask),
        "candidate_changed_cell_count": sum(candidate_mask),
        "net_changed_cell_delta": sum(candidate_mask) - sum(baseline_mask),
        **delta,
        "claims": {
            "reference_is_existing_publisher_fingerprint": True,
            "candidate_clip_is_product_output_from_source_backed_frame": True,
            "clip_coordinates_emitted": False,
            "cell_mask_emitted": False,
            "document_content_emitted": False,
            "pdf_bytes_emitted": False,
            "raster_is_screening_not_semantic_authority": True,
        },
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "baseline_changed_cell_count": receipt["baseline_changed_cell_count"],
        "candidate_changed_cell_count": receipt["candidate_changed_cell_count"],
        "net_changed_cell_delta": receipt["net_changed_cell_delta"],
        "regression_only_cells": receipt["regression_only_cells"],
        "improvement_only_cells": receipt["improvement_only_cells"],
    }, sort_keys=True))


if __name__ == "__main__":
    main()
