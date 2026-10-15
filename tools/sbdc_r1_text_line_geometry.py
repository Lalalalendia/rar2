"""Source-private R1 text-line geometry comparison with public aggregate metrics.

PyMuPDF extracts the actual lines painted in the editor's own PDF. Chromium
supplies SHA-256 hashes and physical bounding boxes of actual SVG <text>.
All text, hashes and absolute locations remain temporary inside the runner.
Only counts and aggregate pixel-error statistics may leave this process.
"""
from __future__ import annotations

from collections import defaultdict
import hashlib
import math
import re
from statistics import median
import unicodedata


def normalized_text_hash(text: str) -> str | None:
    value = re.sub(r"\s+", " ", unicodedata.normalize("NFC", text.replace("\u200b", ""))).strip()
    return hashlib.sha256(value.encode("utf-8")).hexdigest() if value else None


def own_pdf_text_rects(page, dpi: int) -> list[dict]:
    """Extract only PDF text blocks, never embed images or serialize private text."""
    if dpi != 144:
        raise AssertionError("physical browser/PDF comparison must have fixed 144dpi")
    import fitz

    text_page = page.get_text("dict", flags=fitz.TEXTFLAGS_TEXT, sort=False)
    scale = dpi / 72
    rows = []
    for block in text_page.get("blocks", []):
        if block.get("type") != 0:
            continue
        for line in block.get("lines", []):
            spans = line.get("spans", [])
            text = "".join(str(span.get("text", "")) for span in spans)
            digest = normalized_text_hash(text)
            if not digest:
                continue
            bounds = line.get("bbox")
            if not bounds or len(bounds) != 4:
                raise AssertionError("PDF text line lacks physical bbox")
            x0, y0, x1, y1 = (float(v) for v in bounds)
            if not all(map(math.isfinite, (x0, y0, x1, y1))):
                raise AssertionError("PDF text line includes non-finite position")
            if x1 <= x0 or y1 <= y0:
                raise AssertionError("PDF text line includes degenerate geometry")
            rows.append({
                "text_sha256": digest, "x": x0 * scale, "y": y0 * scale,
                "width": (x1 - x0) * scale, "height": (y1 - y0) * scale,
            })
    return rows


def _physical_box(row: dict) -> tuple:
    digest = row.get("text_sha256")
    if not isinstance(digest, str) or len(digest) != 64 or any(
        char not in "0123456789abcdef" for char in digest
    ):
        raise AssertionError("real source line must use a private SHA-256 digest")
    nums = tuple(float(row[key]) for key in ("x", "y", "width", "height"))
    if not all(map(math.isfinite, nums)) or nums[2] <= 0 or nums[3] <= 0:
        raise AssertionError("source text line has invalid physical glyph bounds")
    return nums


def _statistics(values: list[float]) -> dict:
    if not values:
        return {"median": None, "p90": None, "max": None}
    ordered = sorted(values)
    p90_index = max(0, math.ceil(len(ordered) * 0.9) - 1)
    return {"median": round(median(ordered), 3),
            "p90": round(ordered[p90_index], 3),
            "max": round(ordered[-1], 3)}


def compare_actual_text_line_geometry(browser: list[dict], pdf: list[dict]) -> dict:
    """Best-match equal-text lines by nearest physical center, no source text egress.

    This diagnoses whether the *same* Unicode line has different painted
    geometry. Differing wraps/text lines are counted as unmatched, not forced
    into false 1:1 correspondence. PDF bbox includes font ascender geometry,
    so its top differs from SVG text-before-edge and is NOT a baseline proof.
    """
    if len(browser) > 10000 or len(pdf) > 10000:
        raise AssertionError("unexpected unbounded text geometry sample")
    pdf_by_hash = defaultdict(list)
    for line in pdf:
        rect = _physical_box(line)
        pdf_by_hash[line["text_sha256"]].append(rect)

    dx_abs = []
    dy_abs = []
    dw_abs = []
    dh_abs = []
    size_ratios = []
    matched = 0
    nonempty = 0
    for line in browser:
        bx, by, bw, bh = _physical_box(line)
        nonempty += 1
        options = pdf_by_hash.get(line["text_sha256"])
        if not options:
            continue
        cx, cy = bx + bw / 2, by + bh / 2
        index = min(range(len(options)), key=lambda i: (
            (options[i][0] + options[i][2] / 2 - cx) ** 2
            + (options[i][1] + options[i][3] / 2 - cy) ** 2
        ))
        px, py, pw, ph = options.pop(index)
        matched += 1
        dx_abs.append(abs(px - bx))
        dy_abs.append(abs(py - by))
        dw_abs.append(abs(pw - bw))
        dh_abs.append(abs(ph - bh))
        size_ratios.append(pw / bw)
    pdf_unmatched = sum(map(len, pdf_by_hash.values()))
    if matched + (nonempty - matched) != nonempty or matched + pdf_unmatched != len(pdf):
        raise AssertionError("text line matching lost or duplicated source authority")
    return {
        "schema": "chaptera.sbdc034-r1-chromium-ownpdf-text-line-geometry.v1",
        "chromium_shared_painted_line_count": nonempty,
        "own_pdf_painted_text_line_count": len(pdf),
        "exact_unicode_line_matches": matched,
        "browser_lines_without_exact_pdf_text": nonempty - matched,
        "pdf_lines_without_exact_browser_text": pdf_unmatched,
        "matched_abs_left_offset_px": _statistics(dx_abs),
        "matched_abs_top_offset_px": _statistics(dy_abs),
        "matched_abs_line_width_delta_px": _statistics(dw_abs),
        "matched_abs_line_height_delta_px": _statistics(dh_abs),
        "matched_pdf_to_chromium_width_ratio": _statistics(size_ratios),
        "interpretation": "actual_glyph_rectangles_only_not_text_causal_proof",
        "source_text_hashes_and_coordinates_exported": False,
    }
