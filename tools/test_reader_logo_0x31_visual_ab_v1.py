#!/usr/bin/env python3
"""Fail-closed controls for the exact SourceGraph-to-raster LO proxy report."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from reader_logo_0x31_visual_ab_v1 import (
    BASE_SHA,
    SOURCE_SHA256,
    checked_capture,
    classify_visual_delta,
)


class LogoVisualABTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        geometry = []
        shots = []
        for i in range(4):
            name = f"SampleNewsletter-page-{i + 1}.png"
            data = b"public-fixture-synthetic-png" + bytes([i])
            (self.root / name).write_bytes(data)
            shots.append(
                {
                    "page": i + 1,
                    "filename": name,
                    "sha256": hashlib.sha256(data).hexdigest(),
                }
            )
            geometry.append(
                {
                    "order": i,
                    "page_identity_sha256": hashlib.sha256(
                        f"fake-page-{i}".encode()
                    ).hexdigest(),
                    "width_emu": 6000000,
                    "height_emu": 9000000,
                }
            )
        self.receipt = {
            "protocol": "chaptera.cloud-reader-real-scene-browser.v1",
            "repository_commit_sha": BASE_SHA,
            "results": [{
                "fixture": "SampleNewsletter",
                "source_sha256": SOURCE_SHA256,
                "source_byte_len": 291840,
                "rendered": True,
                "pages": 4,
                "browser_preserved_scene_node_order": True,
                "reference_raster_dpi": 144,
                "page_geometry": geometry,
                "screenshots": shots,
            }],
        }
        self.path = self.root / "receipt.json"

    def capture(self):
        self.path.write_text(json.dumps(self.receipt), encoding="utf-8")
        return checked_capture(self.path, BASE_SHA)

    def test_exact_fixture_four_distinct_pages_and_images(self):
        result = self.capture()
        self.assertEqual(len(result["page_ids"]), 4)

    def test_rejects_wrong_commit(self):
        self.receipt["repository_commit_sha"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "commit identity"):
            self.capture()

    def test_rejects_wrong_source_hash(self):
        self.receipt["results"][0]["source_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.capture()

    def test_rejects_duplicate_page_identity(self):
        geom = self.receipt["results"][0]["page_geometry"]
        geom[3]["page_identity_sha256"] = geom[2]["page_identity_sha256"]
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.capture()

    def test_rejects_missing_image(self):
        (self.root / "SampleNewsletter-page-4.png").unlink()
        with self.assertRaisesRegex(ValueError, "mutated"):
            self.capture()

    def test_rejects_wrong_customer_page_order(self):
        self.receipt["results"][0]["page_geometry"][3]["order"] = 4
        with self.assertRaisesRegex(ValueError, "contiguous"):
            self.capture()

    def test_proxy_classification_never_invents_parity(self):
        self.assertEqual(classify_visual_delta(0.20, 0.10, 0.03), "closer_to_libreoffice")
        self.assertEqual(classify_visual_delta(0.10, 0.20, 0.03), "farther_from_libreoffice")
        self.assertEqual(classify_visual_delta(0.10, 0.10, 0.03), "visible_change_equal_reference_distance")
        self.assertEqual(classify_visual_delta(0.10, 0.10, 0), "no_visible_pixel_change")
        self.assertEqual(classify_visual_delta(None, 0.10, 0.05), "not_comparable")


if __name__ == "__main__":
    unittest.main()
