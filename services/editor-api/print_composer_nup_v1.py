#!/usr/bin/env python3
"""Bounded A4 repeated-copy sheet geometry for the current Editor revision.

Consumes the canonical imposition_plan_v1; it never invents a second
page-to-sheet planner. The upstream fixed-output producer owns actual edited
PDF bytes; this consumer records deterministic sheet placement geometry only.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
import re
from typing import Literal

from imposition_plan_v1 import plan_repeated_n_up_v1

PRINT_COMPOSER_NUP_SCHEMA_V1 = "chaptera.editor-print-composer-nup.v1"
EMU_PER_MM = 36_000
CARD_WIDTH_MM = 85
CARD_HEIGHT_MM = 55
GRID_ROWS = 3
GRID_COLUMNS = 3
DEFAULT_GAP_MM = 5
A4_PORTRAIT_MM = (210, 297)
A4_LANDSCAPE_MM = (297, 210)
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}\Z")


class PrintComposerNupError(ValueError):
    """Invalid identity/recipe or a broken upstream imposition contract."""


class PrintComposerCannotFitError(PrintComposerNupError):
    """An exact, unscaled grid cannot fit the requested physical sheet."""


@dataclass(frozen=True)
class SheetPlacementV1:
    logical_page_id: str
    sheet_index: int
    side: Literal["front"]
    slot_index: int
    row: int
    column: int
    rotation_quarter_turns: int
    x_emu: int
    y_top_emu: int
    width_emu: int
    height_emu: int

    def pdf_bottom_left_y_emu(self, sheet_height_emu: int) -> int:
        """Transform top-origin placement to bottom-origin PDF coordinates."""
        return sheet_height_emu - self.y_top_emu - self.height_emu


@dataclass(frozen=True)
class EditorNupSheetV1:
    source_pub_sha256: str
    editor_revision_id: str
    current_fixed_pdf_sha256: str
    logical_page_id: str
    orientation: Literal["portrait", "landscape"]
    sheet_width_emu: int
    sheet_height_emu: int
    gap_emu: int
    placements: tuple[SheetPlacementV1, ...]

    def receipt(self) -> dict[str, object]:
        """Source-safe deterministic handoff for future preview/PDF consumer."""
        payload: dict[str, object] = {
            "protocol_version": PRINT_COMPOSER_NUP_SCHEMA_V1,
            "source_pub_sha256": self.source_pub_sha256,
            "editor_revision_id": self.editor_revision_id,
            "current_fixed_pdf_sha256": self.current_fixed_pdf_sha256,
            "logical_page_ids": [self.logical_page_id],
            "sheet": {
                "format": "iso_a4",
                "orientation": self.orientation,
                "width_emu": self.sheet_width_emu,
                "height_emu": self.sheet_height_emu,
                "side": "front",
            },
            "recipe": {
                "mode": "repeated_n_up",
                "rows": GRID_ROWS,
                "columns": GRID_COLUMNS,
                "copies": GRID_ROWS * GRID_COLUMNS,
                "gap_emu": self.gap_emu,
                "scale": "1:1",
            },
            "placements": [asdict(p) for p in self.placements],
        }
        digest = hashlib.sha256(
            json.dumps(
                payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False
            ).encode("utf-8")
        ).hexdigest()
        return {**payload, "plan_sha256": digest}


def _hash_identity(value: str, name: str) -> str:
    if not isinstance(value, str) or SHA256_PATTERN.fullmatch(value) is None:
        raise PrintComposerNupError(
            f"{name} must be a lowercase 64-character SHA-256"
        )
    return value


def _opaque_id(value: str, name: str) -> str:
    if (
        not isinstance(value, str)
        or not value.strip()
        or len(value) > 256
        or any(ord(character) < 0x20 for character in value)
    ):
        raise PrintComposerNupError(
            f"{name} must be a non-empty bounded identifier"
        )
    return value


def plan_editor_card_a4_9up_v1(
    *,
    source_pub_sha256: str,
    editor_revision_id: str,
    current_fixed_pdf_sha256: str,
    logical_page_id: str,
    orientation: Literal["portrait", "landscape"] = "landscape",
    gap_mm: int = DEFAULT_GAP_MM,
) -> EditorNupSheetV1:
    """Bind exact 85x55mm cards to an existing edited single-page PDF identity.

    Caller must supply the PDF hash of the accepted *current* EditorSession
    revision. The function does not produce PDF bytes, modify the source PUB
    or duplicate authoring pages.
    """
    source_pub_sha256 = _hash_identity(source_pub_sha256, "source_pub_sha256")
    current_fixed_pdf_sha256 = _hash_identity(
        current_fixed_pdf_sha256, "current_fixed_pdf_sha256"
    )
    editor_revision_id = _opaque_id(editor_revision_id, "editor_revision_id")
    logical_page_id = _opaque_id(logical_page_id, "logical_page_id")
    if orientation not in ("portrait", "landscape"):
        raise PrintComposerNupError("orientation must be portrait or landscape")
    if type(gap_mm) is not int or gap_mm < 0:
        raise PrintComposerNupError("gap_mm must be a non-negative integer")

    sheet_width_mm, sheet_height_mm = (
        A4_LANDSCAPE_MM if orientation == "landscape" else A4_PORTRAIT_MM
    )
    content_width_mm = GRID_COLUMNS * CARD_WIDTH_MM + (GRID_COLUMNS - 1) * gap_mm
    content_height_mm = GRID_ROWS * CARD_HEIGHT_MM + (GRID_ROWS - 1) * gap_mm
    if content_width_mm > sheet_width_mm or content_height_mm > sheet_height_mm:
        raise PrintComposerCannotFitError(
            "exact 85x55 mm 3x3 repeated-copy grid cannot fit A4 " + orientation
        )

    planner = plan_repeated_n_up_v1(
        logical_page_id=logical_page_id,
        copies=GRID_ROWS * GRID_COLUMNS,
        rows=GRID_ROWS,
        columns=GRID_COLUMNS,
    )
    if (
        planner.mode != "repeated_n_up"
        or planner.logical_page_ids != (logical_page_id,)
        or len(planner.sheets) != 1
        or planner.placement_count != GRID_ROWS * GRID_COLUMNS
    ):
        raise PrintComposerNupError(
            "canonical imposition planner returned an incompatible recipe"
        )
    front = planner.sheets[0].front
    if (
        front.rows != GRID_ROWS
        or front.columns != GRID_COLUMNS
        or planner.sheets[0].back is not None
    ):
        raise PrintComposerNupError(
            "canonical imposition planner returned an incompatible sheet"
        )

    margin_x_emu = (sheet_width_mm - content_width_mm) * EMU_PER_MM // 2
    margin_y_emu = (sheet_height_mm - content_height_mm) * EMU_PER_MM // 2
    placements = tuple(
        SheetPlacementV1(
            logical_page_id=item.logical_page_id,
            sheet_index=item.sheet_index,
            side="front",
            slot_index=item.slot_index,
            row=item.row,
            column=item.column,
            rotation_quarter_turns=item.rotation_quarter_turns,
            x_emu=margin_x_emu
            + item.column * (CARD_WIDTH_MM + gap_mm) * EMU_PER_MM,
            y_top_emu=margin_y_emu
            + item.row * (CARD_HEIGHT_MM + gap_mm) * EMU_PER_MM,
            width_emu=CARD_WIDTH_MM * EMU_PER_MM,
            height_emu=CARD_HEIGHT_MM * EMU_PER_MM,
        )
        for item in front.placements
    )
    if any(
        item.logical_page_id != logical_page_id
        or item.rotation_quarter_turns != 0
        or item.row != i // GRID_COLUMNS
        or item.column != i % GRID_COLUMNS
        or item.sheet_index != 0
        or item.slot_index != i
        for i, item in enumerate(placements)
    ):
        raise PrintComposerNupError(
            "canonical imposition planner returned non-row-major placements"
        )
    return EditorNupSheetV1(
        source_pub_sha256=source_pub_sha256,
        editor_revision_id=editor_revision_id,
        current_fixed_pdf_sha256=current_fixed_pdf_sha256,
        logical_page_id=logical_page_id,
        orientation=orientation,
        sheet_width_emu=sheet_width_mm * EMU_PER_MM,
        sheet_height_emu=sheet_height_mm * EMU_PER_MM,
        gap_emu=gap_mm * EMU_PER_MM,
        placements=placements,
    )
