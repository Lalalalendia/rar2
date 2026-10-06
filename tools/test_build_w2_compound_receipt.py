#!/usr/bin/env python3
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILDER = ROOT / "tools" / "build_w2_compound_receipt.py"
SOURCE_SHA = "aeac4c03181582008c18655ad77b90a957b00eeddcc0b1c45f6d40ca88c765eb"
STATE = "sha256:" + "1" * 64


def kernel():
    return {
        "receipt_version": "chaptera.w2-same-document-kernel.v1",
        "source": {"sha256": SOURCE_SHA, "byte_len": 4286464, "immutable": True},
        "parent": {"operation_count": 0},
        "fork_initial": {
            "initial_state_preserved": True,
            "identity_rekeyed": True,
            "provenance_exact": True,
        },
        "next_issue": {
            "operation_count": 4,
            "state_id": STATE,
            "recipe_state_id": STATE,
            "recipe_state_equal": True,
            "recipe_operations_equal": True,
            "recipe_assets_equal": True,
            "fork_identity_preserved": True,
            "fresh_reopen_exact": True,
        },
        "layout": {"state": "fits", "explicit": True},
        "editable_output": {"format": "odg", "byte_len": 123, "nonempty": True},
        "invariants": {
            "same_document": True,
            "parent_unchanged_after_next_issue_edit": True,
            "source_pub_immutable": True,
            "source_write_count": 0,
            "wrap_preservation_claimed": False,
        },
    }


def fixed():
    return {
        "receipt_version": "chaptera.editor-fixed-pdf-current-revision-receipt.v2",
        "source": {"sha256": SOURCE_SHA, "byte_len": 4286464, "immutable": True},
        "current_revision": {
            "project_state_id": STATE,
            "mutation_target_count": 4,
            "story_target_state_current": True,
            "move_geometry_current": True,
            "resize_geometry_current": True,
            "replacement_image_current": True,
        },
        "renderer": {"summary": {"page_count": 10}},
        "artifact": {"format": "pdf", "sha256": "2" * 64},
        "invariants": {
            "source_pub_immutable": True,
            "source_reparse_after_edit_count": 0,
            "current_editor_project_authoritative": True,
        },
    }


class W2CompoundReceiptTests(unittest.TestCase):
    def run_builder(self, k, p):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        kp = root / "kernel.json"
        fp = root / "fixed.json"
        out = root / "compound.json"
        kp.write_text(json.dumps(k), encoding="utf-8")
        fp.write_text(json.dumps(p), encoding="utf-8")
        completed = subprocess.run(
            [
                sys.executable,
                str(BUILDER),
                "--kernel",
                str(kp),
                "--fixed-output",
                str(fp),
                "--output",
                str(out),
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        return completed, out

    def test_builds_same_document_receipt(self):
        completed, out = self.run_builder(kernel(), fixed())
        self.assertEqual(completed.returncode, 0, completed.stderr + completed.stdout)
        value = json.loads(out.read_text(encoding="utf-8"))
        self.assertEqual(value["evidence_mode"], "same_document")
        self.assertTrue(value["invariants"]["same_document_claim"])
        self.assertTrue(value["invariants"]["fixed_output_state_matches_next_issue"])
        self.assertEqual(value["fixed_output"]["page_count"], 10)
        self.assertEqual(value["kernel"]["layout_state"], "fits")

    def test_rejects_fixed_output_state_mismatch(self):
        p = fixed()
        p["current_revision"]["project_state_id"] = "sha256:" + "3" * 64
        completed, _ = self.run_builder(kernel(), p)
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(
            "fixed output is not bound to the exact forked next-issue effective state",
            completed.stderr + completed.stdout,
        )

    def test_rejects_page_count_regression(self):
        p = fixed()
        p["renderer"]["summary"]["page_count"] = 14
        completed, _ = self.run_builder(kernel(), p)
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(
            "exactly 10 customer-visible pages",
            completed.stderr + completed.stdout,
        )

    def test_rejects_cross_fixture_kernel(self):
        k = kernel()
        k["source"]["sha256"] = "0" * 64
        completed, _ = self.run_builder(k, fixed())
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(
            "exact May-2023 fixture",
            completed.stderr + completed.stdout,
        )


if __name__ == "__main__":
    unittest.main()
