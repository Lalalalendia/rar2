"""Physical guest export binding checks for the 3x3 print compositor."""
import hashlib
import json
import unittest
import fitz
from print_composer_nup_guest_current_v1 import (
    compose_guest_current_nup_v1, validate_guest_current_nup_v1, PrintComposerNupError,
)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fixture():
    source = b"sample-original"
    doc = fitz.open()
    card = doc.new_page(width=85 * 72 / 25.4, height=55 * 72 / 25.4)
    card.insert_text((12, 26), "CHANGED-REVISION")
    pdf = doc.tobytes(no_new_id=True)
    doc.close()
    counts = dict(blocking=0, pages=1, painted=1, partial=0, unsupported=0,
                  shaped_lines=1, font_missing_glyph_lines=0, moved_nodes=1,
                  edited_stories=0, layout_warnings=0, pdf_warnings=0)
    loss = dict(schema_version="chaptera.temporary-edited-fixed-pdf-loss.v1",
                target="pdf", renderer="pub-pdf-v0.1", current_editor_state=True,
                can_serialize=True, source_hash=sha(source), counts=counts,
                fidelity="partial", limitations=["fallback_ubuntu_light_not_original_source_font"])
    report = dict(protocol_version="chaptera.temporary-edited-export-report.v1",
                  target="pdf", source_hash=sha(source), artifact_sha256=sha(pdf),
                  revision_id="sha256:" + "1" * 64,
                  canonical_authoring_revision_id="2" * 64,
                  loss_report=loss, loss_report_sha256=sha(json.dumps(
                      loss, sort_keys=True, separators=(",", ":")).encode()),
                  expires_at_ms=1800000000000)
    scene = dict(protocol_version="chaptera.reader-scene.v1",
                 source_hash=sha(source), revision_id=report["revision_id"],
                 pages=[dict(page_id="card1", width_emu=3060000, height_emu=1980000)])
    return dict(original_pub_bytes=source, current_fixed_pdf_bytes=pdf,
                private_export_report=report, current_reader_scene=scene)


class GuestNineUpBoundaries(unittest.TestCase):
    def test_current_pdf_nine_times_and_replay(self):
        args = fixture()
        first = compose_guest_current_nup_v1(**args)
        second = compose_guest_current_nup_v1(**args)
        with fitz.open(stream=first.pdf_bytes, filetype="pdf") as doc:
            self.assertEqual(len(doc), 1)
            self.assertEqual(doc[0].get_text().count("CHANGED-REVISION"), 9)
        self.assertEqual(first.pdf_bytes, second.pdf_bytes)
        self.assertEqual(first.preview_png_bytes, second.preview_png_bytes)
        self.assertEqual(len(first.receipt["placements"]), 9)
        self.assertEqual(first.receipt["upstream_guest"]["upstream_fidelity"], "partial")

    def test_revision_and_geometry_mismatch_rejected(self):
        args = fixture()
        args["current_reader_scene"]["revision_id"] = "sha256:" + "3" * 64
        with self.assertRaises(PrintComposerNupError):
            validate_guest_current_nup_v1(**args)
        args = fixture()
        args["current_reader_scene"]["pages"][0]["width_emu"] = 3240000
        with self.assertRaises(PrintComposerNupError):
            validate_guest_current_nup_v1(**args)

    def test_lost_content_must_not_be_multiplied(self):
        args = fixture()
        args["private_export_report"]["loss_report"]["counts"]["unsupported"] = 1
        loss = args["private_export_report"]["loss_report"]
        args["private_export_report"]["loss_report_sha256"] = sha(json.dumps(
            loss, sort_keys=True, separators=(",", ":")).encode())
        with self.assertRaisesRegex(PrintComposerNupError, "unresolved_visual_loss"):
            validate_guest_current_nup_v1(**args)


if __name__ == "__main__":
    unittest.main()
