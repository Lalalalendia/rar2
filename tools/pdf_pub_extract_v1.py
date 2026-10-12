#!/usr/bin/env python3
"""Conservative PDF paint probe for Chaptera's future PDF -> native PUB path.

Research-only adapter. It emits neither PUB bytes nor a Chaptera document;
only exact-source, candidate-only geometry and explicit unsupported diagnostics.
The emitted JSON CONTAINS DOCUMENT TEXT and must remain private for real inputs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import sys
from typing import Any

try:
    import fitz
except ImportError as exc:
    raise SystemExit("missing_dependency:pymupdf_1_26_7") from exc

EMU_PER_POINT = 12700
SCHEMA = "chaptera.pdf-pub-extract.v1"
MAX_PDF_BYTES = 32 * 1024 * 1024
MAX_PAGES = 100
MAX_PAINT_OPS_PER_PAGE = 5000
MAX_TEXT_CHARS_PER_PAGE = 200000


class ProbeRejected(ValueError):
    """Typed failure without raw source text, file path, or PDF parser traceback."""


def _emu(number: float) -> int:
    value = float(number)
    if not math.isfinite(value) or abs(value) > 1_000_000:
        raise ProbeRejected("invalid_geometry")
    return round(value * EMU_PER_POINT)


def _rect_emu(value: Any) -> list[int]:
    rect = fitz.Rect(value)
    if rect.is_empty or rect.is_infinite or not all(math.isfinite(v) for v in rect):
        raise ProbeRejected("invalid_geometry")
    return [_emu(rect.x0), _emu(rect.y0), _emu(rect.x1), _emu(rect.y1)]


def _rgb(values: Any) -> list[int] | None:
    if not isinstance(values, (list, tuple)) or len(values) != 3:
        return None
    if not all(isinstance(v, (int, float)) and math.isfinite(v) and 0 <= v <= 1 for v in values):
        return None
    return [round(float(v) * 255) for v in values]


def _unit_vector(values: Any) -> bool:
    return (
        isinstance(values, (list, tuple))
        and len(values) == 2
        and abs(float(values[0]) - 1.0) < 1e-5
        and abs(float(values[1])) < 1e-5
    )


def _overlaps(left: list[int], right: list[int]) -> bool:
    return min(left[2], right[2]) > max(left[0], right[0]) and min(left[3], right[3]) > max(left[1], right[1])


def _page_user_unit(document: Any, page: Any) -> bool:
    key_type, raw = document.xref_get_key(page.xref, "UserUnit")
    if key_type == "null":
        return True
    if key_type not in ("int", "real"):
        return False
    try:
        return math.isclose(float(raw), 1.0, abs_tol=1e-9)
    except (TypeError, ValueError):
        return False


def _text_candidates(page: Any, paint_types: list[str], diagnostics: set[str]) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    extracted_text: list[str] = []
    text_dict = page.get_text("dict", sort=False)
    for block in text_dict.get("blocks", []):
        if block.get("type") == 1:
            diagnostics.add("image_requires_separate_import")
        elif block.get("type") != 0:
            diagnostics.add("unknown_text_block")
        else:
            for line in block.get("lines", []):
                spans = line.get("spans", [])
                if len(spans) != 1 or not _unit_vector(line.get("dir")) or line.get("wmode") != 0:
                    diagnostics.add("complex_text_layout")
                extracted_text.extend(span.get("text", "") for span in spans if span.get("text"))

    painted_text: list[str] = []
    for trace in page.get_texttrace():
        seqno = trace.get("seqno")
        if not isinstance(seqno, int) or seqno < 0 or seqno >= len(paint_types):
            diagnostics.add("unmatched_text_paint")
            continue
        if paint_types[seqno] != "fill-text":
            diagnostics.add("unsupported_text_paint")
            continue
        chars = trace.get("chars", ())
        if not chars:
            diagnostics.add("unsupported_text_paint")
            continue
        codepoints = [entry[0] for entry in chars]
        if any(not isinstance(c, int) or c < 32 or c > 0x10FFFF or 0xD800 <= c <= 0xDFFF or c == 0xFFFD for c in codepoints):
            diagnostics.add("unmapped_text_glyph")
            continue
        text = "".join(chr(codepoint) for codepoint in codepoints)
        painted_text.append(text)
        if (
            trace.get("type") != 0
            or trace.get("wmode") != 0
            or trace.get("bidi_lvl") != 0
            or not _unit_vector(trace.get("dir"))
            or not math.isclose(trace.get("opacity", -1), 1.0, abs_tol=1e-6)
            or trace.get("layer")
        ):
            diagnostics.add("complex_text_paint")
            continue
        color = _rgb(trace.get("color")) if trace.get("colorspace") == 3 else None
        if color is None:
            diagnostics.add("unsupported_text_color")
            continue
        size = trace.get("size")
        if not isinstance(size, (float, int)) or not math.isfinite(size) or size <= 0:
            diagnostics.add("unsupported_text_size")
            continue
        origins = [entry[2] for entry in chars]
        if any(abs(float(p[1]) - float(origins[0][1])) > 0.25 for p in origins):
            diagnostics.add("complex_text_layout")
            continue
        if any(origins[i + 1][0] < origins[i][0] for i in range(len(origins) - 1)):
            diagnostics.add("complex_text_layout")
            continue
        candidates.append({
            "kind": "text_line",
            "paint_order": seqno,
            "bbox_emu": _rect_emu(trace["bbox"]),
            "baseline_emu": [_emu(origins[0][0]), _emu(origins[0][1])],
            "text": text,
            "font_name": trace.get("font") or "",
            "font_size_pt": round(float(size), 4),
            "fill_rgb": color,
            "font_resource": "unresolved",
            "source": "pdf_texttrace",
            "editable": "candidate_only",
        })
    # Text extraction and glyph-paint extraction disagree for ligatures, invisible
    # OCR layers, multi-span lines, and other cases. No semantic authority then.
    if sorted(painted_text) != sorted(extracted_text):
        diagnostics.add("ambiguous_text_mapping")
    if any(kind in {"stroke-text", "ignore-text"} for kind in paint_types):
        diagnostics.add("unsupported_text_paint")
    if len(candidates) != sum(kind == "fill-text" for kind in paint_types):
        diagnostics.add("unmatched_text_paint")
    return candidates


def _rectangle_candidates(page: Any, paint_types: list[str], diagnostics: set[str]) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    for drawing in page.get_drawings(extended=True):
        typ = drawing.get("type")
        if typ in ("clip", "group") or drawing.get("level", 0) != 0:
            diagnostics.add("unsupported_clip_or_group")
            continue
        seqno = drawing.get("seqno")
        if not isinstance(seqno, int) or not (0 <= seqno < len(paint_types)):
            diagnostics.add("unmatched_vector_paint")
            continue
        if typ != "f" or paint_types[seqno] != "fill-path":
            diagnostics.add("unsupported_vector_paint")
            continue
        if not math.isclose(float(drawing.get("fill_opacity", 0)), 1.0, abs_tol=1e-6):
            diagnostics.add("unsupported_vector_opacity")
            continue
        items = drawing.get("items", ())
        if len(items) != 1 or items[0][0] != "re" or drawing.get("even_odd") or drawing.get("layer"):
            diagnostics.add("unsupported_vector_path")
            continue
        color = _rgb(drawing.get("fill"))
        if color is None:
            diagnostics.add("unsupported_vector_color")
            continue
        candidates.append({
            "kind": "filled_rectangle",
            "paint_order": seqno,
            "bbox_emu": _rect_emu(items[0][1]),
            "fill_rgb": color,
            "source": "pdf_drawings",
            "editable": "candidate_only",
        })
    if len(candidates) != sum(kind == "fill-path" for kind in paint_types):
        diagnostics.add("unmatched_vector_paint")
    return candidates


def probe(data: bytes) -> dict[str, Any]:
    if not isinstance(data, bytes) or not data.startswith(b"%PDF-"):
        raise ProbeRejected("not_pdf")
    if len(data) > MAX_PDF_BYTES:
        raise ProbeRejected("pdf_too_large")
    digest = hashlib.sha256(data).hexdigest()
    try:
        doc = fitz.open(stream=data, filetype="pdf")
    except (RuntimeError, ValueError) as exc:
        raise ProbeRejected("pdf_parse_failed") from exc
    try:
        if doc.needs_pass or doc.is_encrypted:
            raise ProbeRejected("encrypted_pdf")
        if doc.page_count > MAX_PAGES:
            raise ProbeRejected("too_many_pages")
        result: dict[str, Any] = {
            "schema": SCHEMA, "source_sha256": digest, "units": "EMU",
            "pages": [], "status": "candidate",
            "native_pub_written": False,
        }
        for page in doc:
            diagnostics: set[str] = set()
            if page.rotation != 0 or page.cropbox != page.mediabox or not _page_user_unit(doc, page):
                diagnostics.add("nontrivial_page_transform")
            width, height = _emu(page.rect.width), _emu(page.rect.height)
            if width <= 0 or height <= 0:
                raise ProbeRejected("invalid_page_dimensions")
            bboxes = page.get_bboxlog()
            if len(bboxes) > MAX_PAINT_OPS_PER_PAGE:
                raise ProbeRejected("too_many_paint_operations")
            types = [kind for kind, _ in bboxes]
            if any(kind == "fill-image" for kind in types):
                diagnostics.add("image_requires_separate_import")
            if any(kind not in {"fill-text", "fill-path"} for kind in types):
                diagnostics.add("unsupported_paint_operator")
            if any(kind not in {"fill-text", "stroke-text", "ignore-text", "fill-path", "stroke-path", "fill-image"} for kind in types):
                diagnostics.add("unknown_paint_operator")
            if page.first_annot is not None or page.get_links():
                diagnostics.add("annotations_or_links_not_imported")
            objects = _text_candidates(page, types, diagnostics) + _rectangle_candidates(page, types, diagnostics)
            if sum(len(c["text"]) for c in objects if c["kind"] == "text_line") > MAX_TEXT_CHARS_PER_PAGE:
                raise ProbeRejected("text_budget_exceeded")
            objects.sort(key=lambda item: item["paint_order"])
            if any(_overlaps(a["bbox_emu"], b["bbox_emu"]) for i, a in enumerate(objects) for b in objects[i + 1:]):
                diagnostics.add("overlapping_objects_require_review")
            page_result = {
                "index": page.number,
                "width_emu": width, "height_emu": height,
                "objects": objects,
                "diagnostics": sorted(diagnostics),
                "status": "partial" if diagnostics else "candidate",
            }
            result["pages"].append(page_result)
            if diagnostics:
                result["status"] = "partial"
        return result
    finally:
        doc.close()


def _write_private_json(path: Path, payload: dict[str, Any]) -> None:
    # Exclusive creation avoids accidental overwrite of source, symlinks and
    # prior results; no plaintext or customer paths go to stdout/stderr.
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    fd = os.open(path, flags, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as output:
        json.dump(payload, output, sort_keys=True, ensure_ascii=False, indent=2)
        output.write("\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", required=True, type=Path, help="Private, new JSON file")
    options = parser.parse_args(argv)
    try:
        payload = probe(options.source.read_bytes())
        _write_private_json(options.output, payload)
    except ProbeRejected as exc:
        print(f"rejected:{exc}", file=sys.stderr)
        return 2
    except OSError:
        print("rejected:io_failure", file=sys.stderr)
        return 2
    # Only counts and a safe status are printed. The JSON contains source text.
    summary = {
        "status": payload["status"],
        "page_count": len(payload["pages"]),
        "candidate_count": sum(len(page["objects"]) for page in payload["pages"]),
        "diagnostic_count": sum(len(page["diagnostics"]) for page in payload["pages"]),
        "native_pub_written": False,
    }
    print(json.dumps(summary, sort_keys=True))
    return 0 if payload["status"] == "candidate" else 3


if __name__ == "__main__":
    raise SystemExit(main())
