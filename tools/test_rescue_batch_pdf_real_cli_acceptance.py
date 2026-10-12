#!/usr/bin/env python3
"""Fast source-free contracts for actual Rust-CLI acceptance adapter.

These unit tests mock the executable call. The separate hosted acceptance must
build and run the real Rust PUB converter on a SHA-pinned public PUB fixture.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import stat
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import rescue_batch_pdf_v1 as batch
import rescue_batch_pdf_real_cli_acceptance as acceptance


class RealCliAcceptanceContract(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.work = Path(self.tmp.name)
        self.source = self.work / "public.pub"
        self.source.write_bytes(b"public synthetic nonprivate source bytes")
        self.font = self.work / "font.ttf"
        self.font.write_bytes(b"synthetic test font")
        self.cli = self.work / "mock-pub"
        self.cli.write_text("synthetic test executable marker", encoding="utf-8")
        self.cli.chmod(self.cli.stat().st_mode | stat.S_IXUSR)
        self.script = Path(acceptance.__file__).with_name("rescue_batch_pdf_v1.py")
        self.sha = hashlib.sha256(self.source.read_bytes()).hexdigest()
        self.calls: list[list[str]] = []

    def fake_conversion(self, command: list[str], **kwargs) -> subprocess.CompletedProcess:
        self.calls.append(command)
        self.assertEqual(command[1], "convert")
        self.assertEqual(command[3:5], ["--to", "pdf"])
        self.assertEqual(command[5], "--output")
        self.assertEqual(command[7], "--fallback-font")
        self.assertEqual(command[9:], ["--json"])
        source, output, font = Path(command[2]), Path(command[6]), Path(command[8])
        contents = source.read_bytes()
        if contents.startswith(b"not-a-valid-cfb"):
            return subprocess.CompletedProcess(command, 9)
        source_sha = hashlib.sha256(contents).hexdigest()
        font_sha = hashlib.sha256(font.read_bytes()).hexdigest()
        output.write_bytes(b"%PDF-1.7\n" + source_sha.encode() + b"\n%%EOF\n")
        report = {
            "schema_version": "free-pub-pdf-v0.1", "source": {"label": source.name},
            "conversion_profile": {"source": {"source_sha256": source_sha}},
            "target": {"format": "pdf"},
            "typography": {
                "fallback_font": {"fingerprint_sha256": font_sha},
                "disposition": "explicit_user_fallback_not_source_font",
            },
        }
        Path(str(output) + ".loss.json").write_text(json.dumps(report), encoding="utf-8")
        Path(str(output) + ".loss.txt").write_text("synthetic loss report", encoding="utf-8")
        return subprocess.CompletedProcess(command, 0)

    def inprocess_batch_once(self, batch_tool: Path, root: Path, output: Path,
                             font: Path, cli: Path, expected_status: int) -> dict:
        options = SimpleNamespace(root=root, output=output, fallback_font=font,
                                  pub_cli=str(cli), timeout=120)
        with patch.object(batch.subprocess, "run", side_effect=self.fake_conversion):
            status, receipt = batch.run(options)
        self.assertEqual(status, expected_status)
        return receipt

    def test_folder_zip_resume_tamper_failure_isolation_receipt(self) -> None:
        with patch.object(acceptance, "batch_once", side_effect=self.inprocess_batch_once):
            result = acceptance.accept(self.source, self.sha, self.font, self.cli, self.script)
        self.assertEqual(result["schema"], acceptance.RECEIPT_SCHEMA)
        self.assertEqual(result["source_sha256"], self.sha)
        self.assertEqual(result["folder"]["first"], "converted+failed")
        self.assertEqual(result["folder"]["second"], "reused+failed")
        self.assertEqual(result["folder"]["tamper"], "reconverted+failed")
        self.assertEqual(result["zip"]["second"], "reused")
        self.assertTrue(all(value for key, value in result["invariants"].items()
                            if key != "raw_pub_or_pdf_artifacts_uploaded"))
        self.assertIs(result["invariants"]["raw_pub_or_pdf_artifacts_uploaded"], False)
        self.assertEqual(len(self.calls), 6)
        self.assertNotIn(str(self.work), json.dumps(result))

    def test_wrong_pinned_source_rejected_without_running_converter(self) -> None:
        with self.assertRaisesRegex(acceptance.AcceptanceFailure, "pinned_source_sha_mismatch"):
            acceptance.accept(self.source, "0" * 64, self.font, self.cli, self.script)
        self.assertFalse(self.calls)

    def test_source_safe_cli_error_and_no_receipt_on_bad_identity(self) -> None:
        receipt = self.work / "acceptance.json"
        with patch.object(acceptance, "batch_once", side_effect=self.inprocess_batch_once):
            status = acceptance.main([
                "--source", str(self.source), "--source-sha256", "0" * 64,
                "--font", str(self.font), "--pub-cli", str(self.cli),
                "--batch-tool", str(self.script), "--receipt", str(receipt)])
        self.assertEqual(status, 1)
        self.assertFalse(receipt.exists())
        self.assertFalse(self.calls)


if __name__ == "__main__":
    unittest.main()
