import hashlib
from copy import deepcopy
import unittest
from print_composer_nup_current_revision_v1 import (
    validate_current_fixed_pdf_receipt_v2,compose_verified_current_editor_card_nup_v1
)
from print_composer_nup_v1 import PrintComposerNupError
import fitz

SRC=b"pinned-no-write-source"
PROJECT=b'{"operations":[{"kind":"replace_story_range"}]}'
def digest(x):return hashlib.sha256(x).hexdigest()
def sample():
    doc=fitz.open()
    p=doc.new_page(width=85*72/25.4,height=55*72/25.4)
    p.insert_text((12,30),"EDITED-STORY-01")
    blob=doc.tobytes(garbage=4,deflate=True,no_new_id=True)
    doc.close()
    return blob
def receipt(pdf):
    return {
      "receipt_version":"chaptera.editor-fixed-pdf-current-revision-receipt.v2",
      "receipt_kind":"real_hosted",
      "producer":{"implementation":"rar-editor-current-revision-fixed-pdf-v2","source_neutral_renderer":True},
      "source":{"sha256":digest(SRC),"byte_len":len(SRC),"immutable":True},
      "stage_0_1":{"project_sha256":digest(PROJECT)},
      "artifact":{"format":"pdf","sha256":digest(pdf),"byte_len":len(pdf)},
      "current_revision":{"project_state_id":"sha256:"+"a"*64,
          "mutation_target_count":4,"story_target_state_current":True,
          "move_geometry_current":True,"resize_geometry_current":True,
          "replacement_image_current":True},
      "renderer":{"summary":{"page_count":1,"node_painted":3,"node_partial":0,"node_unsupported":0,"diagnostic_codes":[]},
          "target_reports":{x:{"disposition":"painted"} for x in
          ("move_node","resize_node","replace_image")}},
      "source_neutral_chain":{x:"sha256:"+"b"*64 for x in
          ("resource_input_sha256","render_request_sha256","scene_snapshot_id","resources_id",
           "renderer_repair_sha256")},
      "invariants":{**{x:True for x in
          ("current_editor_project_authoritative","authoritative_rust_project_replay",
           "source_pub_immutable","replacement_exact_bytes_reach_renderer_request",
           "move_target_painted","resize_target_painted","replacement_target_painted",
           "edited_story_materialized")},
          **{x:False for x in
          ("raw_document_text_emitted","raw_source_bytes_emitted","replacement_asset_sha_emitted",
           "native_pub_write_used","pdf_renderer_reimplemented_in_rar")},
           "source_reparse_after_edit_count":0},
    }
class CurrentRevisionBindingTests(unittest.TestCase):
    def test_canonical_current_pdf_nine_up(self):
        blob=sample()
        def run():
            return compose_verified_current_editor_card_nup_v1(
                upstream_receipt=receipt(blob), original_pub_bytes=SRC,
                editor_project_bytes=PROJECT,current_fixed_pdf_bytes=blob,
                logical_page_id="page:1")
        a,b=run(),run()
        self.assertEqual(a.pdf_bytes,b.pdf_bytes)
        self.assertEqual(a.receipt,b.receipt)
        with fitz.open(stream=a.pdf_bytes,filetype="pdf") as pdf:
            self.assertEqual(9,pdf[0].get_text().count("EDITED-STORY-01"))
        self.assertEqual(9,len(a.receipt["placements"]))
        self.assertEqual(digest(PROJECT),a.receipt["upstream"]["project_sha256"])
    def test_fail_closed_on_mismatches(self):
        blob=sample()
        for key,value in (("artifact","0"*64),("source","0"*64),("stage_0_1","0"*64)):
            bad=deepcopy(receipt(blob))
            field={"artifact":"sha256","source":"sha256","stage_0_1":"project_sha256"}[key]
            bad[key][field]=value
            with self.subTest(key=key), self.assertRaises(PrintComposerNupError):
                validate_current_fixed_pdf_receipt_v2(
                    receipt=bad,source_bytes=SRC,project_bytes=PROJECT,fixed_pdf_bytes=blob)
        bad=receipt(blob)
        bad["invariants"]["edited_story_materialized"]=False
        with self.assertRaises(PrintComposerNupError):
            validate_current_fixed_pdf_receipt_v2(
                receipt=bad,source_bytes=SRC,project_bytes=PROJECT,fixed_pdf_bytes=blob)

    def test_partial_unsupported_and_diagnostic_pdf_cannot_be_printed_silently(self):
        blob=sample()
        for field,value in (
            ("node_partial",1),
            ("node_unsupported",1),
            ("diagnostic_codes",["pdf.image.unsupported"]),
            ("page_count",2),
            ("node_painted",0),
            ("node_partial",True),
        ):
            degraded=deepcopy(receipt(blob))
            degraded["renderer"]["summary"][field]=value
            with self.subTest(field=field,value=value), self.assertRaises(PrintComposerNupError):
                validate_current_fixed_pdf_receipt_v2(
                    receipt=degraded,source_bytes=SRC,project_bytes=PROJECT,fixed_pdf_bytes=blob)

if __name__=="__main__":
    unittest.main()
