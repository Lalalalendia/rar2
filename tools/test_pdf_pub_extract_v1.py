"""Synthetic, source-free tests for the bounded PDF paint probe."""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest

import fitz
from pdf_pub_extract_v1 import ProbeRejected, main, probe


def make_pdf(builder=None) -> bytes:
    doc = fitz.open()
    page = doc.new_page(width=200, height=300)
    if builder is not None:
        builder(page)
    data = doc.tobytes(deflate=True, garbage=3)
    doc.close()
    return data


def basic_page(page):
    page.insert_text((20, 45), "Hello", fontsize=12)
    page.draw_rect(fitz.Rect(40, 80, 90, 100), color=None, fill=(1, 0, 0))


class PdfPubExtractTest(unittest.TestCase):
    def test_hello_and_opaque_rectangle(self):
        source = make_pdf(basic_page)
        packet = probe(source)
        self.assertEqual(packet, probe(source))
        self.assertEqual(packet["source_sha256"], hashlib.sha256(source).hexdigest())
        self.assertEqual(packet["status"], "candidate")
        self.assertFalse(packet["native_pub_written"])
        self.assertEqual([o["kind"] for o in packet["pages"][0]["objects"]], ["text_line", "filled_rectangle"])
        self.assertEqual(packet["pages"][0]["width_emu"], 200 * 12700)
        self.assertEqual(packet["pages"][0]["height_emu"], 300 * 12700)
        span, rect = packet["pages"][0]["objects"]
        self.assertEqual(span["text"], "Hello")
        self.assertEqual(span["baseline_emu"], [20 * 12700, 45 * 12700])
        self.assertEqual(span["font_resource"], "unresolved")
        self.assertEqual(rect["bbox_emu"], [40 * 12700, 80 * 12700, 90 * 12700, 100 * 12700])
        self.assertEqual(rect["fill_rgb"], [255, 0, 0])
        self.assertEqual([o["paint_order"] for o in packet["pages"][0]["objects"]], [0, 1])

    def test_unhandled_line_is_partial(self):
        packet = probe(make_pdf(lambda p: p.draw_line((5, 15), (95, 15), color=(0, 0, 0))))
        self.assertEqual(packet["status"], "partial")
        self.assertIn("unsupported_vector_paint", packet["pages"][0]["diagnostics"])

    def test_rotated_page_is_partial(self):
        packet = probe(make_pdf(lambda p: p.set_rotation(90)))
        self.assertIn("nontrivial_page_transform", packet["pages"][0]["diagnostics"])

    def test_cropped_page_is_partial(self):
        packet = probe(make_pdf(lambda p: p.set_cropbox(fitz.Rect(10, 10, 190, 290))))
        self.assertIn("nontrivial_page_transform", packet["pages"][0]["diagnostics"])

    def test_opaque_text_only(self):
        packet = probe(make_pdf(lambda p: p.insert_text((20, 45), "Sample", fontsize=12)))
        self.assertEqual(packet["status"], "candidate")
        self.assertEqual(packet["pages"][0]["objects"][0]["text"], "Sample")

    def test_translucent_text_is_partial(self):
        packet = probe(make_pdf(lambda p: p.insert_text((20, 45), "Faint", fill_opacity=0.35)))
        self.assertIn("complex_text_paint", packet["pages"][0]["diagnostics"])

    def test_invisible_text_is_partial(self):
        packet = probe(make_pdf(lambda p: p.insert_text((20, 45), "Hidden", render_mode=3)))
        self.assertIn("unsupported_text_paint", packet["pages"][0]["diagnostics"])

    def test_image_is_partial_even_if_inline(self):
        def image(page):
            pm = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, 2, 2), False)
            pm.clear_with(140)
            page.insert_image(fitz.Rect(20, 30, 60, 70), pixmap=pm)
        packet = probe(make_pdf(image))
        self.assertIn("image_requires_separate_import", packet["pages"][0]["diagnostics"])

    def test_translucent_rectangle_is_partial(self):
        def translucent(page):
            page.draw_rect(fitz.Rect(20, 50, 80, 70), fill=(1, 0, 0), color=None, fill_opacity=0.5)
        packet = probe(make_pdf(translucent))
        self.assertIn("unsupported_vector_opacity", packet["pages"][0]["diagnostics"])

    def test_overlapping_objects_are_flagged(self):
        def overlap(page):
            page.draw_rect(fitz.Rect(0, 0, 200, 300), fill=(1, 1, 1), color=None)
            page.insert_text((20, 45), "Hello", fontsize=12)
        packet = probe(make_pdf(overlap))
        self.assertIn("overlapping_objects_require_review", packet["pages"][0]["diagnostics"])

    def test_rejects_invalid_header_without_leaking_source(self):
        with self.assertRaisesRegex(ProbeRejected, "not_pdf"):
            probe(b"not a pdf")

    def test_typed_encrypted_rejection(self):
        doc = fitz.open()
        doc.new_page()
        source = doc.tobytes(encryption=fitz.PDF_ENCRYPT_AES_256, owner_pw="owner-secret", user_pw="user-secret")
        doc.close()
        with self.assertRaisesRegex(ProbeRejected, "encrypted_pdf"):
            probe(source)

    def test_cli_private_and_non_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            source = folder / "confidential-sample.pdf"
            source.write_bytes(make_pdf(basic_page))
            output = folder / "private.json"
            stream = io.StringIO()
            with contextlib.redirect_stdout(stream):
                self.assertEqual(main([str(source), "--output", str(output)]), 0)
            self.assertNotIn("Hello", stream.getvalue())
            self.assertNotIn(str(source), stream.getvalue())
            self.assertEqual(json.loads(output.read_text())["pages"][0]["objects"][0]["text"], "Hello")
            if os.name != "nt":
                self.assertEqual(output.stat().st_mode & 0o777, 0o600)
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                self.assertEqual(main([str(source), "--output", str(output)]), 2)
            self.assertEqual(err.getvalue().strip(), "rejected:io_failure")


if __name__ == "__main__":
    unittest.main()
