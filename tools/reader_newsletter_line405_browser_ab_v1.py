#!/usr/bin/env python3
"""Exact-source Chromium A/B for Newsletter LINE405; NOT a Publisher golden.

Publisher PDFs are intentionally absent from hosted CI. The two exact Reader
revisions must produce SHA-pinned real browser screenshots at 144 DPI, linked
to the same physical PAGE381 identity by the separate source-ID contract.
"""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from cloud_reader_reference_pdf_raster_v1 import render_png
from pdf_reference_diff_v1 import SIGNIFICANT_CHANNEL_DELTA, compare_rasters
from reader_logo_0x31_visual_ab_v1 import (
    checked_capture, SOURCE_SHA256, PAGE_COUNT, DPI,
)
from reader_logo_publisher_native_verdict_v1 import verify_page381_customer_page4_join

SCHEMA = "chaptera.reader-source-line405-browser-physical-ab.v1"
BASE_SHA = "47eb72faac780692335eef7f00ac0dbd9daaf206"
TARGET_SHA = "846c735f2f7eb2ea92451236bdb146cb0eb481d8"
# Physical, source-verified LINE405 stroke envelope in PAGE381 EMU.
X0, X1, Y0, Y1 = 1_058_750, 7_788_599, 705_474, 807_074
# OfficeArt local lineColor=scheme2 is independently resolved to #FF00FF.
SOURCE_LINE_RGB = (255, 0, 255)
SOURCE_MAGENTA_MIN_FRACTION = 0.98
SOURCE_PAINT_COORDINATE_TOLERANCE_PX = 2
SOURCE_STROKE_MIN_COVERAGE = 0.85


def source_line_roi(page: dict, raster: dict, padding_px: int = 8) -> dict:
    """Predict the clipped source-stroke region in an actual browser PNG."""
    w, h = page.get("width_emu"), page.get("height_emu")
    rw, rh = raster["width"], raster["height"]
    if (not isinstance(w, int) or not isinstance(h, int)
            or min(w, h, rw, rh) <= 0 or padding_px < 0):
        raise ValueError("invalid browser PAGE geometry for physical LINE405")
    x0 = max(0, math.floor(X0 * rw / w) - padding_px)
    x1 = min(rw, math.ceil(X1 * rw / w) + padding_px)
    y0 = max(0, math.floor(Y0 * rh / h) - padding_px)
    y1 = min(rh, math.ceil(Y1 * rh / h) + padding_px)
    if x1 <= x0 or y1 <= y0:
        raise ValueError("LINE405 source envelope is fully outside the captured page")
    return {"x": x0, "y": y0, "width": x1 - x0, "height": y1 - y0}


def accept_page_delta(index: int, count: int, bbox: dict | None, roi: dict | None) -> None:
    if not isinstance(count, int) or count < 0:
        raise ValueError("missing or malformed significant pixel count")
    if index < 3:
        if count:
            raise ValueError(f"non-target customer page {index + 1} changed: {count} pixels")
        return
    if index != 3:
        raise ValueError("unexpected customer page index")
    if count <= 0 or not isinstance(bbox, dict) or roi is None:
        raise ValueError("source LINE405 did not produce a measurable page-4 raster delta")
    if not all(isinstance(bbox.get(key), int) for key in ("x", "y", "width", "height")):
        raise ValueError("malformed page-4 raster change bbox")
    if bbox["width"] <= 0 or bbox["height"] <= 0:
        raise ValueError("empty page-4 significant change bbox")
    inside = (
        bbox["x"] >= roi["x"] and bbox["y"] >= roi["y"]
        and bbox["x"] + bbox["width"] <= roi["x"] + roi["width"]
        and bbox["y"] + bbox["height"] <= roi["y"] + roi["height"]
    )
    if not inside:
        raise ValueError("page-4 raster change escapes the physically sourced LINE405 envelope")



