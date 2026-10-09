#!/usr/bin/env python3
"""Contract tests for source-bound Publisher oracle; no private PUB/PDF assets."""
from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import unittest

import fitz

sys.path.insert(0, str(Path(__file__).parent))
from pub_pdf_cli_publisher_oracle_v1 import classify_cli_failure, compare_pdf, load_references, run, safe_loss_summary

ROOT = Path(__file__).resolve().parents[1]
BATCH = ROOT / "tools/corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json"
SUPPLEMENTAL = ROOT / "tools/corpus/receipts/publisher-visual-golden-supplemental-reference-available-2026-10-06-fingerprint-v1.json"


class OracleContractTests(unittest.TestCase):
    def test_registered_corpus_exact_identity_and_counts(self):
        pairs = load_references(BATCH, SUPPLEMENTAL)
        self.assertEqual(len(pairs), 86)
        self.assertEqual(sum(p["reference_pages"] for p in pairs), 221)
        self.assertEqual(sum(p.get("family") == "manual-reduction-family" for p in pairs), 7)

    def test_sha_identity_drift_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            payload = json.loads(BATCH.read_text())
            payload["pairs"][0]["pages"][0]["grid_sha256"] = "0" * 64
            corrupt = Path(temp) / "corrupt.json"
            corrupt.write_text(json.dumps(payload))
            with self.assertRaises(ValueError):
                load_references(corrupt, SUPPLEMENTAL)

    def test_page_count_and_surface_stage_are_not_silent_matches(self):
        pairs = load_references(BATCH, SUPPLEMENTAL)
        pair = next(p for p in pairs if p["reference_pages"] == 1)
        with tempfile.TemporaryDirectory() as temp:
            filename = Path(temp) / "test.pdf"
            doc = fitz.open()
            doc.new_page()
            doc.new_page()
            doc.save(filename)
            doc.close()
            outcome = compare_pdf(filename, pair)
            self.assertEqual(outcome["status"], "page_count_mismatch")
            self.assertEqual(outcome["candidate_pages"], 2)
            self.assertEqual(outcome["pages"], [])
            doc = fitz.open()
            doc.new_page(width=pair["pages"][0]["media_width_pt"], height=pair["pages"][0]["media_height_pt"])
            other = Path(temp) / "one.pdf"
            doc.save(other)
            doc.close()
            surface_pair = dict(pair, reference_surface_stage="production_sheet")
            self.assertEqual(compare_pdf(other, surface_pair)["status"], "surface_mapping_required")
            unknown_pair = dict(pair, reference_surface_stage="unknown")
            self.assertEqual(compare_pdf(other, unknown_pair)["status"], "raster_compared_stage_unknown")

    def test_cli_failure_classification_is_bounded_and_source_safe(self):
        cases = {
            "Error: bounded PDF conversion currently requires mature 0x2C PUB input": "unsupported_pub_route",
            "Error: fallback font cannot be embedded under fixed PDF policy: restricted": "fallback_font_embedding_blocked",
            "Caused by: open mature-0x2C PUB for bounded PDF conversion": "pub_open_failed",
            "Caused by: resolve bounded shaped text flow for fixed PDF": "layout_projection_failed",
            "Caused by: render bounded deterministic PDF": "pdf_render_failed",
            "Error: read PUB source [redacted-path]": "cli_failed_other",
        }
        for stderr, expected in cases.items():
            with self.subTest(expected=expected):
                status = classify_cli_failure(stderr)
                self.assertEqual(status, expected)
                self.assertNotIn("/", status)
                self.assertNotIn("secret", status)
                self.assertNotIn(".pub", status)

    def test_unsupported_image_mime_histogram_is_bounded(self):
        receipt = {
            "typography": {},
            "pdf": {
                "nodes": [],
                "diagnostics": [
                    {
                        "code": "pdf.image.mime_unsupported",
                        "message": 'exact image MIME "image/svg+xml" is outside the bounded PNG/JPEG PDF slice',
                    },
                    {
                        "code": "pdf.image.mime_unsupported",
                        "message": 'exact image MIME "../../secret" is outside the bounded PNG/JPEG PDF slice',
                    },
                    {
                        "code": "other",
                        "message": 'exact image MIME "image/tiff" is outside the bounded PNG/JPEG PDF slice',
                    },
                ],
            },
        }
        summary = safe_loss_summary(receipt)
        self.assertEqual(
            summary["unsupported_image_mime_counts"],
            {"image/svg+xml": 1, "unclassified": 1},
        )
        rendered = json.dumps(summary)
        self.assertNotIn("secret", rendered)
        self.assertNotIn("image/tiff", rendered)

    def test_absent_private_inputs_and_hosted_source_are_not_claimed_as_compared(self):
        pairs = load_references(BATCH, SUPPLEMENTAL)
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            cli, font = tmp / "pub", tmp / "font.ttf"
            cli.write_text("#!/bin/sh\nexit 1\n")
            font.write_bytes(b"fixture")
            receipt = run(pairs, tmp, cli, font, 1)
            self.assertEqual(receipt["registered_pair_count"], 86)
            self.assertEqual(receipt["registered_publisher_page_count"], 221)
            self.assertEqual(receipt["status_counts"]["private_source_not_hosted"], 7)
            self.assertEqual(receipt["status_counts"]["source_missing_or_ambiguous"], 79)
            self.assertEqual(receipt["compared_page_count"], 0)
            self.assertEqual(receipt["candidate_pdf_count"], 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
