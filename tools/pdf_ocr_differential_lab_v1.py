#!/usr/bin/env python3
"""Synthetic-only PDF OCR differential laboratory (not a PDF reader/product API).

Source-neutral black-box fixture suite for native-PDF extraction versus raster
OCR. No private documents, ABBYY binaries, fonts, or real user text are used.
PyMuPDF is AGPL/commercial dual-licensed: OPTIONAL research use only.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import re
import shutil
import subprocess
import sys
from typing import Callable

import fitz

SAMPLE = "HELLO 2026"
MISDECODED = "WELLO 2026"
PDF_MAX_BYTES = 4 * 1024 * 1024
RASTER_MAX_PIXELS = 5_000_000
OCR_DPI = 240
SCHEMA = "chaptera.pdf-ocr-differential-lab.v1"


def _cmap(*, poisoned: bool) -> bytes:
    first = "0057" if poisoned else "0048"
    lines = [
        "/CIDInit /ProcSet findresource begin", "12 dict begin", "begincmap",
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def",
        "/CMapName /Synthetic-Text-Layer def", "/CMapType 2 def",
        "1 begincodespacerange", "<00> <FF>", "endcodespacerange",
        "8 beginbfchar", f"<48> <{first}>", "<45> <0045>",
        "<4C> <004C>", "<4F> <004F>", "<20> <0020>",
        "<32> <0032>", "<30> <0030>", "<36> <0036>",
        "endbfchar", "endcmap", "CMapName currentdict /CMap defineresource pop",
        "end", "end", "",
    ]
    return "\n".join(lines).encode("ascii")


def _stream(payload: bytes) -> bytes:
    return f"<< /Length {len(payload)} >>\nstream\n".encode("ascii") + payload + b"endstream"


def make_vector_pdf(*, poisoned: bool) -> bytes:
    """Same painted glyphs, but optional false ToUnicode for ASCII H."""
    paint = b"BT /F1 36 Tf 60 135 Td (HELLO 2026) Tj ET\n"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 420 200] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        _stream(paint),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding /ToUnicode 6 0 R >>",
        _stream(_cmap(poisoned=poisoned)),
    ]
    result = bytearray(b"%PDF-1.7\n%\x80\x81\x82\x83\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(result))
        result.extend(f"{index} 0 obj\n".encode("ascii"))
        result.extend(obj)
        result.extend(b"\nendobj\n")
    startxref = len(result)
    result.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode("ascii"))
    for offset in offsets[1:]:
        result.extend(f"{offset:010d} 00000 n \n".encode("ascii"))
    result.extend(
        f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\n"
        f"startxref\n{startxref}\n%%EOF\n".encode("ascii")
    )
    return bytes(result)


def make_raster_pdf(*, hidden_wrong_layer: bool) -> bytes:
    """Raster-only page, optionally carrying invisible wrong search text."""
    source = fitz.open(stream=make_vector_pdf(poisoned=False), filetype="pdf")
    pix = source[0].get_pixmap(dpi=OCR_DPI, colorspace=fitz.csRGB, alpha=False)
    image = pix.tobytes("png")
    source.close()
    result = fitz.open()
    page = result.new_page(width=420, height=200)
    page.insert_image(page.rect, stream=image)
    if hidden_wrong_layer:
        page.insert_text((60, 65), MISDECODED, fontsize=36, render_mode=3)
    data = result.tobytes(deflate=True, garbage=3, no_new_id=True)
    result.close()
    return data


def fixtures() -> dict[str, bytes]:
    return {
        "native_vector": make_vector_pdf(poisoned=False),
        "wrong_tounicode": make_vector_pdf(poisoned=True),
        "raster_only": make_raster_pdf(hidden_wrong_layer=False),
        "invisible_wrong_layer": make_raster_pdf(hidden_wrong_layer=True),
    }


def _normalized(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip().upper()


def render_png(page: fitz.Page) -> bytes:
    pix = page.get_pixmap(dpi=OCR_DPI, colorspace=fitz.csRGB, alpha=False)
    if pix.width * pix.height > RASTER_MAX_PIXELS:
        raise ValueError("raster_limit_exceeded")
    return pix.tobytes("png")


def raster_digest(source: bytes) -> str:
    with fitz.open(stream=source, filetype="pdf") as document:
        page = document[0]
        pix = page.get_pixmap(dpi=OCR_DPI, colorspace=fitz.csRGB, alpha=False)
        return hashlib.sha256(bytes(pix.samples)).hexdigest()


def tesseract_ocr(png: bytes) -> str:
    if not shutil.which("tesseract"):
        raise RuntimeError("tesseract_unavailable")
    proc = subprocess.run(
        ["tesseract", "stdin", "stdout", "--psm", "6", "-l", "eng"],
        input=png,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
        check=False,
    )
    if proc.returncode:
        raise RuntimeError("tesseract_failed")
    return proc.stdout.decode("utf-8", errors="replace")


@dataclass(frozen=True)
class Probe:
    pdf_sha256: str
    native_text_sha256: str
    ocr_text_sha256: str
    native_chars: int
    ocr_chars: int
    image_count: int
    hidden_glyphs: int
    paint_glyphs: int
    disagree: bool
    route: str

    def as_dict(self) -> dict:
        return vars(self).copy()


def probe(source: bytes, ocr: Callable[[bytes], str] = tesseract_ocr) -> Probe:
    if len(source) > PDF_MAX_BYTES:
        raise ValueError("pdf_size_limit_exceeded")
    try:
        doc = fitz.open(stream=source, filetype="pdf")
    except Exception as exc:
        raise ValueError("pdf_open_failed") from exc
    with doc:
        if doc.needs_pass or len(doc) != 1:
            raise ValueError("encrypted_or_not_one_page")
        page = doc[0]
        native = _normalized(page.get_text(sort=False))
        traces = page.get_texttrace()
        hidden = sum(len(t["chars"]) for t in traces if t["type"] == 3)
        visible = sum(len(t["chars"]) for t in traces if t["type"] != 3)
        images = len(page.get_image_info(xrefs=False))
        ocr_text = _normalized(ocr(render_png(page)))
    if not ocr_text:
        route = "OCR_EMPTY_REVIEW"
    elif not native:
        route = "OCR_REQUIRED"
    elif hidden:
        route = "HIDDEN_TEXT_LAYER_REVIEW"
    elif native != ocr_text:
        route = "UNICODE_CONFLICT_REVIEW"
    else:
        route = "NATIVE_TEXT_CORROBORATED"
    return Probe(
        pdf_sha256=hashlib.sha256(source).hexdigest(),
        native_text_sha256=hashlib.sha256(native.encode()).hexdigest(),
        ocr_text_sha256=hashlib.sha256(ocr_text.encode()).hexdigest(),
        native_chars=len(native), ocr_chars=len(ocr_text), image_count=images,
        hidden_glyphs=hidden, paint_glyphs=visible,
        disagree=native != ocr_text, route=route,
    )


def run_synthetic() -> dict:
    cases = fixtures()
    results = {name: probe(data).as_dict() for name, data in cases.items()}
    return {
        "schema": SCHEMA,
        "synthetic_only": True,
        "engines": {"parser": f"PyMuPDF {fitz.VersionBind}", "ocr": "local Tesseract"},
        "identical_pixels_vector_pair": raster_digest(cases["native_vector"]) == raster_digest(cases["wrong_tounicode"]),
        "cases": results,
        "no_original_pdf_modified": True,
        "native_text_is_not_assumed_correct": True,
    }


def main(argv: list[str] | None = None) -> int:
    argp = argparse.ArgumentParser(description=__doc__)
    argp.add_argument("--synthetic", action="store_true", required=True,
                      help="run four bundled generated PDF cases only")
    args = argp.parse_args(argv)
    assert args.synthetic
    data = run_synthetic()
    # All output is synthetic and text-free; never output customer documents.
    print(json.dumps(data, sort_keys=True, indent=2))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, ValueError, subprocess.TimeoutExpired) as exc:
        print(f"probe_failed:{type(exc).__name__}", file=sys.stderr)
        sys.exit(2)
