#!/usr/bin/env python3
"""Source-neutral contract tests: no private PUB/PDF/images/Story text."""
import copy
import unittest

from pdf_paint_trace_roi_join_v1 import TraceJoinError, join_paint_trace


def fixture():
    w, h = 100, 100
    def event(node, index, resource):
        return {
            "page_origin": "page-synthetic-01",
            "node_origin": node,
            "page_paint_index": index,
            "kind": "image",
            "node_bounds_emu": [17 * 12_700, 17 * 12_700, 53 * 12_700, 53 * 12_700],
            "resource_origin": resource,
            "text_run_ordinal": None,
            "unwrapped_program_byte_range": [index * 60, (index + 1) * 60],
            "output_sheet_copies": 1,
        }

    trace = {
        "schema_version": "chaptera.pdf-paint-trace.v1",
        "candidate_pdf_sha256": "a" * 64,
        "pages": [{
            "origin": "page-synthetic-01",
            "media_box_points": ["0", "0", str(w), str(h)],
            "width_emu": w * 12_700,
            "height_emu": h * 12_700,
        }],
        "paint_trace": [
            event("background-node-higher-identity", 0, "library-resource"),
            event("foreground-node-lower-identity", 1, "team-resource"),
        ],
    }
    # Significant in tiles (0,0), (2,2) and (6,6) at 72 dpi.
    oracle = {
        "receipt_version": "chaptera.fidelity-reference-pdf.v1",
        "candidate_sha256": "a" * 64,
        "renderer": {"engine": "MuPDF", "colorspace": "RGB", "alpha": False, "raster_dpi": 72},
        "page_count": {"candidate": 1, "reference": 1, "match": True},
        "pages": [{
            "page_index": 0,
            "page_boxes_match": True,
            "candidate_boxes": {
                "rotation": 0,
                "render_rect": {
                    "x0_pt": 0, "y0_pt": 0, "width_pt": w, "height_pt": h
                },
            },
            "diff": {
                "raster_size_match": True,
                "significant_pixel_count": 48,
                "significant_mask": {
                    "encoding": "row-major-tile-rle-v1",
                    "tile_px": 16, "tile_width": 7, "tile_height": 7,
                    "runs": [[0, 1], [16, 1], [48, 1]],
                },
            },
        }],
    }
    return trace, oracle


class PaintTraceRoiJoinTests(unittest.TestCase):
    def test_overlapping_hero_candidates_have_ambiguity_and_last_painter(self):
        trace, oracle = fixture()
        first = join_paint_trace(trace, oracle)
        self.assertEqual(first, join_paint_trace(trace, oracle), "stable source-neutral replay")
        page = first["pages"][0]
        self.assertEqual(page["significant_pixel_count"], 48)
        self.assertEqual(page["significant_tile_count"], 3)
        self.assertEqual(page["unmatched_significant_tile_count"], 2)
        self.assertEqual(page["ambiguous_significant_tile_count"], 1)
        self.assertEqual(len(page["candidate_paint_events"]), 2)
        back, front = page["candidate_paint_events"]
        self.assertEqual(back["resource_origin"], "library-resource")
        self.assertEqual(front["resource_origin"], "team-resource")
        self.assertEqual(back["candidate_significant_tile_count"], 1)
        self.assertEqual(front["candidate_significant_tile_count"], 1)
        self.assertEqual(back["last_painted_envelope_candidate_tiles"], 0)
        self.assertEqual(front["last_painted_envelope_candidate_tiles"], 1)
        self.assertIn("non-additive", first["limitations"][1])
        self.assertNotIn("proven", str(page))

    def test_inverse_order_keeps_candidates_but_changes_last_painted(self):
        trace, oracle = fixture()
        a, b = trace["paint_trace"]
        trace["paint_trace"] = [
            {**b, "page_paint_index": 0},
            {**a, "page_paint_index": 1},
        ]
        rows = join_paint_trace(trace, oracle)["pages"][0]["candidate_paint_events"]
        self.assertEqual(rows[1]["resource_origin"], "library-resource")
        self.assertEqual(rows[1]["last_painted_envelope_candidate_tiles"], 1)
        self.assertEqual(rows[0]["last_painted_envelope_candidate_tiles"], 0)

    def test_unrelated_rectangle_not_scapegoated(self):
        trace, oracle = fixture()
        trace["paint_trace"][0]["node_bounds_emu"] = [80 * 12_700, 0, 10 * 12_700, 10 * 12_700]
        rows = join_paint_trace(trace, oracle)["pages"][0]["candidate_paint_events"]
        self.assertEqual([r["resource_origin"] for r in rows], ["team-resource"])

    def test_rejects_rotated_mismatched_or_overlapping_receipts(self):
        tr, ref = fixture()
        bad = copy.deepcopy(ref)
        bad["pages"][0]["candidate_boxes"]["rotation"] = 90
        with self.assertRaisesRegex(TraceJoinError, "rotated"):
            join_paint_trace(tr, bad)
        bad = copy.deepcopy(ref)
        bad["pages"][0]["page_boxes_match"] = False
        with self.assertRaisesRegex(TraceJoinError, "page boxes"):
            join_paint_trace(tr, bad)
        bad = copy.deepcopy(ref)
        bad["pages"][0]["diff"]["significant_mask"]["runs"] = [[0, 2], [1, 1]]
        with self.assertRaisesRegex(TraceJoinError, "overlapping"):
            join_paint_trace(tr, bad)
        bad = copy.deepcopy(tr)
        bad["paint_trace"][0]["output_sheet_copies"] = 9
        with self.assertRaisesRegex(TraceJoinError, "nine-up"):
            join_paint_trace(bad, ref)
        bad = copy.deepcopy(tr)
        bad["paint_trace"][1]["page_paint_index"] = 0
        with self.assertRaisesRegex(TraceJoinError, "contiguous"):
            join_paint_trace(bad, ref)
        bad = copy.deepcopy(tr)
        bad["paint_trace"][1]["page_origin"] = "unknown-page"
        with self.assertRaisesRegex(TraceJoinError, "unknown page"):
            join_paint_trace(bad, ref)
        bad = copy.deepcopy(tr)
        bad["candidate_pdf_sha256"] = "b" * 64
        with self.assertRaisesRegex(TraceJoinError, "different PDF bytes"):
            join_paint_trace(bad, ref)


if __name__ == "__main__":
    unittest.main()
