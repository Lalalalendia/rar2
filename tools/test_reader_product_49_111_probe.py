#!/usr/bin/env python3
"""Source-free boundaries for physical Product Chromium Publisher page receipts."""
from __future__ import annotations

from copy import deepcopy
import unittest

from reader_product_49_111_probe import validate_browser_receipt

HEAD = "a" * 40
SOURCE = "b" * 64


def good():
    pages = [{
        "page": i,
        "product_page_identity_sha256": "c" * 64,
        "chromium_png_sha256": "d" * 64,
        "png_dimensions": {"width": 800, "height": 600},
        "width_emu": 800 * 6350,
        "height_emu": 600 * 6350,
        "host_derived_resource_count": 1,
    } for i in (1, 2)]
    return {
        "schema": "chaptera.product-host-visual-chromium.v1",
        "repository_commit_sha": HEAD,
        "source_sha256": SOURCE,
        "revision_identity_sha256": "e" * 64,
        "fidelity_state": "partial",
        "authenticated_product_scene_provided_by_owner_runner": True,
        "host_image_count": 1,
        "external_request_count": 0,
        "page_errors": 0,
        "reference_raster_dpi": 144,
        "pages": pages,
    }


class ProductHostVisualTests(unittest.TestCase):
    def setUp(self):
        self.p = good()

    def validate(self):
        return validate_browser_receipt(self.p, "test", SOURCE, HEAD, 2)

    def bad(self, message: str):
        with self.assertRaisesRegex(ValueError, message):
            self.validate()

    def test_valid_bounded_two_page_product_receipt(self):
        self.assertEqual(len(self.validate()), 2)

    def test_reject_guest_scene_claim(self):
        self.p["authenticated_product_scene_provided_by_owner_runner"] = False
        self.bad("browser identity/route")

    def test_reject_missing_chromium_page(self):
        self.p["pages"].pop()
        self.bad("PAGE count")

    def test_reject_duplicate_page(self):
        self.p["pages"][1]["page"] = 1
        self.bad("ordinal")

    def test_reject_source_mismatch(self):
        self.p["source_sha256"] = "f" * 64
        self.bad("browser identity/route")

    def test_reject_stale_repo_head(self):
        self.p["repository_commit_sha"] = "f" * 40
        self.bad("browser identity/route")

    def test_reject_external_browser_request(self):
        self.p["external_request_count"] = 1
        self.bad("browser identity/route")

    def test_reject_product_chromium_exception(self):
        self.p["page_errors"] = 1
        self.bad("browser identity/route")

    def test_reject_unproven_page_id(self):
        self.p["pages"][0]["product_page_identity_sha256"] = "fake"
        self.bad("screenshot proof")

    def test_reject_wrong_page_extent(self):
        self.p["pages"][0]["png_dimensions"]["width"] = 801
        self.bad("DPI/extent")

    def test_reject_wrong_fidelity_disposition(self):
        self.p["fidelity_state"] = "undisclosed"
        self.bad("fidelity")

    def test_reject_unbounded_image_resources(self):
        self.p["host_image_count"] = 97
        self.bad("budget")

    def test_reject_no_revision_identity(self):
        self.p["revision_identity_sha256"] = None
        self.bad("revision")


if __name__ == "__main__":
    unittest.main()