def expected_source_line_bbox(page: dict, raster: dict) -> dict:
    """Exact visible geometry of the source EMU stroke, not a raster oracle."""
    w, h = page.get("width_emu"), page.get("height_emu")
    rw, rh = raster.get("width"), raster.get("height")
    if not all(isinstance(item, int) and item > 0 for item in (w, h, rw, rh)):
        raise ValueError("invalid source PAGE or browser raster dimensions")
    left = max(0, math.floor(X0 * rw / w))
    right = min(rw, math.ceil(X1 * rw / w))
    top = max(0, round(Y0 * rh / h))
    bottom = min(rh, round(Y1 * rh / h))
    if right <= left or bottom <= top:
        raise ValueError("source LINE405 stroke has no visible raster footprint")
    return {"x": left, "y": top, "width": right - left, "height": bottom - top}


def accept_source_line_footprint(bbox: dict, count: int, expected: dict) -> None:
    """Reject misplaced or nearly invisible pixels within an overly broad ROI."""
    if not isinstance(bbox, dict) or not isinstance(count, int) or count <= 0:
        raise ValueError("LINE405 has no positive source stroke pixel footprint")
    keys = ("x", "y", "width", "height")
    if any(not isinstance(bbox.get(k), int) for k in keys):
        raise ValueError("LINE405 has invalid significant-pixel geometry")
    if min(bbox["width"], bbox["height"]) <= 0:
        raise ValueError("LINE405 significant-pixel geometry is empty")
    t = SOURCE_PAINT_COORDINATE_TOLERANCE_PX
    if any(abs(bbox[key] - expected[key]) > t for key in ("x", "y")) or any(
        abs((bbox[axis] + bbox[size]) - (expected[axis] + expected[size])) > t
        for axis, size in (("x", "width"), ("y", "height"))
    ):
        raise ValueError("LINE405 stroke has incorrect source-backed start, end, or thickness")
    expected_area = expected["width"] * expected["height"]
    if count < math.ceil(SOURCE_STROKE_MIN_COVERAGE * expected_area):
        raise ValueError("LINE405 stroke has insufficient visible source-area coverage")


def verify_source_line_magenta(before: dict, after: dict, bbox: dict, count: int) -> dict:
    """Confirm browser paint actually matches the source RGB, not just its ROI.

    SourceLine RGB is independent OfficeArt authority; this does not compare
    against or claim visual parity with the separate Publisher PDF oracle.
    """
    w, h = before.get("width"), before.get("height")
    if (not isinstance(w, int) or not isinstance(h, int) or w <= 0 or h <= 0
            or (w, h) != (after.get("width"), after.get("height"))):
        raise ValueError("LINE405 browser rasters have inconsistent dimensions")
    stride_b, stride_a = before.get("stride"), after.get("stride")
    samples_b, samples_a = before.get("samples"), after.get("samples")
    if (not isinstance(stride_b, int) or not isinstance(stride_a, int)
            or min(stride_b, stride_a) < 3 * w
            or not isinstance(samples_b, bytes) or not isinstance(samples_a, bytes)
            or len(samples_b) != stride_b * h
            or len(samples_a) != stride_a * h):
        raise ValueError("LINE405 malformed browser RGB raster bytes")
    x0, y0 = bbox["x"], bbox["y"]
    x1, y1 = x0 + bbox["width"], y0 + bbox["height"]
    if x0 < 0 or y0 < 0 or x1 > w or y1 > h:
        raise ValueError("LINE405 significant bbox exceeds source browser page")
    changed = 0
    source_magenta = 0
    for y in range(y0, y1):
        row_b, row_a = y * stride_b, y * stride_a
        for x in range(x0, x1):
            pb, pa = row_b + x * 3, row_a + x * 3
            br, bg, bb = samples_b[pb:pb + 3]
            ar, ag, ab = samples_a[pa:pa + 3]
            if max(abs(ar - br), abs(ag - bg), abs(ab - bb)) < SIGNIFICANT_CHANNEL_DELTA:
                continue
            changed += 1
            # The 1px antialias fringe is permitted outside this strong-color
            # class, but the visible stroke must overwhelmingly be magenta.
            if ar >= 239 and ag <= 20 and ab >= 239:
                source_magenta += 1
    if changed != count:
        raise ValueError("LINE405 significant pixel tally differs from raster comparator")
    fraction = source_magenta / count
    if fraction < SOURCE_MAGENTA_MIN_FRACTION:
        raise ValueError("LINE405 browser raster did not paint source-backed magenta")
    return {
        "source_rgb": list(SOURCE_LINE_RGB),
        "strong_source_magenta_pixels": source_magenta,
        "fraction_of_changed_significant_pixels": round(fraction, 9),
        "min_source_magenta_fraction": SOURCE_MAGENTA_MIN_FRACTION,
    }


