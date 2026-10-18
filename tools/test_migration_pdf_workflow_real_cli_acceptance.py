"""Source-free negative contracts for physical Migration PDF workflow acceptance.

The real binary/real PUB test lives in a separate CI job. These tests never
download, decode or embed an actual Publisher document or font.
"""
from contextlib import redirect_stderr
from io import StringIO
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import migration_pdf_workflow_real_cli_acceptance as proof


class PhysicalRouteAcceptanceContract(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.pub = self.root / "fixture.pub"
        self.font = self.root / "font.ttf"
        self.cli = self.root / "pub"
        self.route = self.root / "migration-workflow.py"
        self.receipt = self.root / "receipt.json"
        self.font.write_bytes(b"synthetic font marker only")
        self.cli.write_bytes(b"synthetic executable marker only")
        self.route.write_bytes(b"synthetic route marker only")

    def args(self, sha: str = proof.PUBLIC_FIXTURE_SHA) -> list[str]:
        return ["--source", str(self.pub), "--source-sha256", sha,
                "--font", str(self.font), "--pub-cli", str(self.cli),
                "--workflow", str(self.route), "--receipt", str(self.receipt)]

    def test_refuses_source_identity_argument_before_any_io(self) -> None:
        stderr = StringIO()
        with redirect_stderr(stderr), patch.object(proof, "accept") as accepted:
            code = proof.main(self.args("0" * 64))
        self.assertEqual(code, 1)
        accepted.assert_not_called()
        self.assertIn("fixture_identity_argument_drift", stderr.getvalue())
        self.assertFalse(self.receipt.exists())

    def test_missing_original_cannot_create_receipt(self) -> None:
        stderr = StringIO()
        with redirect_stderr(stderr):
            code = proof.main(self.args())
        self.assertEqual(code, 1)
        self.assertIn("required_input_not_regular", stderr.getvalue())
        self.assertFalse(self.receipt.exists())

    def test_wrong_length_rejected_before_converter(self) -> None:
        self.pub.write_bytes(b"not the approved document")
        stderr = StringIO()
        with redirect_stderr(stderr), patch.object(proof, "invoke") as invoked:
            code = proof.main(self.args())
        self.assertEqual(code, 1)
        self.assertIn("pinned_real_pub_size_drift", stderr.getvalue())
        invoked.assert_not_called()

    def test_json_receipt_reader_rejects_symlink(self) -> None:
        valid = self.root / "valid.json"
        valid.write_text('{"schema":"test"}', encoding="utf-8")
        alias = self.root / "receipt-link.json"
        alias.symlink_to(valid)
        with self.assertRaisesRegex(proof.AcceptanceFailure, "missing_or_unsafe_receipt"):
            proof.read_json(alias)

    def test_json_receipt_reader_rejects_nonobject(self) -> None:
        receipt = self.root / "not-an-object.json"
        receipt.write_text("[]", encoding="utf-8")
        with self.assertRaisesRegex(proof.AcceptanceFailure, "receipt_not_object"):
            proof.read_json(receipt)

    def test_operator_csv_reader_rejects_missing(self) -> None:
        with self.assertRaisesRegex(proof.AcceptanceFailure, "missing_operator_csv"):
            proof.read_csv(self.root / "not-there.csv")

    def test_operator_csv_reader_handles_one_failure(self) -> None:
        csv = self.root / "exceptions.csv"
        csv.write_text("source_relative_path,state\nbad.pub,exception\n", encoding="utf-8")
        self.assertEqual(proof.read_csv(csv), [{"source_relative_path": "bad.pub",
                                                "state": "exception"}])

    def test_real_route_unexpected_process_status_fails_closed(self) -> None:
        result = subprocess.CompletedProcess(["python3"], 2, stdout=b"source-private",
                                             stderr=b"source-private")
        with patch.object(proof.subprocess, "run", return_value=result) as executed:
            with self.assertRaisesRegex(proof.AcceptanceFailure,
                                        "physical_route_exit_mismatch"):
                proof.invoke(self.route, "inventory", self.pub, expected=0)
        executed.assert_called_once()
        self.assertNotIn(str(self.root), str(result.returncode))

    def test_receipt_only_written_after_success(self) -> None:
        self.pub.write_bytes(b"not publisher source")
        with patch.object(proof, "accept",
                          side_effect=proof.AcceptanceFailure("physical_not_confirmed")):
            stderr = StringIO()
            with redirect_stderr(stderr):
                code = proof.main(self.args())
        self.assertEqual(code, 1)
        self.assertFalse(self.receipt.exists())
        self.assertNotIn(str(self.root), stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
