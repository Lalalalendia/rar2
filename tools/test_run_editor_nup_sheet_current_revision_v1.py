#!/usr/bin/env python3
"""Contract tests for two fresh current-Editor PDF to nine-up replays."""
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import fitz
import run_editor_nup_sheet_current_revision_v1 as runner

SOURCE=b"unmodified-test-PUB"
PROJECT=b'{"operations":[]}'
def sha(raw):return hashlib.sha256(raw).hexdigest()
def card(text):
    doc=fitz.open();p=doc.new_page(width=85*72/25.4,height=55*72/25.4)
    p.insert_text((12,28),text)
    data=doc.tobytes(garbage=4,deflate=True,no_new_id=True)
    doc.close();return data
def receipt(pdf):
    return {
        "receipt_version":"chaptera.editor-fixed-pdf-current-revision-receipt.v2",
        "receipt_kind":"real_hosted",
        "producer":{"implementation":"rar-editor-current-revision-fixed-pdf-v2","source_neutral_renderer":True},
        "source":{"sha256":sha(SOURCE),"byte_len":len(SOURCE),"immutable":True},
        "stage_0_1":{"project_sha256":sha(PROJECT)},
        "artifact":{"format":"pdf","sha256":sha(pdf),"byte_len":len(pdf)},
        "current_revision":{"project_state_id":"sha256:"+"a"*64,"mutation_target_count":4,
                            "story_target_state_current":True,"move_geometry_current":True,
                            "resize_geometry_current":True,"replacement_image_current":True},
        "renderer":{"summary":{"page_count":1,"node_painted":3,"node_partial":0,"node_unsupported":0,"diagnostic_codes":[]},
                    "target_reports":{name:{"disposition":"painted"} for name in
                                    ("move_node","resize_node","replace_image")}},
        "source_neutral_chain":{name:"sha256:"+"b"*64 for name in
               ("resource_input_sha256","render_request_sha256","scene_snapshot_id",
                "resources_id","renderer_repair_sha256")},
        "invariants":{**{name:True for name in ("current_editor_project_authoritative",
            "authoritative_rust_project_replay","source_pub_immutable",
            "replacement_exact_bytes_reach_renderer_request","move_target_painted",
            "resize_target_painted","replacement_target_painted","edited_story_materialized")},
            **{name:False for name in ("raw_document_text_emitted","raw_source_bytes_emitted",
               "replacement_asset_sha_emitted","native_pub_write_used",
               "pdf_renderer_reimplemented_in_rar")},
            "source_reparse_after_edit_count":0}}
class TwoFreshReplayTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        root=Path(self.tmp.name)
        self.inputs=dict(source=root/"source.pub",project=root/"project.json",
                         replacement=root/"replacement.png",stage01_receipt=root/"stage.json",
                         resource_input=root/"resources.json",yab_checkout=root/"producer")
        for key,raw in (("source",SOURCE),("project",PROJECT),("replacement",b"image"),
                        ("stage01_receipt",b'{"stage":true}'),("resource_input",b'{"resource":true}')):
            self.inputs[key].write_bytes(raw)
        self.inputs["yab_checkout"].mkdir()
        self.fake_runner=patch.object(runner,"CANONICAL_RUNNER",Path(__file__))
        self.fake_runner.start();self.addCleanup(self.fake_runner.stop)
    def fake_outputs(self,contents,mutate=False,bad_receipt=False):
        count=[]
        def simulate(cmd):
            n=len(count);count.append(cmd)
            self.assertIn("--source",cmd);self.assertIn("--pdf-output",cmd)
            out=lambda key:Path(cmd[cmd.index(key)+1])
            payload=contents[n]
            meta=receipt(payload)
            if bad_receipt:meta["artifact"]["sha256"]="0"*64
            out("--pdf-output").write_bytes(payload)
            out("--receipt-output").write_text(json.dumps(meta))
            if mutate:self.inputs["source"].write_bytes(b"changed")
        return simulate,count
    def run_pair(self):
        return runner.run_two_fresh_replays(**self.inputs,logical_page_id="card-1",gap_mm=5)
    def test_identical_two_real_consumers_and_no_input_mutation(self):
        pdf=card("EDITED-1");simulate,count=self.fake_outputs([pdf,pdf])
        with patch.object(runner,"_run_canonical_runner",side_effect=simulate):
            result,proof=self.run_pair()
        self.assertEqual(2,len(count))
        self.assertEqual(2,proof["fresh_replay"]["runs"])
        self.assertEqual(SOURCE,self.inputs["source"].read_bytes())
        self.assertEqual(PROJECT,self.inputs["project"].read_bytes())
        with fitz.open(stream=result.pdf_bytes,filetype="pdf") as out:
            self.assertEqual(9,out[0].get_text().count("EDITED-1"))
    def test_changed_second_native_pdf_fails_closed(self):
        simulate,count=self.fake_outputs([card("BEFORE"),card("AFTER")])
        with patch.object(runner,"_run_canonical_runner",side_effect=simulate):
            with self.assertRaisesRegex(Exception,"fresh_replay_output_mismatch"):self.run_pair()
        self.assertEqual(2,len(count))
    def test_forged_upstream_digest_fails(self):
        pdf=card("EDIT");simulate,count=self.fake_outputs([pdf,pdf],bad_receipt=True)
        with patch.object(runner,"_run_canonical_runner",side_effect=simulate):
            with self.assertRaises(Exception):self.run_pair()
        self.assertEqual(1,len(count))
    def test_changed_original_source_fails(self):
        pdf=card("EDIT");simulate,count=self.fake_outputs([pdf,pdf],mutate=True)
        with patch.object(runner,"_run_canonical_runner",side_effect=simulate):
            with self.assertRaisesRegex(Exception,"input_changed_during_replay"):self.run_pair()
    def test_symlink_not_admitted(self):
        link=Path(self.tmp.name)/"link.pub";link.symlink_to(self.inputs["source"])
        self.inputs["source"]=link
        with patch.object(runner,"_run_canonical_runner") as producer:
            with self.assertRaises(Exception):self.run_pair()
            producer.assert_not_called()
if __name__=="__main__":unittest.main()
