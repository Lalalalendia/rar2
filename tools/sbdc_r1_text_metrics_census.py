"""Source-neutral text metrics: exact R1 ReaderScene vs its own physical PDF.

No Story text, node ID, source coordinate, text fragment, glyph or font binary
appears in the returned receipt. PDF extraction is private and ephemeral.
The PDF glyph box top is NOT an SVG text baseline: differences here are
diagnostics, not a pixel-perfect typography oracle.
"""
from __future__ import annotations

import math
import statistics
from collections import defaultdict
from io import BytesIO

import fitz
from PIL import Image, ImageChops, ImageDraw, ImageFilter

from sbdc_r1_raster_disagreement_census import (
    DIFF_THRESHOLD,
    MASK_MARGIN_PX,
    _mask_stats,
    _paint_rect,
    _resolved_line_rects,
    _source_rect,
)

EMU_PER_POINT = 12700
ASSIGNMENT_MARGIN_PT = 2.0


def _aggregate(values):
    values = sorted(values)
    if not values:
        return {"count": 0, "min": None, "median": None, "p95": None, "max": None}
    index = min(len(values) - 1, max(0, math.ceil(0.95 * len(values)) - 1))
    return {
        "count": len(values),
        "min": round(values[0], 3),
        "median": round(float(statistics.median(values)), 3),
        "p95": round(values[index], 3),
        "max": round(values[-1], 3),
    }


def _line_own_size_pt(node, source_line):
    layout = node.get("text_layout") or {}
    sizes = [layout.get("font_size_emu")]
    for span in source_line.get("spans") or ():
        if span.get("font_size_emu", 0) > 0:
            sizes.append(span["font_size_emu"])
    return [float(size) / EMU_PER_POINT for size in sizes if size and size > 0]


def _pdf_spans(page):
    """Private PDF text iterator: emits only geometry and font size, never text."""
    spans = []
    blocks = page.get_text("dict", sort=False).get("blocks", [])
    for block_index, block in enumerate(blocks):
        if block.get("type") != 0:
            continue
        for line_index, line in enumerate(block.get("lines") or []):
            for span in line.get("spans") or []:
                bounds = span.get("bbox") or ()
                if len(bounds) != 4 or any(not math.isfinite(v) for v in bounds):
                    continue
                size = span.get("size")
                if not isinstance(size, (int, float)) or not math.isfinite(size) or size <= 0:
                    continue
                if bounds[2] <= bounds[0] or bounds[3] <= bounds[1]:
                    continue
                spans.append({
                    "bbox": tuple(float(v) for v in bounds),
                    "size_pt": float(size),
                    "pdf_line_id": (block_index, line_index),
                    # No text or font name is retained in the output/report.
                })
    return spans


def _pdf_frame_candidate(node):
    rect = _source_rect(node.get("text_bounds") or node.get("bounds"))
    if rect is None or not node.get("text"):
        return None
    x, y, width, height = rect
    return tuple(v / EMU_PER_POINT for v in (x, y, width, height))


def _closest_frame_index(span, candidates):
    left, top, right, bottom = span["bbox"]
    center_x = (left + right) / 2
    center_y = (top + bottom) / 2
    matches = []
    for index, bounds in enumerate(candidates):
        if bounds is None:
            continue
        x, y, width, height = bounds
        if (x - ASSIGNMENT_MARGIN_PT <= center_x <= x + width + ASSIGNMENT_MARGIN_PT
            and y - ASSIGNMENT_MARGIN_PT <= center_y <= y + height + ASSIGNMENT_MARGIN_PT):
            # Only resolves overlapping text frames, not font identity.
            matches.append((width * height, index))
    if not matches:
        return None
    return min(matches)[1]


