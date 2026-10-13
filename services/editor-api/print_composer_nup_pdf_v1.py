#!/usr/bin/env python3
"""Compose an actual A4 sheet from the accepted *current-revision* fixed PDF.

Only PDF-to-PDF placement is performed here. The canonical imposition planner
owns all rectangles; the current EditorSession fixed-output producer owns the
source PDF. Embedded text, paint, and images stay in the PDF, not a raster copy.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import stat

from print_composer_nup_v1 import (
    EMU_PER_MM,
    EditorNupSheetV1,
    PrintComposerNupError,
    plan_editor_card_a4_9up_v1,
)

EMU_PER_POINT = 12_700
MAX_FIXED_PDF_BYTES = 16 * 1024 * 1024
MAX_IMPOSED_PDF_BYTES = 32 * 1024 * 1024
MAX_PREVIEW_PNG_BYTES = 8 * 1024 * 1024
CARD_SIZE_TOLERANCE_POINTS = 0.1
PREVIEW_SCALE = 1.0


@dataclass(frozen=True)
class ComposedNupPdfV1:
    pdf_bytes: bytes
    preview_png_bytes: bytes
    receipt: dict[str, object]


def _canonical_plan(plan: EditorNupSheetV1) -> None:
    """Do not accept caller-constructed rectangles as a substitute for planner."""
    if not isinstance(plan, EditorNupSheetV1):
        raise PrintComposerNupError("invalid_nup_plan")
    if plan.gap_emu < 0 or plan.gap_emu % EMU_PER_MM:
        raise PrintComposerNupError("invalid_nup_plan_gap")
    expected = plan_editor_card_a4_9up_v1(
        source_pub_sha256=plan.source_pub_sha256,
        editor_revision_id=plan.editor_revision_id,
        current_fixed_pdf_sha256=plan.current_fixed_pdf_sha256,
        logical_page_id=plan.logical_page_id,
        orientation=plan.orientation,
        gap_mm=plan.gap_emu // EMU_PER_MM,
    )
    if expected != plan:
        raise PrintComposerNupError("noncanonical_nup_placement")


def compose_editor_card_a4_9up_pdf_v1(
    *,
    plan: EditorNupSheetV1,
    current_fixed_pdf_bytes: bytes,
    source_pub_bytes: bytes | None = None,
) -> ComposedNupPdfV1:
    """Impose one exact 85x55mm CURRENT edited page into one A4 PDF + PNG.

    current_fixed_pdf_bytes must be supplied by the upstream canonical
    EditorSession fixed-output path; this function does not accept or parse PUB.
    source_pub_bytes is optional independent immutability verification.
    """
    _canonical_plan(plan)
    if not isinstance(current_fixed_pdf_bytes, bytes) or not (
        40 <= len(current_fixed_pdf_bytes) <= MAX_FIXED_PDF_BYTES
    ):
        raise PrintComposerNupError("fixed_pdf_size_out_of_bounds")
    actual_pdf_hash = hashlib.sha256(current_fixed_pdf_bytes).hexdigest()
    if actual_pdf_hash != plan.current_fixed_pdf_sha256:
        raise PrintComposerNupError("fixed_pdf_revision_binding_mismatch")
    if source_pub_bytes is not None:
        if not isinstance(source_pub_bytes, bytes):
            raise PrintComposerNupError("source_pub_bytes_invalid")
        if hashlib.sha256(source_pub_bytes).hexdigest() != plan.source_pub_sha256:
            raise PrintComposerNupError("source_pub_identity_mismatch")

    try:
        import fitz  # PyMuPDF, already used by the repo's PDF research tools
    except ImportError as exc:
        raise PrintComposerNupError("pymupdf_1_26_7_required") from exc

    source = None
    target = None
    try:
        try:
            source = fitz.open(stream=current_fixed_pdf_bytes, filetype="pdf")
        except Exception as exc:
            raise PrintComposerNupError("fixed_pdf_invalid") from exc
        if source.needs_pass or source.is_encrypted:
            raise PrintComposerNupError("fixed_pdf_encrypted")
        if source.is_repaired:
            raise PrintComposerNupError("fixed_pdf_was_repaired")
        if len(source) != 1:
            raise PrintComposerNupError("fixed_pdf_must_have_one_logical_page")
        page = source[0]
        if page.rotation != 0:
            raise PrintComposerNupError("fixed_pdf_page_rotation_unsupported")
        if page.first_annot is not None or page.first_widget is not None or page.get_links():
            raise PrintComposerNupError("fixed_pdf_interactive_elements_unsupported")
        width_points = 85 * EMU_PER_MM / EMU_PER_POINT
        height_points = 55 * EMU_PER_MM / EMU_PER_POINT
        if (
            abs(page.rect.width - width_points) > CARD_SIZE_TOLERANCE_POINTS
            or abs(page.rect.height - height_points) > CARD_SIZE_TOLERANCE_POINTS
        ):
            raise PrintComposerNupError("fixed_pdf_card_size_mismatch")

        target = fitz.open()
        sheet = target.new_page(
            width=plan.sheet_width_emu / EMU_PER_POINT,
            height=plan.sheet_height_emu / EMU_PER_POINT,
        )
        for placement in plan.placements:
            rect = fitz.Rect(
                placement.x_emu / EMU_PER_POINT,
                placement.y_top_emu / EMU_PER_POINT,
                (placement.x_emu + placement.width_emu) / EMU_PER_POINT,
                (placement.y_top_emu + placement.height_emu) / EMU_PER_POINT,
            )
            sheet.show_pdf_page(rect, source, 0, keep_proportion=True, overlay=True)
        pdf_bytes = target.tobytes(garbage=4, deflate=True, no_new_id=True)
        if len(pdf_bytes) > MAX_IMPOSED_PDF_BYTES:
            raise PrintComposerNupError("imposed_pdf_size_out_of_bounds")
        # Render the actual imposed PDF for sheet preview, not a separate
        # raster-layout interpretation that might disagree with printing.
        raster = sheet.get_pixmap(
            matrix=fitz.Matrix(PREVIEW_SCALE, PREVIEW_SCALE), alpha=False
        )
        preview_bytes = raster.tobytes("png")
        if len(preview_bytes) > MAX_PREVIEW_PNG_BYTES:
            raise PrintComposerNupError("preview_png_size_out_of_bounds")
    finally:
        if target is not None:
            target.close()
        if source is not None:
            source.close()

    if source_pub_bytes is not None and (
        hashlib.sha256(source_pub_bytes).hexdigest() != plan.source_pub_sha256
    ):
        raise PrintComposerNupError("original_source_mutated")
    receipt = {
        **plan.receipt(),
        "pdf": {
            "sha256": hashlib.sha256(pdf_bytes).hexdigest(),
            "byte_len": len(pdf_bytes),
            "pages": 1,
            "vector_pdf_placement": True,
            "producer": "current_editor_fixed_pdf",
        },
        "preview": {
            "format": "png",
            "sha256": hashlib.sha256(preview_bytes).hexdigest(),
            "byte_len": len(preview_bytes),
            "from_imposed_pdf": True,
        },
        "original_pub_hash_rechecked": source_pub_bytes is not None,
        "loss_diagnostics": [],
    }
    return ComposedNupPdfV1(pdf_bytes, preview_bytes, receipt)


def _read_regular_bounded(path: Path, limit: int) -> bytes:
    """Bound locally generated inputs; reject symlinks/FIFOs, not PUB parsing."""
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
        raise PrintComposerNupError("input_is_not_bounded_regular_file")
    with path.open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise PrintComposerNupError("input_size_out_of_bounds")
    return data


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current-fixed-pdf", type=Path, required=True)
    parser.add_argument("--source-pub", type=Path)
    parser.add_argument("--source-pub-sha256", required=True)
    parser.add_argument("--editor-revision-id", required=True)
    parser.add_argument("--logical-page-id", required=True)
    parser.add_argument("--gap-mm", type=int, default=5)
    parser.add_argument("--output-pdf", type=Path, required=True)
    parser.add_argument("--output-preview", type=Path, required=True)
    parser.add_argument("--output-receipt", type=Path, required=True)
    args = parser.parse_args()
    try:
        fixed = _read_regular_bounded(args.current_fixed_pdf, MAX_FIXED_PDF_BYTES)
        source = (
            _read_regular_bounded(args.source_pub, 256 * 1024 * 1024)
            if args.source_pub is not None
            else None
        )
        inputs = [args.current_fixed_pdf]
        if args.source_pub is not None:
            inputs.append(args.source_pub)
        outputs = [args.output_pdf, args.output_preview, args.output_receipt]
        all_paths = [p.resolve() for p in [*inputs, *outputs]]
        if len(set(all_paths)) != len(all_paths):
            raise PrintComposerNupError("source_and_output_paths_must_differ")
        plan = plan_editor_card_a4_9up_v1(
            source_pub_sha256=args.source_pub_sha256,
            editor_revision_id=args.editor_revision_id,
            current_fixed_pdf_sha256=hashlib.sha256(fixed).hexdigest(),
            logical_page_id=args.logical_page_id,
            gap_mm=args.gap_mm,
        )
        product = compose_editor_card_a4_9up_pdf_v1(
            plan=plan, current_fixed_pdf_bytes=fixed, source_pub_bytes=source
        )
        for path, payload in (
            (args.output_pdf, product.pdf_bytes),
            (args.output_preview, product.preview_png_bytes),
            (args.output_receipt, (json.dumps(product.receipt, sort_keys=True, indent=2) + "\n").encode()),
        ):
            if not path.parent.exists():
                raise PrintComposerNupError("output_parent_does_not_exist")
            path.write_bytes(payload)
        print(json.dumps(product.receipt, sort_keys=True))
        return 0
    except (PrintComposerNupError, OSError) as exc:
        # No customer file paths or text in the diagnostic channel.
        code = str(exc) if isinstance(exc, PrintComposerNupError) else "file_io_error"
        print(json.dumps({"error": code, "ok": False}, sort_keys=True))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
