#!/usr/bin/env python3
"""Bind a private, authenticated guest Editor PDF/scene report to A4 nine-up.

The caller must obtain the PDF, report and scene from the SAME authorized
current temporary session. JSON by itself is not an authentication token.
This module does not call the server, parse PUB or alter authoring content.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re

from print_composer_nup_current_revision_v1 import read_regular_bounded
from print_composer_nup_output_v1 import NupOutputPublishError, publish_nup_outputs_v1
from print_composer_nup_pdf_v1 import (
    ComposedNupPdfV1, MAX_FIXED_PDF_BYTES, compose_editor_card_a4_9up_pdf_v1,
)
from print_composer_nup_v1 import PrintComposerNupError, plan_editor_card_a4_9up_v1

PROTOCOL = "chaptera.temporary-edited-export-report.v1"
LOSS_SCHEMA = "chaptera.temporary-edited-fixed-pdf-loss.v1"
READER_SCENE = "chaptera.reader-scene.v1"
GUEST_BINDING = "chaptera.guest-current-pdf-to-a4-nine-up.v1"
MAX_SOURCE = 32 * 1024 * 1024
MAX_REPORT = 256 * 1024
MAX_SCENE = 16 * 1024 * 1024
HASH = re.compile(r"[0-9a-f]{64}\Z")
REVISION = re.compile(r"sha256:[0-9a-f]{64}\Z")
@dataclass(frozen=True)
class GuestNupBindingV1:
    source_sha256: str
    revision_id: str
    canonical_authoring_revision_id: str
    logical_page_id: str
    export_receipt_sha256: str
    upstream_fidelity: str
    limitations: tuple[str, ...]


LOSS_COUNT_KEYS = (
    "blocking", "partial", "unsupported", "font_missing_glyph_lines",
    "layout_warnings", "pdf_warnings",
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(value: object) -> bytes:
    try:
        return json.dumps(value, sort_keys=True, separators=(",", ":"),
                          ensure_ascii=False, allow_nan=False).encode("utf-8")
    except (TypeError, ValueError, UnicodeError) as exc:
        raise PrintComposerNupError("guest_invalid_json") from exc


def require_object(value: object, name: str) -> dict:
    if not isinstance(value, dict):
        raise PrintComposerNupError("guest_invalid_" + name)
    return value


def require_equal(actual: object, expected: object, name: str) -> None:
    if type(actual) is not type(expected) or actual != expected:
        raise PrintComposerNupError("guest_" + name + "_mismatch")


def require_hash(value: object, name: str, *, revision: bool = False) -> str:
    if not isinstance(value, str) or not (
        REVISION.fullmatch(value) if revision else HASH.fullmatch(value)
    ):
        raise PrintComposerNupError("guest_invalid_" + name)
    return value


def validate_guest_current_nup_v1(
    *, original_pub_bytes: bytes, current_fixed_pdf_bytes: bytes,
    private_export_report: dict, current_reader_scene: dict,
) -> GuestNupBindingV1:
    """Validate exact hashes, revision, one 85x55mm page and loss disclosure."""
    if not isinstance(original_pub_bytes, bytes) or not (0 < len(original_pub_bytes) <= MAX_SOURCE):
        raise PrintComposerNupError("guest_invalid_source_size")
    if not isinstance(current_fixed_pdf_bytes, bytes) or not (
        40 <= len(current_fixed_pdf_bytes) <= MAX_FIXED_PDF_BYTES
    ):
        raise PrintComposerNupError("guest_invalid_pdf_size")
    report = require_object(private_export_report, "report")
    scene = require_object(current_reader_scene, "scene")
    if len(canonical_json(report)) > MAX_REPORT or len(canonical_json(scene)) > MAX_SCENE:
        raise PrintComposerNupError("guest_private_input_oversize")
    require_equal(report.get("protocol_version"), PROTOCOL, "report_protocol")
    require_equal(report.get("target"), "pdf", "report_target")
    revision = require_hash(report.get("revision_id"), "revision_id", revision=True)
    authoring = require_hash(report.get("canonical_authoring_revision_id"), "authoring_id")
    source_sha = digest(original_pub_bytes)
    require_equal(require_hash(report.get("source_hash"), "source_hash"), source_sha, "source_hash")
    require_equal(require_hash(report.get("artifact_sha256"), "pdf_hash"),
                  digest(current_fixed_pdf_bytes), "artifact")
    if type(report.get("expires_at_ms")) is not int or report["expires_at_ms"] <= 0:
        raise PrintComposerNupError("guest_invalid_expiry")

    loss = require_object(report.get("loss_report"), "loss_report")
    require_equal(require_hash(report.get("loss_report_sha256"), "loss_hash"),
                  digest(canonical_json(loss)), "loss_hash")
    for name, expected in (
        ("schema_version", LOSS_SCHEMA), ("target", "pdf"),
        ("renderer", "pub-pdf-v0.1"), ("source_hash", source_sha),
        ("current_editor_state", True), ("can_serialize", True),
    ):
        require_equal(loss.get(name), expected, "loss_" + name)
    counts = require_object(loss.get("counts"), "counts")
    for name in (*LOSS_COUNT_KEYS, "pages", "painted", "shaped_lines", "moved_nodes", "edited_stories"):
        value = counts.get(name)
        if type(value) is not int or value < 0 or value > 100_000:
            raise PrintComposerNupError("guest_invalid_loss_count")
    if counts["pages"] != 1:
        raise PrintComposerNupError("guest_invalid_pdf_page_count")
    if any(counts[name] for name in LOSS_COUNT_KEYS):
        raise PrintComposerNupError("guest_unresolved_visual_loss")
    fidelity = loss.get("fidelity")
    if fidelity not in ("partial", "bounded"):
        raise PrintComposerNupError("guest_invalid_fidelity")
    limitations = loss.get("limitations")
    if not isinstance(limitations, list) or len(limitations) > 20 or any(
        not isinstance(code, str) or not 0 < len(code) <= 128 or
        not re.fullmatch(r"[a-z0-9_]+", code) for code in limitations
    ):
        raise PrintComposerNupError("guest_invalid_limitations")

    require_equal(scene.get("protocol_version"), READER_SCENE, "scene_protocol")
    require_equal(scene.get("source_hash"), source_sha, "scene_source")
    require_equal(scene.get("revision_id"), revision, "scene_revision")
    pages = scene.get("pages")
    if not isinstance(pages, list) or len(pages) != 1:
        raise PrintComposerNupError("guest_scene_page_count_not_one")
    page = require_object(pages[0], "page")
    require_equal(page.get("width_emu"), 85 * 36_000, "page_width")
    require_equal(page.get("height_emu"), 55 * 36_000, "page_height")
    logical_id = page.get("page_id")
    if not isinstance(logical_id, str) or not 0 < len(logical_id) <= 256 or any(
        ord(ch) < 32 for ch in logical_id
    ):
        raise PrintComposerNupError("guest_invalid_page_id")
    return GuestNupBindingV1(
        source_sha256=source_sha, revision_id=revision,
        canonical_authoring_revision_id=authoring, logical_page_id=logical_id,
        export_receipt_sha256=digest(canonical_json(report)),
        upstream_fidelity=fidelity, limitations=tuple(limitations),
    )


def compose_guest_current_nup_v1(
    *, original_pub_bytes: bytes, current_fixed_pdf_bytes: bytes,
    private_export_report: dict, current_reader_scene: dict, gap_mm: int = 5,
) -> ComposedNupPdfV1:
    bound = validate_guest_current_nup_v1(
        original_pub_bytes=original_pub_bytes, current_fixed_pdf_bytes=current_fixed_pdf_bytes,
        private_export_report=private_export_report, current_reader_scene=current_reader_scene,
    )
    plan = plan_editor_card_a4_9up_v1(
        source_pub_sha256=bound.source_sha256,
        editor_revision_id=bound.revision_id,
        current_fixed_pdf_sha256=digest(current_fixed_pdf_bytes),
        logical_page_id=bound.logical_page_id, gap_mm=gap_mm,
    )
    product = compose_editor_card_a4_9up_pdf_v1(
        plan=plan, current_fixed_pdf_bytes=current_fixed_pdf_bytes,
        source_pub_bytes=original_pub_bytes,
    )
    return ComposedNupPdfV1(product.pdf_bytes, product.preview_png_bytes, {
        **product.receipt,
        "upstream_guest": {
            "protocol_version": GUEST_BINDING,
            "revision_id": bound.revision_id,
            "canonical_authoring_revision_id": bound.canonical_authoring_revision_id,
            "source_sha256": bound.source_sha256,
            "private_export_report_sha256": bound.export_receipt_sha256,
            "upstream_fidelity": bound.upstream_fidelity,
            "upstream_limitations": list(bound.limitations),
            "proof_class": "trusted_guest_pipeline_structural_binding",
        },
        "loss_diagnostics": (["upstream_guest_fidelity_partial"]
                             if bound.upstream_fidelity == "partial" else []),
    })


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source-pub", "current-fixed-pdf", "private-export-report",
                 "current-reader-scene", "output-pdf", "output-preview", "output-receipt"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--gap-mm", type=int, default=5)
    args = parser.parse_args()
    inputs = [args.source_pub, args.current_fixed_pdf,
              args.private_export_report, args.current_reader_scene]
    outputs = [args.output_pdf, args.output_preview, args.output_receipt]
    try:
        snapshots = [read_regular_bounded(path, limit) for path, limit in zip(
            inputs, (MAX_SOURCE, MAX_FIXED_PDF_BYTES, MAX_REPORT, MAX_SCENE)
        )]
        try:
            report, scene = json.loads(snapshots[2]), json.loads(snapshots[3])
        except (ValueError, UnicodeError) as exc:
            raise PrintComposerNupError("guest_input_json_invalid") from exc
        result = compose_guest_current_nup_v1(
            original_pub_bytes=snapshots[0], current_fixed_pdf_bytes=snapshots[1],
            private_export_report=report, current_reader_scene=scene, gap_mm=args.gap_mm,
        )
        if any(read_regular_bounded(path, limit) != before for path, limit, before in zip(
            inputs, (MAX_SOURCE, MAX_FIXED_PDF_BYTES, MAX_REPORT, MAX_SCENE), snapshots
        )):
            raise PrintComposerNupError("guest_input_changed_during_composition")
        publish_nup_outputs_v1(inputs=inputs, outputs=[
            (outputs[0], result.pdf_bytes), (outputs[1], result.preview_png_bytes),
            (outputs[2], (json.dumps(result.receipt, sort_keys=True, indent=2) + "\n").encode()),
        ])
        print(json.dumps({"ok": True, "pdf_sha256": digest(result.pdf_bytes),
                          "preview_sha256": digest(result.preview_png_bytes)}, sort_keys=True))
        return 0
    except (OSError, ValueError, UnicodeError) as exc:
        code = str(exc) if isinstance(exc, (PrintComposerNupError, NupOutputPublishError)) else "guest_input_or_file_error"
        print(json.dumps({"ok": False, "error": code}, sort_keys=True))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
