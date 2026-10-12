#!/usr/bin/env python3
"""Source-free Publisher oracle tests: PDF-only archive and false parity rejection."""
import tempfile
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


class PublisherNativeGateTests(unittest.TestCase):
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
