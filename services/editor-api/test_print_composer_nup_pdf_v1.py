#!/usr/bin/env python3
"""PDF-to-PDF physical acceptance for the exact A4 nine-copy output consumer."""
import hashlib
from dataclasses import replace
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import fitz

from print_composer_nup_v1 import PrintComposerNupError, plan_editor_card_a4_9up_v1
from print_composer_nup_pdf_v1 import compose_editor_card_a4_9up_pdf_v1

PUB_BYTES = b"immutable-test-pub-source-42"
PUB_SHA = hashlib.sha256(PUB_BYTES).hexdigest()


def single_card_pdf(label: str) -> bytes:
    pdf = fitz.open()
    try:
        sheet = pdf.new_page(width=85 * 72 / 25.4, height=55 * 72 / 25.4)
        sheet.insert_text((20, 35), label, fontsize=12)
        sheet.draw_rect(fitz.Rect(18, 50, 55, 72), color=(0, 0, 0), fill=(0, 0.2, 0.7))
        return pdf.tobytes(garbage=4, deflate=True, no_new_id=True)
    finally:
        pdf.close()


def product(label: str):
    source = single_card_pdf(label)
    plan = plan_editor_card_a4_9up_v1(
        source_pub_sha256=PUB_SHA,
        editor_revision_id="editor-revision-42-" + label,
        current_fixed_pdf_sha256=hashlib.sha256(source).hexdigest(),
        logical_page_id="card-page-1",
    )
    return compose_editor_card_a4_9up_pdf_v1(
        plan=plan, current_fixed_pdf_bytes=source, source_pub_bytes=PUB_BYTES
    )