def measure(before_path: Path, after_path: Path) -> dict:
    if BASE_SHA == TARGET_SHA:
        raise ValueError("baseline and target must be distinct exact revisions")
    before = checked_capture(before_path, BASE_SHA)
    after = checked_capture(after_path, TARGET_SHA)
    if before["page_ids"] != after["page_ids"]:
        raise ValueError("customer PAGE identity/order changed across exact Reader revisions")
    page381_id = verify_page381_customer_page4_join(before, after)
    old_nodes = before["fixture"]["nodes"]
    new_nodes = after["fixture"]["nodes"]
    if not isinstance(old_nodes, int) or new_nodes != old_nodes + 1:
        raise ValueError("expected exactly one additional source LINE405 Scene node")

    pages = []
    for index in range(PAGE_COUNT):
        shot_b = before["fixture"]["screenshots"][index]
        shot_a = after["fixture"]["screenshots"][index]
        raster_b = render_png(before_path.parent / shot_b["filename"])
        raster_a = render_png(after_path.parent / shot_a["filename"])
        geometry_b = before["fixture"]["page_geometry"][index]
        geometry_a = after["fixture"]["page_geometry"][index]
        if geometry_b != geometry_a:
            raise ValueError(f"physical customer PAGE geometry changed: p{index + 1}")
        delta = compare_rasters(raster_b, raster_a)
        if delta["raster_size_match"] is not True:
            raise ValueError(f"browser PNG dimensions changed: p{index + 1}")
        roi = source_line_roi(geometry_b, raster_b) if index == 3 else None
        count = delta["significant_pixel_count"]
        bbox = delta["significant_bbox"]
        accept_page_delta(index, count, bbox, roi)
        source_bbox = expected_source_line_bbox(geometry_b, raster_b) if index == 3 else None
        source_paint = None
        if index == 3:
            accept_source_line_footprint(bbox, count, source_bbox)
            source_paint = verify_source_line_magenta(raster_b, raster_a, bbox, count)
        pages.append({
            "customer_page": index + 1,
            "source_page_identity_sha256": before["page_ids"][index],
            "baseline_png_sha256": shot_b["sha256"],
            "candidate_png_sha256": shot_a["sha256"],
            "pixels_changed_significantly": count,
            "fraction_changed_significantly": delta["significant_fraction"],
            "significant_bbox": bbox,
            "physical_source_line_roi_pixels": roi,
            "expected_source_line_bbox_pixels": source_bbox,
            "verified_source_rgb_paint": source_paint,
        })
    return {
        "schema": SCHEMA,
        "source_pub_sha256": SOURCE_SHA256,
        "baseline_commit": BASE_SHA,
        "candidate_commit": TARGET_SHA,
        "capture_dpi": DPI,
        "source_line": "PAGE381/raw0x20/seq405",
        "physical_page381_id": page381_id,
        "page381_to_customer_page4_independently_proven": True,
        "source_page_order_unchanged": True,
        "candidate_added_scene_nodes": new_nodes - old_nodes,
        "pages": pages,
        "claims": {
            "real_144dpi_chromium_browser_raster_measured": True,
            "non_target_customer_pages_no_significant_changes": True,
            "target_changes_confined_to_native_sourced_line_envelope": True,
            "source_line_length_thickness_and_magenta_rgb_verified": True,
            "publisher_visual_parity_proven": False,
            "publisher_pdf_present_in_hosted_ci": False,
            "libreoffice_is_reference": False,
            "original_pub_or_publisher_pdf_bytes_emitted": False,
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline-receipt", type=Path, required=True)
    parser.add_argument("--candidate-receipt", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = measure(args.baseline_receipt, args.candidate_receipt)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    for page in report["pages"]:
        print(
            f"page{page['customer_page']} direct_significant_pixels="
            f"{page['pixels_changed_significantly']}"
        )
    print("LINE405_BROWSER_AB=source_confined_visible_delta_only_NOT_PUBLISHER_VERDICT")


if __name__ == "__main__":
    main()
