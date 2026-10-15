"""Source-private composite-pixel alignment probe for exact SBDC034 R1.

This is not a glyph baseline oracle and never changes the PDF or the browser
renderer. All page pixels, source positions, texts and node identities stay in
the isolated runner. Only aggregate counts for predetermined vertical shifts
are eligible for the source-free acceptance receipt.
"""
from __future__ import annotations

from PIL import Image, ImageChops, ImageDraw, ImageFilter

from sbdc_r1_raster_disagreement_census import (
    DIFF_THRESHOLD,
    _finite_number,
    _paint_rect,
    _resolved_line_rects,
)

CANDIDATE_DY_PX = tuple(range(-24, 25, 4))
MAX_SHIFT_PX = 24


def _difference_mask(left: Image.Image, right: Image.Image) -> Image.Image:
    channels = ImageChops.difference(left, right).split()
    maximum = ImageChops.lighter(
        channels[0], ImageChops.lighter(channels[1], channels[2])
    )
    return maximum.point(lambda value: 255 if value >= DIFF_THRESHOLD else 0)


def _pixel_neighborhood_mismatch(
    source: Image.Image, target: Image.Image, mask: Image.Image,
) -> dict:
    """One-way nearest RGB-pixel agreement within 1px and 2px.

    This is an exploratory upper bound on edge/rasterization sensitivity,
    NOT a glyph coverage metric and NEVER replaces the exact page comparison.
    We retain both directions separately, because tolerance is not symmetric.
    """
    baseline = _difference_mask(source, target)
    unmatched = baseline
    counts = {0: ImageChops.multiply(mask, baseline).histogram()[255]}
    for radius in (1, 2):
        for dy in range(-radius, radius + 1):
            for dx in range(-radius, radius + 1):
                if max(abs(dx), abs(dy)) != radius:
                    continue
                translated = Image.new(
                    "RGB", target.size, target.getpixel((0, 0))
                )
                translated.paste(target, (dx, dy))
                mismatch = _difference_mask(source, translated)
                unmatched = ImageChops.darker(unmatched, mismatch)
        counts[radius] = ImageChops.multiply(mask, unmatched).histogram()[255]
    if not (counts[2] <= counts[1] <= counts[0]):
        raise AssertionError("neighborhood agreement must be monotonic")
    return {
        "radius1_unmatched_pixels": counts[1],
        "radius2_unmatched_pixels": counts[2],
    }


def _source_private_shared_mask(
    scene: dict, image_size: tuple[int, int]
) -> tuple[Image.Image, int]:
    page, = scene["pages"]
    width, height = image_size
    page_width = _finite_number(page["width_emu"], "page_width_emu")
    page_height = _finite_number(page["height_emu"], "page_height_emu")
    if page_width <= 0 or page_height <= 0:
        raise AssertionError("invalid physical source page dimensions")
    sx, sy = width / page_width, height / page_height
    shared = Image.new("L", image_size)
    excluded = Image.new("L", image_size)
    draw_shared = ImageDraw.Draw(shared)
    draw_excluded = ImageDraw.Draw(excluded)
    accepted_lines = 0
    for node in scene["nodes"]:
        if node.get("page_id") != page["page_id"]:
            continue
        if node.get("kind") == "picture_frame" and node.get("resource_id"):
            _paint_rect(draw_excluded, node.get("bounds"), node, sx, sy)
        if node.get("text"):
            lines = _resolved_line_rects(node)
            if lines is None:
                _paint_rect(
                    draw_excluded, node.get("text_bounds") or node.get("bounds"),
                    node, sx, sy,
                )
            else:
                for line in lines:
                    if _paint_rect(draw_shared, line, node, sx, sy):
                        accepted_lines += 1
        for cell in (node.get("table") or {}).get("cells", []):
            if cell.get("text"):
                _paint_rect(draw_excluded, cell.get("bounds"), node, sx, sy)

    if accepted_lines == 0:
        raise AssertionError("no source-resolved line areas for pixel alignment")
    # Keep the exact text regions but exclude picture and fallback interference
    # by more than the full shift sweep. The retained mask never leaves CI.
    shared = shared.filter(ImageFilter.MaxFilter(5))
    excluded = excluded.filter(ImageFilter.MaxFilter(2 * MAX_SHIFT_PX + 5))
    shared = ImageChops.subtract(shared, excluded)
    draw = ImageDraw.Draw(shared)
    draw.rectangle((0, 0, width - 1, MAX_SHIFT_PX), fill=0)
    draw.rectangle(
        (0, height - MAX_SHIFT_PX - 1, width - 1, height - 1), fill=0
    )
    count = shared.histogram()[255]
    if count < 100:
        raise AssertionError("no enough uncontaminated shared text pixels")
    return shared, count


