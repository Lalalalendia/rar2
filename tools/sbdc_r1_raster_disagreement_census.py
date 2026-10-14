"""Source-neutral, ROI-based image/text pixel mismatch census for real SBDC R1.

This is *geometric attribution*, not font/semantic causal proof. It never
serializes source text, shape IDs, pixel coordinates, image bytes or screenshots.
Pixel buckets are exclusive and must sum to the independently counted page diff.
"""
from __future__ import annotations

import math

from PIL import Image, ImageChops, ImageDraw, ImageFilter

DIFF_THRESHOLD = 24
MASK_MARGIN_PX = 2


def _finite_number(value, name: str) -> float:
    if isinstance(value, bool):
        raise ValueError(f"invalid {name}")
    number = float(value)
    if not math.isfinite(number):
        raise ValueError(f"non-finite {name}")
    return number


def _source_rect(rect):
    if not isinstance(rect, dict):
        return None
    try:
        x, y, w, h = (
            _finite_number(rect[k], k) for k in ("x", "y", "width", "height")
        )
    except (ValueError, KeyError, TypeError):
        return None
    if not (w > 0 and h > 0):
        return None
    return x, y, w, h


def _transform_points(corners, node):
    transform = node.get("transform") or {}
    a = _finite_number(transform.get("a", 1), "transform.a")
    b = _finite_number(transform.get("b", 0), "transform.b")
    c = _finite_number(transform.get("c", 0), "transform.c")
    d = _finite_number(transform.get("d", 1), "transform.d")
    tx = _finite_number(transform.get("tx", 0), "transform.tx")
    ty = _finite_number(transform.get("ty", 0), "transform.ty")
    return [(a * x + c * y + tx, b * x + d * y + ty)
            for x, y in corners]


def _paint_rect(draw, source_rect, node, sx, sy):
    rect = _source_rect(source_rect)
    if rect is None:
        return False
    x, y, w, h = rect
    corners = ((x, y), (x + w, y), (x + w, y + h), (x, y + h))
    points = _transform_points(corners, node)
    draw.polygon([(round(px * sx), round(py * sy)) for px, py in points], fill=255)
    return True


def _resolved_line_rects(node):
    layout = node.get("text_layout")
    if not isinstance(layout, dict) or layout.get("disposition") != "shared_resolved":
        return None
    bounds = _source_rect(node.get("text_bounds") or node.get("bounds"))
    if bounds is None:
        return None
    x, y, width, height = bounds
    top = y + _finite_number(layout.get("vertical_offset_emu", 0), "vertical_offset")
    rects = []
    for line in sorted(layout.get("lines") or [], key=lambda ln: ln.get("line_index", 0)):
        line_h = _finite_number(line["line_height_emu"], "line_height_emu")
        measured = _finite_number(line["measured_width_emu"], "measured_width_emu")
        offset = _finite_number(line.get("x_offset_emu", 0), "x_offset_emu")
        if line_h <= 0 or measured < 0:
            raise ValueError("invalid shared text line layout")
        # Source's line box clipped to its own fixed-frame text viewport.
        left = max(x, x + offset)
        right = min(x + width, x + offset + measured)
        line_top = max(y, top)
        line_bottom = min(y + height, top + line_h)
        if line.get("text") and right > left and line_bottom > line_top:
            rects.append({"x": left, "y": line_top,
                          "width": right - left, "height": line_bottom - line_top})
        top += line_h
    return rects


def _mask_stats(mask, significant, significant_total):
    covered = mask.histogram()[255]
    counted = ImageChops.multiply(mask, significant).histogram()[255]
    return {
        "area_pixels": covered,
        "significant_pixels": counted,
        "fraction_of_significant": round(
            counted / significant_total, 6) if significant_total else 0.0,
        "significant_density_in_area": round(
            counted / covered, 6) if covered else 0.0,
    }


