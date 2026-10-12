#!/usr/bin/env python3
"""Source-free Publisher oracle tests: PDF-only archive and false parity rejection."""
import hashlib
import tempfile
import uuid
import unittest
import zipfile
from pathlib import Path

import fitz

from reader_logo_publisher_native_verdict_v1 import (
    NATIVE_ORACLE_BUNDLE_SHA256,
    REFERENCE_PAGES,
    BASE_COMMIT_SHA,
    PRODUCT_COMMIT_SHA,
    disposition,
    derive_pub_source_id,
    verify_page381_customer_page4_join,
    evaluate,
    inspect_pdf_only_bundle,
    require_exact_publisher_bundle,
)


def synthetic_pdf(page_count):
    with fitz.open() as pdf:
        for _ in range(page_count):
            pdf.new_page(width=612, height=792)
        return pdf.tobytes()


def make_pdf_only_zip(path, *, drop=None, extra=None, corrupt_count=None, folder=""):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, count in REFERENCE_PAGES.items():
            if name == drop:
                continue
            if name == corrupt_count:
                count += 1
            archive.writestr(f"{folder}{name}.pdf", synthetic_pdf(count))
        if extra is not None:
            archive.writestr(extra, b"bad member")



def synthetic_capture(page_ids):
    return {"fixture": {"page_geometry": [
        {"page_id": value, "order": index,
         "page_identity_sha256": hashlib.sha256(value.encode("utf-8")).hexdigest()}
        for index, value in enumerate(page_ids)
    ]}}