def census_r1_text_metrics(scene: dict, own_pdf_bytes: bytes,
                           browser: Image.Image, own_pdf: Image.Image,
                           significant_expected: int) -> dict:
    """Measure per-source-text-frame glyph-region disagreements + PDF sizes."""
    if browser.mode != "RGB" or own_pdf.mode != "RGB" or browser.size != own_pdf.size:
        raise AssertionError("text font census requires aligned RGB screen and PDF")
    page, = scene["pages"]
    width, height = browser.size
    page_width, page_height = page["width_emu"], page["height_emu"]
    if not isinstance(page_width, int) or not isinstance(page_height, int) or min(page_width, page_height) <= 0:
        raise AssertionError("source page dimensions missing")
    sx, sy = width / page_width, height / page_height
    source_nodes = [
        node for node in scene["nodes"]
        if node.get("page_id") == page["page_id"]
        and node.get("text") and _pdf_frame_candidate(node) is not None
    ]
    if not source_nodes:
        raise AssertionError("physical source has no source-backed text frames")

    # PDF program metrics from actual re-downloaded edited-R1 PDF.
    with fitz.open(stream=own_pdf_bytes, filetype="pdf") as document:
        if document.page_count != 1:
            raise AssertionError("exact R1 own-PDF text census needs one page")
        pdf_spans = _pdf_spans(document[0])
    if not pdf_spans:
        raise AssertionError("physical PDF has no extractable text spans")
    candidates = [_pdf_frame_candidate(node) for node in source_nodes]
    span_by_frame = defaultdict(list)
    unassigned_spans = 0
    for span in pdf_spans:
        index = _closest_frame_index(span, candidates)
        if index is None:
            unassigned_spans += 1
        else:
            span_by_frame[index].append(span)

    # Same threshold and 144-DPI rasters as the proven global census.
    channels = ImageChops.difference(browser, own_pdf).split()
    largest = ImageChops.lighter(channels[0], ImageChops.lighter(channels[1], channels[2]))
    changed = largest.point(lambda pixel: 255 if pixel >= DIFF_THRESHOLD else 0)
    if changed.histogram()[255] != significant_expected:
        raise AssertionError("text font census disagrees with independent R1 page pixel proof")
    source_total_lines = 0
    pdf_assigned_spans = 0
    pdf_matched_frames = 0
    source_shared_frames = 0
    fallback_frames = 0
    frame_stats = []
    size_diffs = []
    matched_nearest_top = []
    possible_size_differences = 0
    source_font_size_pt = []

    for index, node in enumerate(source_nodes):
        layout = node.get("text_layout")
        resolved = isinstance(layout, dict) and layout.get("disposition") == "shared_resolved"
        lines = sorted(layout.get("lines") or [], key=lambda line: line.get("line_index", 0)) if resolved else []
        if resolved:
            source_shared_frames += 1
            source_total_lines += len(lines)
            source_font_size_pt.append(float(layout["font_size_emu"]) / EMU_PER_POINT)
        else:
            fallback_frames += 1
        assigned = span_by_frame.get(index, [])
        if assigned:
            pdf_matched_frames += 1
        pdf_assigned_spans += len(assigned)
        distinct_pdf_lines = len({span["pdf_line_id"] for span in assigned})
        local_size_diffs = []

        if resolved:
            all_sizes = [size for line in lines for size in _line_own_size_pt(node, line)]
            if not all_sizes:
                all_sizes = [float(layout["font_size_emu"]) / EMU_PER_POINT]
            for span in assigned:
                # Conservative: compare with the closest valid source style,
                # not necessarily the uniform top-level size.
                difference = min(abs(span["size_pt"] - size) for size in all_sizes)
                size_diffs.append(difference)
                local_size_diffs.append(difference)
                if difference > 1.5:
                    possible_size_differences += 1

            # This compares box tops (not baselines) and is thus a diagnostic
            # only. It must never be used to change layout without font proof.
            source_rects = _resolved_line_rects(node) or []
            frame_y_pt = [(rect["y"] / EMU_PER_POINT) for rect in source_rects]
            pdf_y_pt = [span["bbox"][1] for span in assigned]
            for top in frame_y_pt:
                if pdf_y_pt:
                    matched_nearest_top.append(min(abs(y - top) for y in pdf_y_pt))

        mask = Image.new("L", browser.size)
        draw = ImageDraw.Draw(mask)
        if resolved:
            for rect in _resolved_line_rects(node) or []:
                _paint_rect(draw, rect, node, sx, sy)
        else:
            _paint_rect(draw, node.get("text_bounds") or node.get("bounds"), node, sx, sy)
        mask = mask.filter(ImageFilter.MaxFilter(2 * MASK_MARGIN_PX + 1))
        changed_in_box = _mask_stats(mask, changed, significant_expected)
        frame_stats.append({
            "region_category": "shared_resolved" if resolved else "browser_fallback",
            "source_plan_line_count": len(lines) if resolved else 0,
            "pdf_extracted_line_count": distinct_pdf_lines,
            "pdf_assigned_span_count": len(assigned),
            "significant_pixels_in_region_nonexclusive": changed_in_box["significant_pixels"],
            "area_pixels_in_region": changed_in_box["area_pixels"],
            "font_size_different_by_gt_1_5pt_span_count": sum(delta > 1.5 for delta in local_size_diffs),
        })

    if pdf_assigned_spans == 0:
        raise AssertionError("source frame ↔ own-PDF extracted text matching failed")
    # Sort by magnitude; avoid exposing source order and node identity.
    frame_stats.sort(key=lambda record: (
        -record["significant_pixels_in_region_nonexclusive"],
        record["region_category"],
    ))
    return {
        "schema": "chaptera.sbdc034-r1-text-metrics-census.v1",
        "origin": "exact_r1_authorized_reader_scene_and_own_pdf_text_extract",
        "source_text_frame_count": len(source_nodes),
        "source_shared_resolved_frame_count": source_shared_frames,
        "source_browser_fallback_frame_count": fallback_frames,
        "source_shared_layout_line_count": source_total_lines,
        "pdf_extracted_text_span_count": len(pdf_spans),
        "pdf_extracted_spans_unassigned_to_text_frame": unassigned_spans,
        "pdf_assigned_spans_to_source_frames": pdf_assigned_spans,
        "pdf_source_frames_with_assigned_spans": pdf_matched_frames,
        "pdf_spans_with_nearest_source_size_gt_1_5pt": possible_size_differences,
        "nearest_source_vs_pdf_font_size_abs_delta_pt": _aggregate(size_diffs),
        "nearest_source_line_top_vs_pdf_glyph_box_top_delta_pt": _aggregate(matched_nearest_top),
        "source_shared_font_size_pt": _aggregate(source_font_size_pt),
        "frames_anonymous_sorted_by_pixel_mismatch": frame_stats,
        "max_category_claim": "geometry_and_size_diagnostic_not_shared_font_causality",
        "private_story_text_glyph_bytes_font_names_node_ids_positions_exported": False,
    }