def census_r1_disagreement(browser: Image.Image, own_pdf: Image.Image,
                           scene: dict, significant_expected: int,
                           browser_preview_census: dict | None = None) -> dict:
    """Count real 144-DPI RGB pixel changes by source-backed geometric regions.

    The four mutually exclusive page zones are text-only, picture-only, both,
    and outside. Text masks use resolved line boxes when available, otherwise
    text-frame viewports. All masks are dilated two pixels for antialias edges.
    """
    if browser.size != own_pdf.size or browser.mode != "RGB" or own_pdf.mode != "RGB":
        raise AssertionError("pixel attribution needs exact same-sized RGB rasters")
    page, = scene["pages"]
    width, height = browser.size
    page_width = _finite_number(page["width_emu"], "page.width_emu")
    page_height = _finite_number(page["height_emu"], "page.height_emu")
    if page_width <= 0 or page_height <= 0:
        raise AssertionError("invalid source page geometry")
    sx, sy = width / page_width, height / page_height
    full_text = Image.new("L", (width, height))
    shared = Image.new("L", (width, height))
    fallback = Image.new("L", (width, height))
    table = Image.new("L", (width, height))
    images = Image.new("L", (width, height))
    draws = {name: ImageDraw.Draw(mask) for name, mask in [
        ("shared", shared), ("fallback", fallback),
        ("table", table), ("image", images),
    ]}
    resolved_text_nodes = 0
    fallback_text_nodes = 0
    table_text_cells = 0
    positive_picture_nodes = 0
    transformed_nodes = 0
    for node in scene["nodes"]:
        if node.get("page_id") != page["page_id"]:
            continue
        transform = node.get("transform") or {}
        if any(abs(_finite_number(transform.get(key, default), key) - default) > 1e-8
               for key, default in (("a", 1), ("b", 0), ("c", 0), ("d", 1),
                                    ("tx", 0), ("ty", 0))):
            transformed_nodes += 1
        if node.get("kind") == "picture_frame" and node.get("resource_id"):
            if _paint_rect(draws["image"], node.get("bounds"), node, sx, sy):
                positive_picture_nodes += 1
        if node.get("text"):
            rects = _resolved_line_rects(node)
            if rects is not None:
                resolved_text_nodes += 1
                for rect in rects:
                    _paint_rect(draws["shared"], rect, node, sx, sy)
            else:
                if _paint_rect(
                    draws["fallback"], node.get("text_bounds") or node.get("bounds"),
                    node, sx, sy
                ):
                    fallback_text_nodes += 1
        for cell in (node.get("table") or {}).get("cells", []):
            if cell.get("text") and _paint_rect(
                draws["table"], cell.get("bounds"), node, sx, sy
            ):
                table_text_cells += 1

    def padded(mask):
        return mask.filter(ImageFilter.MaxFilter(2 * MASK_MARGIN_PX + 1))

    shared, fallback, table, images = map(
        padded, (shared, fallback, table, images)
    )
    full_text = ImageChops.lighter(shared, ImageChops.lighter(fallback, table))

    channels = ImageChops.difference(browser, own_pdf).split()
    largest_delta = ImageChops.lighter(
        channels[0], ImageChops.lighter(channels[1], channels[2])
    )
    significant = largest_delta.point(
        lambda channel: 255 if channel >= DIFF_THRESHOLD else 0
    )
    significant_total = significant.histogram()[255]
    if significant_total != significant_expected:
        raise AssertionError(
            "ROI census and existing independent full-page difference disagree"
        )
    overlap = ImageChops.multiply(full_text, images)
    text_only = ImageChops.subtract(full_text, overlap)
    image_only = ImageChops.subtract(images, overlap)
    outside = ImageChops.invert(ImageChops.lighter(full_text, images))
    zones = {
        "text_only": _mask_stats(text_only, significant, significant_total),
        "picture_only": _mask_stats(image_only, significant, significant_total),
        "text_picture_overlap": _mask_stats(overlap, significant, significant_total),
        "outside_text_and_pictures": _mask_stats(outside, significant, significant_total),
    }
    if sum(x["area_pixels"] for x in zones.values()) != width * height:
        raise AssertionError("geometric zones do not partition page pixels")
    if sum(x["significant_pixels"] for x in zones.values()) != significant_total:
        raise AssertionError("geometric zones do not partition changed pixels")
    preview = browser_preview_census if isinstance(browser_preview_census, dict) else {}
    return {
        "schema": "chaptera.sbdc034-r1-text-image-pixel-census.v1",
        "source": "exact_current_r1_chromium_and_own_pdf_144dpi",
        "significant_channel_delta_threshold": DIFF_THRESHOLD,
        "roi_dilation_px": MASK_MARGIN_PX,
        "page_size_px": [width, height],
        "significant_pixels_total": significant_total,
        "zones_exclusive": zones,
        "text_source_node_counts": {
            "shared_resolved_plan_nodes": resolved_text_nodes,
            "fallback_frame_nodes": fallback_text_nodes,
            "table_cells_with_text": table_text_cells,
            "transformed_any_nodes": transformed_nodes,
        },
        "source_picture_frame_nodes": positive_picture_nodes,
        "nonexclusive_detail": {
            "shared_resolved_line_regions": _mask_stats(
                shared, significant, significant_total),
            "fallback_text_frame_regions": _mask_stats(
                fallback, significant, significant_total),
            "table_cell_text_regions": _mask_stats(
                table, significant, significant_total),
        },
        "actual_chromium_text_preview": {
            "total": preview.get("total", 0),
            "by_reason": preview.get("by_reason", {}),
            "by_size_source": preview.get("by_size_source", {}),
        },
        "interpretation": "source_scene_ROI_attribution_not_proof_of_cause",
        "source_text_pixels_pdf_and_coordinates_exported": False,
    }
