#!/usr/bin/env python3
"""No-source bounded synthetic tests for PDF page-order A/B screening."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zlib
import base64

import fitz
from PIL import Image
sys.path.insert(0, str(Path(__file__).parent))
from pub_pdf_page_order_ab import (ORDER_SCHEMA, candidate_grid, compare_virtual_reorder,
                                   summarize, validate_order_census, viewer_order_indices)


def rows():
    pairs = []
    for i in range(79):
        digest = f"{i+1:064x}"
        pairs.append({"source_sha256": digest, "route": "mature_0x2c", "page_count": 1,
                      "canonical_pdf_order_as_viewer_ordinals": [1],
                      "order_changed": False, "changed_position_count": 0})
    return {"schema": ORDER_SCHEMA, "hosted_pair_count": 79,
            "order_changed_pair_count": 0, "pairs": pairs}


def reference_for(grid: bytes, ordinal: int) -> dict:
    return {"page": ordinal, "grid_sha256": hashlib.sha256(grid).hexdigest(),
            "rgb_zlib_base64": base64.b64encode(zlib.compress(grid)).decode(),
            "media_width_pt": 612.0, "media_height_pt": 792.0}


class PageOrderContract(unittest.TestCase):
    def test_registry_rejects_duplication_bad_permutation_inconsistent_claim(self):
        census = rows()
        self.assertEqual(len(validate_order_census(census)), 79)
        census["pairs"][0]["source_sha256"] = census["pairs"][1]["source_sha256"]
        with self.assertRaisesRegex(ValueError, "duplicate source"):
            validate_order_census(census)
        census = rows()
        census["pairs"][0].update(page_count=2, canonical_pdf_order_as_viewer_ordinals=[1, 1])
        with self.assertRaisesRegex(ValueError, "non-bijective"):
            validate_order_census(census)
        census = rows()
        census["pairs"][0].update(page_count=2, canonical_pdf_order_as_viewer_ordinals=[2, 1],
                                  order_changed=True, changed_position_count=1)
        with self.assertRaisesRegex(ValueError, "inconsistent"):
            validate_order_census(census)

    def test_canonical_to_viewer_inverse_on_27_page_example(self):
        mapping = [7, 16, 10, 18, 15, 2, 6, 23, 20, 27, 26, 5, 9, 14, 3, 22, 17, 1, 8, 21, 11, 4, 24, 13, 19, 12, 25]
        out = viewer_order_indices(mapping)
        self.assertEqual(out[24], 26)  # viewer page 25 comes from current canonical PDF page 27
        self.assertEqual(out[26], 9)   # viewer page 27 comes from current canonical PDF page 10

    def test_existing_page_bytes_are_compared_under_exact_bijection(self):
        with tempfile.TemporaryDirectory() as t:
            pdf = Path(t) / "synthetic.pdf"
            doc = fitz.open()
            for rgb in ((1, 0, 0), (0, 0, 1)):
                page = doc.new_page(width=612, height=792)
                page.draw_rect(page.rect, fill=rgb, color=rgb)
            doc.save(pdf)
            doc.close()
            with fitz.open(pdf) as doc:
                raster0, raster1 = candidate_grid(doc[0]), candidate_grid(doc[1])
            pair = {"reference_surface_stage": "unknown", "reference_pages": 2,
                    "pages": [reference_for(raster1, 1), reference_for(raster0, 2)]}
            refs = {1: raster1, 2: raster0}
            verified = compare_virtual_reorder(pdf, pair, [2, 1], reference_loader=lambda row: refs[row["page"]])
            self.assertEqual(verified["status"], "raster_screened_reference_stage_unknown")
            self.assertEqual([x["changed_cell_count"] for x in verified["after"]], [0, 0])
            self.assertTrue(all(x["changed_cell_count"] > 0 for x in verified["before"]))
            same = compare_virtual_reorder(pdf, pair, [1, 2], reference_loader=lambda row: refs[row["page"]])
            self.assertEqual(same["before"], same["after"])
            too_short = compare_virtual_reorder(pdf, pair, [1], reference_loader=lambda row: refs[row["page"]])
            self.assertEqual(too_short["status"], "page_count_or_order_mismatch")
            blocked = compare_virtual_reorder(pdf, dict(pair, reference_surface_stage="production_sheet"), [2, 1], reference_loader=lambda row: refs[row["page"]])
            self.assertEqual(blocked["status"], "surface_mapping_required")

    def test_geometry_drift_is_inconclusive_not_success(self):
        with tempfile.TemporaryDirectory() as t:
            pdf = Path(t) / "synthetic.pdf"
            doc = fitz.open()
            doc.new_page(width=612, height=792)
            doc.new_page(width=400, height=600)
            doc.save(pdf)
            doc.close()
            with fitz.open(pdf) as doc:
                ref = candidate_grid(doc[0])
            pair = {"reference_surface_stage": "unknown", "reference_pages": 2,
                    "pages": [reference_for(ref, 1), reference_for(ref, 2)]}
            result = compare_virtual_reorder(pdf, pair, [2, 1], reference_loader=lambda row: ref)
            self.assertEqual(result["status"], "inconclusive_media_extent_drift")

    def test_one_regression_outweighs_stronger_improvement(self):
        rows = [{"fixture": "synthetic", "pub_sha256": "a" * 64, "status": "raster_screened_reference_stage_unknown",
                 "before": [{"status": "compared", "candidate_pdf_ordinal": 1, "changed_cell_count": 4000},
                            {"status": "compared", "candidate_pdf_ordinal": 2, "changed_cell_count": 10}],
                 "after": [{"status": "compared", "candidate_pdf_ordinal": 2, "changed_cell_count": 10},
                           {"status": "compared", "candidate_pdf_ordinal": 1, "changed_cell_count": 11}]}]
        result = summarize(rows)
        self.assertEqual(result["verdict"], "regressions_under_screening")
        self.assertEqual((result["improved_pages"], result["regressed_pages"]), (1, 1))
        self.assertGreater(result["net_improved_cells"], 0)

    def test_source_safe_output_has_no_page_bytes_or_raw_ids(self):
        rows = [{"fixture": "synthetic", "pub_sha256": "a" * 64, "status": "raster_screened_reference_stage_unknown",
                 "before": [{"status": "compared", "candidate_pdf_ordinal": 1, "changed_cell_count": 42}],
                 "after": [{"status": "compared", "candidate_pdf_ordinal": 1, "changed_cell_count": 42}]}]
        text = json.dumps(summarize(rows))
        self.assertNotIn("rgb_zlib_base64", text)
        self.assertNotIn("source_path", text)
        self.assertEqual(json.loads(text)["verdict"], "no_screened_change")


if __name__ == "__main__":
    unittest.main(verbosity=2)
