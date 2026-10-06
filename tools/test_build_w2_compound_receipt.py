#!/usr/bin/env python3
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILDER = ROOT / "tools" / "build_w2_compound_receipt.py"

class W2CompoundReceiptTests(unittest.TestCase):
    def test_builds_explicit_split_evidence_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            d = pathlib.Path(tmp)
            newsletter = {
                "receipt_version":"chaptera.editor-desktop-continuity-acceptance.v2",
                "source":{"sha256":"1"*64,"immutable":True},
                "export":{"package_valid":True},
            }
            overset = {
                "receipt_version":"chaptera.authoring-overset-receipt.v1",
                "source_hash":"2"*64,
                "states":{"accepted":{"state":"overset"}},
                "output_probe":{"overset_state_explicit":True},
            }
            fork = {
                "receipt_version":"chaptera.project-fork-receipt.v1",
                "source_hash":"3"*64,
                "invariants":{
                    "initial_state_preserved":True,
                    "parent_unchanged_after_fork_edit":True,
                    "project_id_rekeyed":True,
                    "document_id_rekeyed":True,
                    "history_id_rekeyed":True,
                    "genesis_revision_id_rekeyed":True,
                },
            }
            pdf = {
                "id":37474235340,
                "head_sha":"c92767c4e0236c48ec50aae2263e5e466881e29c",
                "status":"completed",
                "conclusion":"success",
            }
            paths = {}
            for name, value in (("newsletter",newsletter),("overset",overset),("fork",fork),("pdf",pdf)):
                p = d / f"{name}.json"
                p.write_text(json.dumps(value), encoding="utf-8")
                paths[name] = p
            out = d / "compound.json"
            completed = subprocess.run([
                sys.executable, str(BUILDER),
                "--newsletter", str(paths["newsletter"]),
                "--overset", str(paths["overset"]),
                "--fork", str(paths["fork"]),
                "--pdf-run", str(paths["pdf"]),
                "--output", str(out),
            ], cwd=ROOT, text=True, capture_output=True, check=False)
            self.assertEqual(completed.returncode, 0, completed.stderr + completed.stdout)
            value = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(value["evidence_mode"], "split_evidence")
            self.assertFalse(value["invariants"]["same_document_claim"])
            self.assertEqual(value["newsletter"]["source_hash"], "1"*64)
            self.assertEqual(value["overset"]["source_hash"], "2"*64)
            self.assertEqual(value["project_fork"]["source_hash"], "3"*64)
            self.assertTrue(value["fixed_output"]["verified_live"])

    def test_rejects_wrong_fixed_output_head(self):
        with tempfile.TemporaryDirectory() as tmp:
            d = pathlib.Path(tmp)
            values = {
                "newsletter":{"receipt_version":"chaptera.editor-desktop-continuity-acceptance.v2","source":{"sha256":"1"*64,"immutable":True},"export":{"package_valid":True}},
                "overset":{"receipt_version":"chaptera.authoring-overset-receipt.v1","source_hash":"2"*64,"states":{"accepted":{"state":"overset"}},"output_probe":{"overset_state_explicit":True}},
                "fork":{"receipt_version":"chaptera.project-fork-receipt.v1","source_hash":"3"*64,"invariants":{"initial_state_preserved":True,"parent_unchanged_after_fork_edit":True,"project_id_rekeyed":True,"document_id_rekeyed":True,"history_id_rekeyed":True,"genesis_revision_id_rekeyed":True}},
                "pdf":{"id":37474235340,"head_sha":"0"*40,"status":"completed","conclusion":"success"},
            }
            for name, value in values.items():
                (d/f"{name}.json").write_text(json.dumps(value), encoding="utf-8")
            completed = subprocess.run([
                sys.executable, str(BUILDER),
                "--newsletter", str(d/"newsletter.json"),
                "--overset", str(d/"overset.json"),
                "--fork", str(d/"fork.json"),
                "--pdf-run", str(d/"pdf.json"),
                "--output", str(d/"out.json"),
            ], cwd=ROOT, text=True, capture_output=True, check=False)
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("fixed-output head mismatch", completed.stderr + completed.stdout)

if __name__ == "__main__":
    unittest.main()