def probe_shared_text_composite_shift(
    browser: Image.Image, own_pdf: Image.Image, scene: dict,
    page_significant_pixel_count: int,
) -> dict:
    """Compare identical text areas after *virtual* whole-PDF translations.

    A minimum away from zero is a triage signal only: translating background,
    clipping, and extracted font boxes do not prove an actual glyph offset.
    Never treat this probe as permission to change baseline or acceptance.
    """
    if browser.mode != "RGB" or own_pdf.mode != "RGB":
        raise AssertionError("exact R1 alignment requires RGB rasters")
    if browser.size != own_pdf.size:
        raise AssertionError("exact R1 alignment requires equal raster dimensions")
    baseline_mask = _difference_mask(browser, own_pdf)
    if baseline_mask.histogram()[255] != page_significant_pixel_count:
        raise AssertionError("shift probe disagrees with independent R1 pixel proof")
    mask, eligible = _source_private_shared_mask(scene, browser.size)
    baseline = None
    scores = []
    for dy in CANDIDATE_DY_PX:
        translated = Image.new("RGB", own_pdf.size, own_pdf.getpixel((0, 0)))
        translated.paste(own_pdf, (0, dy))
        diff_mask = _difference_mask(browser, translated)
        count = ImageChops.multiply(mask, diff_mask).histogram()[255]
        scores.append({"pdf_translation_dy_px": dy, "different_pixels": count})
        if dy == 0:
            baseline = count
    if baseline is None:
        raise AssertionError("zero-shift control is mandatory")
    best = min(
        scores,
        key=lambda score: (
            score["different_pixels"], abs(score["pdf_translation_dy_px"]),
            score["pdf_translation_dy_px"]
        ),
    )
    if best["different_pixels"] > baseline:
        raise AssertionError("shift scan failed to include baseline")
    # Ask whether differences concentrate near the same-colored neighbor,
    # independently in each direction. The renderer is not changed.
    browser_to_pdf = _pixel_neighborhood_mismatch(browser, own_pdf, mask)
    pdf_to_browser = _pixel_neighborhood_mismatch(own_pdf, browser, mask)
    if any(
        value > baseline
        for value in (*browser_to_pdf.values(), *pdf_to_browser.values())
    ):
        raise AssertionError("local pixel-neighbor recovery exceeded baseline")
    return {
        "schema": "chaptera.sbdc034-r1-private-shared-text-composite-shift.v1",
        "sample_kind": "source_resolved_text_minus_nearby_pictures_and_fallback",
        "render_dpi": 144,
        "threshold_max_rgb_channel": DIFF_THRESHOLD,
        "eligible_source_mask_pixels": eligible,
        "baseline_source_mask_different_pixels": baseline,
        "best_virtual_pdf_dy_px": best["pdf_translation_dy_px"],
        "best_virtual_different_pixels": best["different_pixels"],
        "potential_reduction_pixels": baseline - best["different_pixels"],
        "shift_scores": scores,
        "pixel_neighborhood_browser_to_pdf": browser_to_pdf,
        "pixel_neighborhood_pdf_to_browser": pdf_to_browser,
        "interpretation": "composite_probe_not_glyph_baseline_or_acceptance_budget",
        "source_text_ids_coordinates_fonts_pixels_exported": False,
    }
