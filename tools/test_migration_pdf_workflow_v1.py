"""Offline source-free contract and executable end-to-end tests for the route."""
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
import os
from pathlib import Path
import tempfile
from unittest.mock import patch
import unittest
import zipfile

import migration_pdf_workflow_v1 as workflow

FAKE_PUB_CLI = '''#!/usr/bin/env python3
import hashlib,json,pathlib,sys
args=sys.argv[1:]
assert args[0]=='convert' and args[2:4]==['--to','pdf']
assert args[4]=='--output' and args[6]=='--fallback-font' and args[8:]==['--json']
source=pathlib.Path(args[1])
if b'FAIL' in source.read_bytes():
    sys.exit(9)
pdf=pathlib.Path(args[5])
pdf.write_bytes(b'%PDF-1.7\\n'+hashlib.sha256(source.read_bytes()).hexdigest().encode()+b'\\n')
pathlib.Path(str(pdf)+'.loss.json').write_text(json.dumps({'schema':'synthetic-only'}),encoding='utf-8')
pathlib.Path(str(pdf)+'.loss.txt').write_text('synthetic warning\\n',encoding='utf-8')
'''


class WorkflowTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.base = Path(tmp.name)
        self.sources = self.base / "sources"
        self.sources.mkdir()
        self.pdf = self.base / "pdf"
        self.reports = self.base / "reports"
        self.font = self.base / "fallback.ttf"
        self.font.write_bytes(b"fixture-font-only")
        self.cli = self.base / "fake-pub-cli"
        self.cli.write_text(FAKE_PUB_CLI, encoding="utf-8")
        self.cli.chmod(0o700)

    def _args(self, source=None, pdf=None, reports=None):
        return ["run", str(source or self.sources), "--pdf-dir", str(pdf or self.pdf),
                "--report-dir", str(reports or self.reports),
                "--fallback-font", str(self.font), "--pub-cli", str(self.cli)]

    def _captured(self, args):
        stdout, stderr = StringIO(), StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            result = workflow.main(args)
        return result, stdout.getvalue(), stderr.getvalue()

    def test_inventory_folder_recursive_with_counts_only(self):
        (self.sources / "nested").mkdir()
        (self.sources / "nested" / "c.PUB").write_bytes(b"C")
        (self.sources / "A.pub").write_bytes(b"A")
        (self.sources / "ignore.txt").write_bytes(b"private")
        result, out, err = self._captured(["inventory", str(self.sources)])
        self.assertEqual((result, err), (0, ""))
        data = json.loads(out)
        self.assertEqual(data["candidate_pub_count"], 2)
        self.assertNotIn("private", out)
        self.assertNotIn("nested", out)
        self.assertIn("no_decode", data["claim_boundary"])

    def test_inventory_zip(self):
        source = self.base / "collection.zip"
        with zipfile.ZipFile(source, "w") as z:
            z.writestr("nested/one.PUB", "synthetic bytes")
            z.writestr("notes.txt", "notes")
        self.assertEqual(workflow.inventory(source)["candidate_pub_count"], 1)
        self.assertEqual(workflow.inventory(source)["input_kind"], "zip")

    def test_inventory_empty_is_fail_closed(self):
        self.assertEqual(self._captured(["inventory", str(self.sources)])[0], 2)

    def test_inventory_rejects_input_symlink(self):
        alias = self.base / "alias"
        alias.symlink_to(self.sources, target_is_directory=True)
        self.assertEqual(self._captured(["inventory", str(alias)])[0], 2)

    def test_inventory_rejects_zip_escape(self):
        source = self.base / "unsafe.zip"
        with zipfile.ZipFile(source, "w") as z:
            z.writestr("../outside.pub", "bad")
        self.assertEqual(self._captured(["inventory", str(source)])[0], 2)

    def test_preflight_rejects_pdf_inside_source_without_run(self):
        with patch.object(workflow.batch, "run") as run:
            result, _, _ = self._captured(self._args(pdf=self.sources / "pdf"))
        self.assertEqual(result, 2)
        run.assert_not_called()

    def test_preflight_rejects_reports_inside_source_without_run(self):
        with patch.object(workflow.batch, "run") as run:
            result, _, _ = self._captured(self._args(reports=self.sources / "reports"))
        self.assertEqual(result, 2)
        run.assert_not_called()

    def test_preflight_rejects_report_pdf_overlap(self):
        with patch.object(workflow.batch, "run") as run:
            result, _, _ = self._captured(self._args(reports=self.pdf / "nested"))
        self.assertEqual(result, 2)
        run.assert_not_called()

    def test_preflight_rejects_symlink_output(self):
        alias = self.base / "alias"
        alias.symlink_to(self.base / "some-other")
        with patch.object(workflow.batch, "run") as run:
            result, _, _ = self._captured(self._args(pdf=alias))
        self.assertEqual(result, 2)
        run.assert_not_called()

    def test_invalid_timeout_before_side_effects(self):
        with patch.object(workflow.batch, "run") as run:
            result, _, _ = self._captured(self._args() + ["--timeout", "0"])
        self.assertEqual(result, 2)
        run.assert_not_called()

    def test_uses_existing_owners_without_second_converter(self):
        correct = {"integrity_confirmed_review_required": 2, "integrity_rejected": 0,
                   "exception": 0, "not_yet_in_checkpoint": 0}
        received = {"counts": correct, "total_sources": 2}
        with patch.object(workflow.batch, "run", return_value=(0, {"entries": []})) as producer, \
             patch.object(workflow.reporter, "generate", return_value=received) as verifier, \
             patch.object(workflow.reporter, "write_reports") as writer:
            result, out, err = self._captured(self._args())
        self.assertEqual((result, err), (0, ""))
        producer.assert_called_once()
        verifier.assert_called_once_with(self.pdf)
        writer.assert_called_once_with(received, self.reports, self.pdf)
        self.assertIn("integrity_review=2", out)

    def test_cannot_mask_conversion_failure_with_good_report(self):
        good = {"integrity_confirmed_review_required": 1, "integrity_rejected": 0,
                "exception": 0, "not_yet_in_checkpoint": 0}
        with patch.object(workflow.batch, "run", return_value=(1, {})), \
             patch.object(workflow.reporter, "generate", return_value={"counts": good, "total_sources": 1}), \
             patch.object(workflow.reporter, "write_reports"):
            self.assertEqual(self._captured(self._args())[0], 1)

    def test_integrity_mismatch_return_failure(self):
        counts = {"integrity_confirmed_review_required": 0, "integrity_rejected": 1,
                  "exception": 0, "not_yet_in_checkpoint": 0}
        with patch.object(workflow.batch, "run", return_value=(0, {})), \
             patch.object(workflow.reporter, "generate", return_value={"counts": counts, "total_sources": 1}), \
             patch.object(workflow.reporter, "write_reports"):
            self.assertEqual(self._captured(self._args())[0], 1)

    def test_existing_producer_rejection_not_leaked(self):
        with patch.object(workflow.batch, "run", side_effect=workflow.batch.InputRejected("output_locked")):
            result, stdout, stderr = self._captured(self._args())
        self.assertEqual((result, stdout), (2, ""))
        self.assertIn("output_locked", stderr)
        self.assertNotIn(str(self.sources), stderr)

    def test_actual_batch_and_report_folder_resume_and_failure(self):
        if getattr(workflow.batch, "LOCAL_STUB", False):
            self.skipTest("local checkout has no real batch authority; CI runs this with actual repo module")
        (self.sources / "healthy.pub").write_bytes(b"synthetic healthy pub bytes")
        (self.sources / "damaged.pub").write_bytes(b"FAIL synthetic pub bytes")
        originals = {f.name: f.read_bytes() for f in self.sources.iterdir()}
        first, out, err = self._captured(self._args())
        self.assertEqual(first, 1, err)
        self.assertIn("integrity_review=1", out)
        report = json.loads((self.reports / "operator-report.json").read_text(encoding="utf-8"))
        self.assertEqual(report["counts"]["exception"], 1)
        self.assertEqual(report["counts"]["integrity_confirmed_review_required"], 1)
        self.assertIn("damaged.pub", (self.reports / "exceptions.csv").read_text(encoding="utf-8"))
        manifest = json.loads((self.pdf / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(sorted(row["status"] for row in manifest["entries"]), ["converted", "failed"])
        second, _, _ = self._captured(self._args())
        self.assertEqual(second, 1)
        again = json.loads((self.pdf / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(sorted(row["status"] for row in again["entries"]), ["failed", "reused"])
        self.assertEqual({f.name: f.read_bytes() for f in self.sources.iterdir()}, originals)
        self.assertEqual(sorted(p.name for p in self.reports.iterdir()),
                         ["exceptions.csv", "operator-report.csv", "operator-report.json", "review-index.html"])
        self.assertIn("Not approved for delivery",
                      (self.reports / "review-index.html").read_text(encoding="utf-8"))

    def test_actual_batch_and_report_zip(self):
        if getattr(workflow.batch, "LOCAL_STUB", False):
            self.skipTest("local checkout has no real batch authority; CI runs this with actual repo module")
        source = self.base / "archive.zip"
        with zipfile.ZipFile(source, "w") as z:
            z.writestr("sub/issue.pub", b"synthetic pub bytes")
        first, out, err = self._captured(self._args(source=source))
        self.assertEqual(first, 0, err)
        self.assertIn("integrity_review=1", out)
        second, _, _ = self._captured(self._args(source=source))
        self.assertEqual(second, 0)
        manifest = json.loads((self.pdf / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["entries"][0]["status"], "reused")


if __name__ == "__main__":
    unittest.main()
