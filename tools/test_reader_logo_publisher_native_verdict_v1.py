#!/usr/bin/env python3
"""Fail-closed, source-safe controls; no Publisher corpus is included here."""
import tempfile
import unittest
from pathlib import Path

from reader_logo_publisher_native_verdict_v1 import (
    NATIVE_ORACLE_BUNDLE_SHA256, SOURCE_SHA256, disposition,
    require_exact_publisher_bundle, select_newsletter,
)


def synthetic_pairs():
    rows = [
        {
            "basename": "SampleNewsletter",
            "oracle_id": "synthetic-newsletter",
            "pub_sha256": SOURCE_SHA256,
            "pub_bytes": 291840,
            "pdf_pages": 4,
        },
    ]
    rows.extend(
        {
            "basename": f"synthetic-{n}",
            "oracle_id": f"synthetic-{n}",
            "pub_sha256": "0" * 64,
            "pub_bytes": 100,
            "pdf_pages": 2 if n < 7 else 1,
        }
        for n in range(9)
    )
    return rows


class PublisherNativeGateTests(unittest.TestCase):
    def test_only_exact_zip_identity_admitted(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "oracle.zip"
            path.write_bytes(b"synthetic-not-real-oracle")
            require_exact_publisher_bundle(path, NATIVE_ORACLE_BUNDLE_SHA256)
            with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
                require_exact_publisher_bundle(path, "0" * 64)
            path.rename(Path(tmp) / "oracle.pdf")
            with self.assertRaisesRegex(ValueError, "pinned external ZIP"):
                require_exact_publisher_bundle(Path(tmp) / "oracle.pdf", NATIVE_ORACLE_BUNDLE_SHA256)

    def test_newsletter_exact_pair_and_page_count(self):
        selected = select_newsletter(synthetic_pairs())
        self.assertEqual(selected["pub_sha256"], SOURCE_SHA256)
        self.assertEqual(selected["pdf_pages"], 4)

    def test_wrong_source_not_misidentified_as_publisher_gold(self):
        rows = synthetic_pairs()
        rows[0]["pub_sha256"] = "a" * 64
        with self.assertRaisesRegex(ValueError, "PUB source/page identity"):
            select_newsletter(rows)

    def test_missing_or_duplicate_identity_rejected(self):
        rows = synthetic_pairs()
        rows[0]["basename"] = "Other"
        with self.assertRaisesRegex(ValueError, "exactly one SampleNewsletter"):
            select_newsletter(rows)
        rows = synthetic_pairs()
        rows[-1]["basename"] = "SampleNewsletter"
        with self.assertRaisesRegex(ValueError, "exactly one SampleNewsletter"):
            select_newsletter(rows)

    def test_no_incomplete_reference_bundle(self):
        with self.assertRaisesRegex(ValueError, "exactly 10 pairs"):
            select_newsletter(synthetic_pairs()[:9])
        rows = synthetic_pairs()
        rows[-1]["pdf_pages"] = 2
        with self.assertRaisesRegex(ValueError, "20 reference pages"):
            select_newsletter(rows)

    def test_disposition_measures_native_not_lo(self):
        self.assertEqual(disposition(200, 100, 13), "closer_to_native_publisher")
        self.assertEqual(disposition(100, 200, 13), "farther_from_native_publisher")
        self.assertEqual(disposition(100, 100, 13), "equal_native_distance")
        self.assertEqual(disposition(100, 100, 0), "no_visible_effect")


if __name__ == "__main__":
    unittest.main()
