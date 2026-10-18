"""Offline source-free tests. No PUB file, font or PDF renderer is required."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import migration_operator_pdf_report_v1 as tool


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class OperatorReportV1Test(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.batch = self.root / "batch"
        self.batch.mkdir()
        self.report = self.root / "reports"
        self.name = "test--123456789abc--1234567890abcdef.pdf"
        self.artifacts = (b"%PDF-1.7\n123", b'{"schema":"test"}', b"warning\n")
        for suffix, data in zip(("", ".loss.json", ".loss.txt"), self.artifacts):
            (self.batch / (self.name + suffix)).write_bytes(data)
        self.success = dict(source_relative_path="nested/test.pub", source_sha256=sha(b"pub"),
                            status="converted", output_pdf=self.name, pdf_sha256=sha(self.artifacts[0]),
                            loss_json_sha256=sha(self.artifacts[1]), loss_text_sha256=sha(self.artifacts[2]))
        self.manifest = dict(schema=tool.SCHEMA, total_sources=1, input_kind="directory", entries=[self.success])
        self.save()

    def save(self):
        (self.batch/"manifest.json").write_text(json.dumps(self.manifest), encoding="utf-8")

    def test_good_output_still_needs_visual_review(self):
        rc=tool.main([str(self.batch),"--report-dir",str(self.report)])
        self.assertEqual(rc,0)
        data=json.loads((self.report/"operator-report.json").read_text())
        self.assertEqual(data["entries"][0]["state"],"integrity_confirmed_review_required")
        self.assertIn("not_visual_fidelity",data["claim_boundary"])
        self.assertEqual((self.report/"exceptions.csv").read_text().count("\n"),1)
        rendered=(self.report/"review-index.html").read_text(encoding="utf-8")
        self.assertIn("Not approved for delivery", rendered)
        self.assertIn("Open PDF", rendered)
        self.assertIn("Loss report", rendered)
        self.assertIn("visual", rendered)

    def test_failed_file_is_separate_exception(self):
        self.manifest["total_sources"]=2
        self.manifest["entries"].append({"source_relative_path":"broken.pub","status":"failed","reason":"converter_failed"})
        self.save()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),1)
        data=json.loads((self.report/"operator-report.json").read_text())
        self.assertEqual(data["counts"]["exception"],1)
        self.assertIn("broken.pub",(self.report/"exceptions.csv").read_text())

    def test_missing_checkpoint_entries_are_not_counted_success(self):
        self.manifest["total_sources"]=2
        self.save()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),1)
        data=json.loads((self.report/"operator-report.json").read_text())
        self.assertEqual(data["counts"]["not_yet_in_checkpoint"],1)

    def test_tampered_pdf_fails_closed(self):
        (self.batch/self.name).write_bytes(b"%PDF-1.7\nchanged")
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),1)
        data=json.loads((self.report/"operator-report.json").read_text())
        self.assertEqual(data["counts"]["integrity_rejected"],1)

    def test_corrupt_loss_json_fails_even_if_sha_matched(self):
        corrupt=b"not JSON"
        (self.batch/(self.name+".loss.json")).write_bytes(corrupt)
        self.success["loss_json_sha256"]=sha(corrupt)
        self.save()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),1)
        data=json.loads((self.report/"operator-report.json").read_text())
        self.assertEqual(data["entries"][0]["reason"],"invalid_pdf_or_loss_structure")

    def test_output_symlink_rejected_even_if_identical(self):
        original=self.batch/self.name
        clone=self.root/"clone.pdf"
        clone.write_bytes(original.read_bytes())
        original.unlink()
        original.symlink_to(clone)
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),1)
        data=json.loads((self.report/"operator-report.json").read_text())
        self.assertEqual(data["counts"]["integrity_rejected"],1)

    def test_malformed_duplicate_source_identity_rejected(self):
        self.manifest["entries"].append(dict(self.success,source_relative_path="nested/TEST.pub"))
        self.manifest["total_sources"]=2
        self.save()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),2)
        self.assertFalse(self.report.exists())

    def test_source_path_traversal_rejected(self):
        self.success["source_relative_path"]="../../other.pub"
        self.save()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),2)

    def test_report_dir_inside_batch_rejected(self):
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.batch/"out")]),2)

    def test_active_batch_lock_rejected(self):
        (self.batch/".rescue-batch.lock").write_text("active")
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),2)

    def test_spreadsheet_formula_injection_is_escaped(self):
        self.success["source_relative_path"]="=1+1.pub"
        self.save()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),0)
        csv=(self.report/"operator-report.csv").read_text()
        self.assertIn("'=1+1.pub",csv)

    def test_no_source_data_or_pub_copied_into_report(self):
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),0)
        self.assertEqual(sorted(x.name for x in self.report.iterdir()),["exceptions.csv","operator-report.csv","operator-report.json","review-index.html"])

    def test_atomic_repeat_deterministic(self):
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),0)
        first=(self.report/"operator-report.json").read_bytes()
        first_html=(self.report/"review-index.html").read_bytes()
        self.assertEqual(tool.main([str(self.batch),"--report-dir",str(self.report)]),0)
        self.assertEqual(first,(self.report/"operator-report.json").read_bytes())
        self.assertEqual(first_html,(self.report/"review-index.html").read_bytes())


if __name__ == "__main__":
    unittest.main()
