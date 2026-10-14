#!/usr/bin/env python3
"""Source-safe 85x55mm card / 3x3 A4 imposition consumer checks."""
import unittest
from print_composer_nup_v1 import (
    EMU_PER_MM,
    PrintComposerCannotFitError,
    PrintComposerNupError,
    plan_editor_card_a4_9up_v1,
)

SOURCE = "a" * 64
FIXED_A = "b" * 64
FIXED_B = "c" * 64


def plan(**overrides):
    inputs = dict(
        source_pub_sha256=SOURCE,
        editor_revision_id="editor-state:accepted-42",
        current_fixed_pdf_sha256=FIXED_A,
        logical_page_id="source-page-1",
    )
    inputs.update(overrides)
    return plan_editor_card_a4_9up_v1(**inputs)


class EditorCard9UpTests(unittest.TestCase):
    def test_one_page_nine_exact_non_overlapping_placements(self):
        result = plan()
        self.assertEqual(
            (297 * EMU_PER_MM, 210 * EMU_PER_MM),
            (result.sheet_width_emu, result.sheet_height_emu),
        )
        self.assertEqual(9, len(result.placements))
        self.assertEqual(
            {"source-page-1"}, {p.logical_page_id for p in result.placements}
        )
        self.assertEqual(
            [(r, c) for r in range(3) for c in range(3)],
            [(p.row, p.column) for p in result.placements],
        )
        self.assertEqual(
            [(i, 0, "front") for i in range(9)],
            [(p.slot_index, p.sheet_index, p.side) for p in result.placements],
        )
        self.assertEqual(
            {(85 * EMU_PER_MM, 55 * EMU_PER_MM)},
            {(p.width_emu, p.height_emu) for p in result.placements},
        )
        self.assertEqual(
            (16 * EMU_PER_MM, 17_500 * 36),
            (result.placements[0].x_emu, result.placements[0].y_top_emu),
        )
        self.assertEqual(
            (196 * EMU_PER_MM, 137_500 * 36),
            (result.placements[-1].x_emu, result.placements[-1].y_top_emu),
        )
        for i, a in enumerate(result.placements):
            self.assertGreaterEqual(a.x_emu, 0)
            self.assertGreaterEqual(a.y_top_emu, 0)
            self.assertLessEqual(
                a.x_emu + a.width_emu, result.sheet_width_emu
            )
            self.assertLessEqual(
                a.y_top_emu + a.height_emu, result.sheet_height_emu
            )
            self.assertGreaterEqual(
                a.pdf_bottom_left_y_emu(result.sheet_height_emu), 0
            )
            for b in result.placements[i + 1 :]:
                overlap = (
                    a.x_emu < b.x_emu + b.width_emu
                    and b.x_emu < a.x_emu + a.width_emu
                    and a.y_top_emu < b.y_top_emu + b.height_emu
                    and b.y_top_emu < a.y_top_emu + a.height_emu
                )
                self.assertFalse(overlap)

    def test_receipt_binds_current_revision_and_current_pdf(self):
        baseline = plan().receipt()
        self.assertEqual(baseline, plan().receipt())
        self.assertEqual(
            "chaptera.editor-print-composer-nup.v1", baseline["protocol_version"]
        )
        self.assertEqual(["source-page-1"], baseline["logical_page_ids"])
        self.assertEqual("repeated_n_up", baseline["recipe"]["mode"])
        self.assertEqual("1:1", baseline["recipe"]["scale"])
        self.assertEqual(9, len(baseline["placements"]))
        self.assertNotEqual(
            baseline["plan_sha256"],
            plan(editor_revision_id="editor-state:accepted-43").receipt()[
                "plan_sha256"
            ],
        )
        self.assertNotEqual(
            baseline["plan_sha256"],
            plan(current_fixed_pdf_sha256=FIXED_B).receipt()["plan_sha256"],
        )

    def test_cannot_fit_does_not_rotate_or_scale(self):
        with self.assertRaises(PrintComposerCannotFitError):
            plan(orientation="portrait")
        with self.assertRaises(PrintComposerCannotFitError):
            plan(gap_mm=22)
        self.assertEqual(9, len(plan(gap_mm=0).placements))

    def test_invalid_identity_and_recipe_fail_explicitly(self):
        for change in (
            {"source_pub_sha256": "not a hash"},
            {"source_pub_sha256": "A" * 64},
            {"current_fixed_pdf_sha256": ""},
            {"editor_revision_id": ""},
            {"logical_page_id": "\x00"},
            {"orientation": "automatic"},
            {"gap_mm": -1},
            {"gap_mm": True},
            {"gap_mm": 0.5},
        ):
            with self.subTest(change=change), self.assertRaises(
                PrintComposerNupError
            ):
                plan(**change)


if __name__ == "__main__":
    unittest.main()