class PublisherNativeGateTests(unittest.TestCase):
    def test_source_derived_page381_uuid_matches_cdm_source_id_v1(self):
        source = "".join(f"{value:02x}" for value in range(32))
        self.assertEqual(
            derive_pub_source_id(source, "contents/0x2c/seq/330", "cdm.story"),
            "3c3cfd8a-2347-5cf4-977d-34e4f346f6df",
        )
        self.assertEqual(
            derive_pub_source_id(
                "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
                "contents/0x2c/seq/381", "cdm.page",
            ),
            "782eec0b-01ee-5d9b-bad4-1ef749beb9b1",
        )

    def test_source_page381_must_be_verified_on_both_browser_page4_receipts(self):
        original = [
            str(uuid.UUID(int=1)), str(uuid.UUID(int=2)),
            str(uuid.UUID(int=3)), "782eec0b-01ee-5d9b-bad4-1ef749beb9b1",
        ]
        before, after = synthetic_capture(original), synthetic_capture(original)
        self.assertEqual(
            verify_page381_customer_page4_join(before, after), original[3]
        )
        switched = original.copy()
        switched[2], switched[3] = switched[3], switched[2]
        with self.assertRaisesRegex(ValueError, "not uniquely customer page 4"):
            verify_page381_customer_page4_join(before, synthetic_capture(switched))
        duplicates = original.copy()
        duplicates[1] = duplicates[3]
        with self.assertRaisesRegex(ValueError, "not uniquely customer page 4"):
            verify_page381_customer_page4_join(before, synthetic_capture(duplicates))
        forged = synthetic_capture(original)
        forged["fixture"]["page_geometry"][3]["page_identity_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "digest does not match"):
            verify_page381_customer_page4_join(before, forged)

    def test_synthetic_pdf_only_bundle_has_ten_pdf_twenty_pages(self):
        with tempfile.TemporaryDirectory() as directory:
            zip_path = Path(directory) / "synthetic.zip"
            make_pdf_only_zip(zip_path, folder="pdf-oracle/")
            census = inspect_pdf_only_bundle(zip_path)
            self.assertEqual(census["pdf_count"], 10)
            self.assertEqual(census["reference_pages"], 20)
            self.assertEqual(
                census["files"]["SampleNewsletter"]["pdf_pages"], 4
            )
            with fitz.open(stream=census["newsletter_pdf_bytes"], filetype="pdf") as pdf:
                self.assertEqual(pdf.page_count, 4)

    def test_fake_zip_may_pass_structural_census_but_never_native_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            zip_path = Path(directory) / "synthetic.zip"
            make_pdf_only_zip(zip_path)
            self.assertNotEqual(NATIVE_ORACLE_BUNDLE_SHA256, "0" * 64)
            self.assertEqual(inspect_pdf_only_bundle(zip_path)["pdf_count"], 10)
            with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
                require_exact_publisher_bundle(zip_path)
            with self.assertRaisesRegex(ValueError, "existing ZIP"):
                require_exact_publisher_bundle(Path(directory) / "missing.zip")

    def test_pub_or_manifest_cannot_be_mistaken_for_publisher_pdf_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            for extra in ("SampleNewsletter.pub", "PAIRS.csv", "readme.txt"):
                path = Path(directory) / "extra.zip"
                make_pdf_only_zip(path, extra=extra)
                with self.assertRaisesRegex(ValueError, "ten PDFs"):
                    inspect_pdf_only_bundle(path)

    def test_missing_duplicate_or_wrong_reference_page_count_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "oracle.zip"
            make_pdf_only_zip(path, drop="SampleNewsletter")
            with self.assertRaisesRegex(ValueError, "ten PDFs"):
                inspect_pdf_only_bundle(path)
            make_pdf_only_zip(path, corrupt_count="SampleNewsletter")
            with self.assertRaisesRegex(ValueError, "page count drift"):
                inspect_pdf_only_bundle(path)
            make_pdf_only_zip(path, extra="unexpected.pdf", drop="51318")
            with self.assertRaisesRegex(ValueError, "unexpected non-PDF"):
                inspect_pdf_only_bundle(path)

    def test_zip_path_traversal_and_duplicate_basename_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "unsafe.zip"
            with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
                for name, count in REFERENCE_PAGES.items():
                    destination = "../SampleNewsletter.pdf" if name == "SampleNewsletter" else name + ".pdf"
                    archive.writestr(destination, synthetic_pdf(count))
            with self.assertRaisesRegex(ValueError, "unsafe"):
                inspect_pdf_only_bundle(path)
            # A ten-member archive with duplicate newsletter and one missing
            # other fixture must reject duplicate identity.
            with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
                for name, count in REFERENCE_PAGES.items():
                    destination = "SampleNewsletter.pdf" if name == "60685" else name + ".pdf"
                    archive.writestr(destination, synthetic_pdf(count))
            with self.assertRaisesRegex(ValueError, "duplicate"):
                inspect_pdf_only_bundle(path)

    def test_commit_and_delta_controls_run_before_external_bundle_access(self):
        with self.assertRaisesRegex(ValueError, "distinct commits"):
            evaluate(
                Path("missing-before.json"), Path("missing-after.json"), Path("missing.zip"),
                baseline_commit=BASE_COMMIT_SHA, candidate_commit=BASE_COMMIT_SHA,
            )
        with self.assertRaisesRegex(ValueError, "bounded positive"):
            evaluate(
                Path("missing-before.json"), Path("missing-after.json"), Path("missing.zip"),
                baseline_commit=BASE_COMMIT_SHA, candidate_commit=PRODUCT_COMMIT_SHA,
                expected_added_nodes=0,
            )
        with self.assertRaisesRegex(ValueError, "40-hex"):
            evaluate(
                Path("missing-before.json"), Path("missing-after.json"), Path("missing.zip"),
                baseline_commit="main",
            )

    def test_native_disposition_not_labeled_lo(self):
        self.assertEqual(disposition(200, 100, 13), "closer_to_native_publisher")
        self.assertEqual(disposition(100, 200, 13), "farther_from_native_publisher")
        self.assertEqual(disposition(100, 100, 13), "equal_native_distance")
        self.assertEqual(disposition(100, 100, 0), "no_visible_effect")


if __name__ == "__main__":
    unittest.main()
