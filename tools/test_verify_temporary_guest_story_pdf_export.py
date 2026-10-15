"""Synthetic regression tests for the physical authored Story PDF consumer."""
from __future__ import annotations

import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

import fitz

PATH = Path(__file__).with_name("verify_temporary_guest_story_pdf_export.py")
spec = importlib.util.spec_from_file_location("authored_pdf", PATH)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class VerifyAuthoredTextPdf(unittest.TestCase):
    def _pdf(self, path: Path, *, draw_text="", metadata_text="",
             duplicate=False, render_mode=0):
        pdf = fitz.open()
        page = pdf.new_page()
        if draw_text:
            page.insert_text((80, 96), draw_text, render_mode=render_mode)
        if duplicate:
            second = pdf.new_page()
            second.insert_text((80, 96), draw_text)
        if metadata_text:
            pdf.set_metadata({"subject": metadata_text})
        pdf.save(path)
        pdf.close()

    def test_passes_actual_visible_text(self):
        with TemporaryDirectory() as folder:
            path = Path(folder) / "ok.pdf"
            self._pdf(path, draw_text="ChapteraNewTextBoxProof42")
            report = module.verify_pdf(path, "ChapteraNewTextBoxProof42")
            self.assertEqual(report["status"], "PASS")
            self.assertEqual(report["marker_page_count"], 1)
            self.assertEqual(len(report["artifact_sha256"]), 64)

    def test_rejects_metadata_only(self):
        with TemporaryDirectory() as folder:
            path = Path(folder) / "metadata.pdf"
            self._pdf(path, metadata_text="ChapteraNewTextBoxProof42")
            with self.assertRaises(ValueError):
                module.verify_pdf(path, "ChapteraNewTextBoxProof42")

    def test_rejects_different_text(self):
        with TemporaryDirectory() as folder:
            path = Path(folder) / "other.pdf"
            self._pdf(path, draw_text="Original source only")
            with self.assertRaises(ValueError):
                module.verify_pdf(path, "ChapteraNewTextBoxProof42")

    def test_rejects_ambiguous_duplicate_on_other_page(self):
        with TemporaryDirectory() as folder:
            path = Path(folder) / "duplicate.pdf"
            self._pdf(path, draw_text="ChapteraNewTextBoxProof42", duplicate=True)
            with self.assertRaises(ValueError):
                module.verify_pdf(path, "ChapteraNewTextBoxProof42")

    def test_rejects_invisible_pdf_text(self):
        with TemporaryDirectory() as folder:
            path = Path(folder) / "hidden.pdf"
            self._pdf(path, draw_text="ChapteraNewTextBoxProof42", render_mode=3)
            with self.assertRaises(ValueError):
                module.verify_pdf(path, "ChapteraNewTextBoxProof42")

    def test_rejects_nonpdf(self):
        with TemporaryDirectory() as folder:
            path = Path(folder) / "fake.pdf"
            path.write_bytes(b"PK\x03\x04" + b"PDF-" + b"x" * 12)
            with self.assertRaises(ValueError):
                module.verify_pdf(path, "ChapteraNewTextBoxProof42")


if __name__ == "__main__":
    unittest.main()