class PhysicalNupPdfV1Tests(unittest.TestCase):
    def test_actual_a4_pdf_contains_nine_current_vector_instances(self):
        created = product("EDITED-42")
        self.assertEqual(PUB_SHA, hashlib.sha256(PUB_BYTES).hexdigest())
        with fitz.open(stream=created.pdf_bytes, filetype="pdf") as doc:
            self.assertEqual(1, len(doc))
            page = doc[0]
            self.assertAlmostEqual(297 * 72 / 25.4, page.rect.width, delta=0.02)
            self.assertAlmostEqual(210 * 72 / 25.4, page.rect.height, delta=0.02)
            self.assertEqual(9, page.get_text().count("EDITED-42"))
            spans = [
                span for block in page.get_text("dict")["blocks"]
                if block.get("type") == 0
                for line in block.get("lines", [])
                for span in line.get("spans", [])
                if "EDITED-42" in span["text"]
            ]
            self.assertEqual(9, len(spans))
            # Independent PDF-side coordinates prove nine physical positions.
            for slot, span in enumerate(spans):
                target = created.receipt["placements"][slot]
                self.assertAlmostEqual(
                    target["x_emu"] / 12700 + 20, span["bbox"][0], delta=0.7
                )
                self.assertAlmostEqual(
                    target["y_top_emu"] / 12700 + 35 - 13.29,
                    span["bbox"][1], delta=1.0
                )
            self.assertEqual(9, len(page.get_drawings()))
            self.assertEqual(
                9,
                sum(
                    1
                    for block in page.get_text("dict")["blocks"]
                    if block.get("type") == 0
                    for line in block.get("lines", [])
                    for span in line.get("spans", [])
                    if "EDITED-42" in span["text"]
                ),
            )
        self.assertTrue(created.preview_png_bytes.startswith(b"\x89PNG\r\n\x1a\n"))
        pix = fitz.Pixmap(created.preview_png_bytes)
        self.assertGreater(pix.width, 700)
        self.assertGreater(pix.height, 500)
        self.assertEqual(9, len(created.receipt["placements"]))
        self.assertTrue(created.receipt["original_pub_hash_rechecked"])
        self.assertEqual([], created.receipt["loss_diagnostics"])
        self.assertEqual(hashlib.sha256(created.pdf_bytes).hexdigest(), created.receipt["pdf"]["sha256"])

    def test_same_edited_revision_replay_is_byte_identical(self):
        first = product("REPLAY-43")
        second = product("REPLAY-43")
        self.assertEqual(first.pdf_bytes, second.pdf_bytes)
        self.assertEqual(first.preview_png_bytes, second.preview_png_bytes)
        self.assertEqual(first.receipt, second.receipt)

    def test_different_revision_changes_all_nine_copies_not_baseline(self):
        before = product("BASELINE-40")
        after = product("ACCEPTED-41")
        self.assertNotEqual(before.receipt["pdf"]["sha256"], after.receipt["pdf"]["sha256"])
        with fitz.open(stream=after.pdf_bytes, filetype="pdf") as pdf:
            text = pdf[0].get_text()
            self.assertEqual(9, text.count("ACCEPTED-41"))
            self.assertNotIn("BASELINE-40", text)

    def test_noncanonical_physical_placement_refused(self):
        source = single_card_pdf("A")
        plan = plan_editor_card_a4_9up_v1(
            source_pub_sha256=PUB_SHA,
            editor_revision_id="a",
            current_fixed_pdf_sha256=hashlib.sha256(source).hexdigest(),
            logical_page_id="p1",
        )
        bad = replace(plan, placements=(replace(plan.placements[0], x_emu=1), *plan.placements[1:]))
        with self.assertRaisesRegex(PrintComposerNupError, "noncanonical_nup_placement"):
            compose_editor_card_a4_9up_pdf_v1(plan=bad, current_fixed_pdf_bytes=source)

    def test_source_digest_tamper_and_invalid_pdf_are_rejected(self):
        source = single_card_pdf("EDITED-42")
        plan = plan_editor_card_a4_9up_v1(
            source_pub_sha256=PUB_SHA,
            editor_revision_id="a",
            current_fixed_pdf_sha256=hashlib.sha256(source).hexdigest(),
            logical_page_id="p1",
        )
        with self.assertRaisesRegex(PrintComposerNupError, "source_pub_identity_mismatch"):
            compose_editor_card_a4_9up_pdf_v1(
                plan=plan, current_fixed_pdf_bytes=source, source_pub_bytes=b"wrong-source"
            )
        with self.assertRaisesRegex(PrintComposerNupError, "fixed_pdf_revision_binding_mismatch"):
            compose_editor_card_a4_9up_pdf_v1(
                plan=plan, current_fixed_pdf_bytes=single_card_pdf("TAMPERED")
            )
        with self.assertRaisesRegex(PrintComposerNupError, "fixed_pdf_invalid"):
            broken = b"%PDF-1.6\n" + b"x" * 80
            new_plan = plan_editor_card_a4_9up_v1(
                source_pub_sha256=PUB_SHA,
                editor_revision_id="a",
                current_fixed_pdf_sha256=hashlib.sha256(broken).hexdigest(),
                logical_page_id="p1",
            )
            compose_editor_card_a4_9up_pdf_v1(plan=new_plan, current_fixed_pdf_bytes=broken)

    def test_wrong_card_size_or_multiple_pages_do_not_scale_silently(self):
        pdf = fitz.open()
        try:
            pdf.new_page(width=200, height=200)
            wrong_size = pdf.tobytes()
            pdf.new_page(width=200, height=200)
            two_pages = pdf.tobytes()
        finally:
            pdf.close()
        for raw, code in (
            (wrong_size, "fixed_pdf_card_size_mismatch"),
            (two_pages, "fixed_pdf_must_have_one_logical_page"),
        ):
            plan = plan_editor_card_a4_9up_v1(
                source_pub_sha256=PUB_SHA,
                editor_revision_id="a",
                current_fixed_pdf_sha256=hashlib.sha256(raw).hexdigest(),
                logical_page_id="p1",
            )
            with self.subTest(code=code), self.assertRaisesRegex(PrintComposerNupError, code):
                compose_editor_card_a4_9up_pdf_v1(plan=plan, current_fixed_pdf_bytes=raw)

    def test_cli_emits_pdf_preview_receipt_without_changing_source(self):
        fixed = single_card_pdf("CLI-EDIT-44")
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            src_path, pdf_path = root / "source.pub", root / "current.pdf"
            output, preview, receipt = root / "sheet.pdf", root / "sheet.png", root / "receipt.json"
            src_path.write_bytes(PUB_BYTES)
            pdf_path.write_bytes(fixed)
            argv = [
                sys.executable,
                str(Path(__file__).with_name("print_composer_nup_pdf_v1.py")),
                "--source-pub", str(src_path),
                "--source-pub-sha256", PUB_SHA,
                "--current-fixed-pdf", str(pdf_path),
                "--editor-revision-id", "accepted-cli-44",
                "--logical-page-id", "card-1",
                "--output-pdf", str(output),
                "--output-preview", str(preview),
                "--output-receipt", str(receipt),
            ]
            completed = subprocess.run(argv, capture_output=True, text=True, check=False)
            self.assertEqual(0, completed.returncode, completed.stdout + completed.stderr)
            self.assertEqual(PUB_BYTES, src_path.read_bytes())
            self.assertEqual(fixed, pdf_path.read_bytes())
            self.assertTrue(output.read_bytes().startswith(b"%PDF-"))
            self.assertTrue(preview.read_bytes().startswith(b"\x89PNG"))
            self.assertIn('"original_pub_hash_rechecked": true', receipt.read_text())
            argv[argv.index(str(output))] = str(src_path)  # clobber guard
            failed = subprocess.run(argv, capture_output=True, text=True, check=False)
            self.assertEqual(2, failed.returncode)
            self.assertEqual(PUB_BYTES, src_path.read_bytes())


if __name__ == "__main__":
    unittest.main()
