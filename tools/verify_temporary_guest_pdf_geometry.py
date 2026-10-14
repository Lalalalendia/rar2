#!/usr/bin/env python3
"""Independent PDF consumer for real-Chromium Guest MoveNode downloads.

Reads the physical file Playwright saved, not the Editor Scene or an export
receipt. Shares the established native PDF xref/painted-rectangle verifier used
by the real HTTPS worker acceptance. A valid %PDF header alone is insufficient.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from run_temporary_editor_current_acceptance import verify_physical_edited_pdf
from verify_editable_export_geometry import RectEmu

MAX_PDF_BYTES = 16 * 1024 * 1024


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", type=Path, required=True)
    for name in ("x", "y", "width", "height", "before-x", "before-y"):
        parser.add_argument("--" + name + "-emu", type=int, required=True)
    args = parser.parse_args()
    if not args.artifact.is_file():
        raise AssertionError("physical browser PDF download is missing")
    size = args.artifact.stat().st_size
    if size < 40 or size > MAX_PDF_BYTES:
        raise AssertionError("physical browser PDF violates bounded byte policy")
    expected = RectEmu(
        x=args.x_emu, y=args.y_emu,
        width=args.width_emu, height=args.height_emu,
    )
    if expected.width <= 0 or expected.height <= 0:
        raise AssertionError("edited physical rectangle has invalid dimensions")
    if (expected.x, expected.y) == (args.before_x_emu, args.before_y_emu):
        raise AssertionError("PDF consumer received unchanged source geometry")
    payload = args.artifact.read_bytes()
    if len(payload) != size:
        raise AssertionError("PDF file size drifted during consumer read")
    proof = verify_physical_edited_pdf(payload, expected)
    if proof["geometry_form"] != "pdf_rectangle_stream" or proof["pages"] < 1:
        raise AssertionError("physical PDF produced no current revision rectangle")
    print(json.dumps({
        "receipt_kind": "chaptera.tempedit-guest-pdf-physical-geometry.v1",
        "target": "pdf",
        "artifact_sha256": hashlib.sha256(payload).hexdigest(),
        "artifact_byte_len": size,
        "expected_rect_emu": {
            "x": expected.x, "y": expected.y,
            "width": expected.width, "height": expected.height,
        },
        "source_position_changed": True,
        "geometry_form": proof["geometry_form"],
        "pages": proof["pages"],
        "geometry_matches_edit": True,
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
