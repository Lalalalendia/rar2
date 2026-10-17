"""Source-free regression and Tesseract smoke for PDF text-layer trust lab."""

import hashlib
import json
import shutil
import unittest

import fitz

from pdf_ocr_differential_lab_v1 import (
    MISDECODED, SAMPLE, SCHEMA, fixtures, make_vector_pdf, probe,
    raster_digest, run_synthetic,
)


class PdfOcrDifferentialLabTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cases = fixtures()

    def test_vector_pixels_same_but_extracted_text_different(self):
        control = self.cases["native_vector"]
        poisoned = self.cases["wrong_tounicode"]
        self.assertNotEqual(hashlib.sha256(control).digest(), hashlib.sha256(poisoned).digest())
        self.assertEqual(raster_digest(control), raster_digest(poisoned))
        with fitz.open(stream=control, filetype="pdf") as doc:
            self.assertEqual(doc[0].get_text().strip(), SAMPLE)
        with fitz.open(stream=poisoned, filetype="pdf") as doc:
            self.assertEqual(doc[0].get_text().strip(), MISDECODED)

    def test_fixture_generation_is_deterministic(self):
        self.assertEqual(make_vector_pdf(poisoned=True), self.cases["wrong_tounicode"])
        self.assertEqual(fixtures(), self.cases)

    def test_structural_routes_with_stubbed_ocr(self):
        expected = {
            "native_vector": "NATIVE_TEXT_CORROBORATED",
            "wrong_tounicode": "UNICODE_CONFLICT_REVIEW",
            "raster_only": "OCR_REQUIRED",
            "invisible_wrong_layer": "HIDDEN_TEXT_LAYER_REVIEW",
        }
        for name, data in self.cases.items():
            with self.subTest(name=name):
                result = probe(data, ocr=lambda png: SAMPLE)
                self.assertEqual(result.route, expected[name])
                self.assertEqual(result.ocr_chars, len(SAMPLE))
                self.assertEqual(result.native_chars, 0 if name == "raster_only" else len(SAMPLE))
                if name == "invisible_wrong_layer":
                    self.assertGreater(result.hidden_glyphs, 0)
                    self.assertEqual(result.image_count, 1)
                if name == "wrong_tounicode":
                    self.assertEqual(result.hidden_glyphs, 0)
                    self.assertEqual(result.image_count, 0)

    def test_empty_ocr_is_not_trusted(self):
        for data in self.cases.values():
            self.assertEqual(probe(data, ocr=lambda png: "").route, "OCR_EMPTY_REVIEW")

    def test_false_ocr_alarm_does_not_autocorrect_pdf(self):
        # Real OCR can also be mistaken; disagreement means review, not correction.
        first = self.cases["native_vector"]
        before = hashlib.sha256(first).digest()
        verdict = probe(first, ocr=lambda png: MISDECODED)
        self.assertEqual(verdict.route, "UNICODE_CONFLICT_REVIEW")
        self.assertEqual(hashlib.sha256(first).digest(), before)

    def test_reject_oversized_and_wrong_page_count(self):
        with self.assertRaisesRegex(ValueError, "pdf_size_limit_exceeded"):
            probe(b"x" * (4 * 1024 * 1024 + 1), ocr=lambda p: SAMPLE)
        doc = fitz.open()
        doc.new_page()
        doc.new_page()
        data = doc.tobytes()
        doc.close()
        with self.assertRaisesRegex(ValueError, "encrypted_or_not_one_page"):
            probe(data, ocr=lambda p: SAMPLE)

    def test_reject_encrypted(self):
        doc = fitz.open()
        doc.new_page()
        data = doc.tobytes(encryption=fitz.PDF_ENCRYPT_AES_256, owner_pw="owner", user_pw="user")
        doc.close()
        with self.assertRaisesRegex(ValueError, "encrypted_or_not_one_page"):
            probe(data, ocr=lambda p: SAMPLE)

    @unittest.skipUnless(shutil.which("tesseract"), "local Tesseract binary unavailable")
    def test_real_tesseract_four_synthetic_cases(self):
        receipt = run_synthetic()
        self.assertEqual(receipt["schema"], SCHEMA)
        self.assertTrue(receipt["identical_pixels_vector_pair"])
        expected = {
            "native_vector": "NATIVE_TEXT_CORROBORATED",
            "wrong_tounicode": "UNICODE_CONFLICT_REVIEW",
            "raster_only": "OCR_REQUIRED",
            "invisible_wrong_layer": "HIDDEN_TEXT_LAYER_REVIEW",
        }
        for name, case in receipt["cases"].items():
            with self.subTest(name=name):
                self.assertEqual(case["route"], expected[name])
                self.assertEqual(case["ocr_text_sha256"], hashlib.sha256(SAMPLE.encode()).hexdigest())
        # CI artifacts contain only synthetic hashes/counters, not raw input text.
        encoded = json.dumps(receipt)
        self.assertNotIn(SAMPLE, encoded)
        self.assertNotIn(MISDECODED, encoded)


if __name__ == "__main__":
    unittest.main()
