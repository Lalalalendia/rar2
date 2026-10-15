#!/usr/bin/env python3
"""Independent, source-private PDF text proof for a newly authored Guest Story.

Consumes the actual PDF downloaded through Chromium. It must not trust Story
state, the export receipt, PDF metadata or invisible PDF text.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys
import unicodedata

MAX_PDF_BYTES = 16 * 1024 * 1024
MAX_MARKER_CHARS = 256
SCHEMA = "chaptera.tempedit-independent-authored-text-pdf-consumer.v1"


def canonical_text(text: str) -> str:
    return "".join(c for c in unicodedata.normalize("NFC", text) if not c.isspace())


def verify_pdf(path: Path, marker: str) -> dict:
    if not marker or len(marker) > MAX_MARKER_CHARS or not marker.isprintable():
        raise ValueError("invalid expected marker envelope")
    data = path.read_bytes()
    if not data.startswith(b"%PDF-") or not (1 <= len(data) <= MAX_PDF_BYTES):
        raise ValueError("download is not a bounded physical PDF")

    import fitz  # independent PyMuPDF PDF text consumer

    pdf = fitz.open(stream=data, filetype="pdf")
    try:
        if pdf.is_encrypted or pdf.page_count < 1:
            raise ValueError("PDF encrypted or contains no pages")
        expected = canonical_text(marker)
        found_pages = 0
        for page in pdf:
            raw = page.get_text("rawdict", flags=fitz.TEXTFLAGS_RAWDICT)
            chars = []
            for block in raw.get("blocks", []):
                if block.get("type") != 0:
                    continue
                for line in block.get("lines", []):
                    for span in line.get("spans", []):
                        chars.extend(char.get("c", "") for char in span.get("chars", []))
            if expected not in canonical_text("".join(chars)):
                continue

            # Require physical text-painting commands, not metadata or an
            # invisible text layer (PDF rendering mode 3).
            visible_chars = []
            for op in page.get_texttrace():
                if op.get("type") == 3 or op.get("opacity", 0) <= 0:
                    continue
                for char in op.get("chars", ()):
                    try:
                        visible_chars.append(chr(char[0]))
                    except (ValueError, TypeError, IndexError):
                        pass
            if expected in canonical_text("".join(visible_chars)):
                found_pages += 1
        if found_pages != 1:
            raise ValueError("authored Story missing, hidden or ambiguous in PDF")
        return {
            "schema": SCHEMA,
            "status": "PASS",
            "artifact_sha256": hashlib.sha256(data).hexdigest(),
            "pdf_page_count": pdf.page_count,
            "authored_text_recovered": True,
            "marker_page_count": found_pages,
            "physical_pdf_text_consumer": "PyMuPDF",
        }
    finally:
        pdf.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--expected-text", required=True)
    args = parser.parse_args()
    try:
        result = verify_pdf(args.artifact, args.expected_text)
    except (OSError, ValueError, RuntimeError, ImportError):
        print(json.dumps({"schema": SCHEMA, "status": "FAIL",
                          "reason": "pdf_authored_text_not_proven"}))
        return 1
    print(json.dumps(result, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
