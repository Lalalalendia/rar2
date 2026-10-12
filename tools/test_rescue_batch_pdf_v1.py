#!/usr/bin/env python3
"""Synthetic/offline acceptance for the PUB->PDF batch orchestrator.

The isolated fake CLI proves subprocess and file receipt behavior, not PDF fidelity.
"""
from __future__ import annotations

import json
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from types import SimpleNamespace
from unittest.mock import patch
import hashlib
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile

import rescue_batch_pdf_v1 as batch
from rescue_batch_pdf_v1 import InputRejected, output_name, safe_relative, sha256

SCRIPT = Path(__file__).with_name("rescue_batch_pdf_v1.py")
FAKE_CLI = '''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
args=sys.argv[1:]
assert args[0]=="convert" and args[2:4]==["--to","pdf"]
source=pathlib.Path(args[1]); output=pathlib.Path(args[5]); font=pathlib.Path(args[7])
assert args[4]=="--output" and args[6]=="--fallback-font" and args[8:]==["--json"]
with open(os.environ["RESCUE_FAKE_CALLS"],"a",encoding="utf-8") as log: log.write("called\\n")
contents=source.read_bytes()
if b"FAIL" in contents: sys.exit(9)
output.write_bytes(b"%PDF-1.7\\n"+hashlib.sha256(contents+font.read_bytes()).hexdigest().encode()+b"\\n")
pathlib.Path(str(output)+".loss.json").write_text(json.dumps({"schema":"synthetic-only"}),encoding="utf-8")
pathlib.Path(str(output)+".loss.txt").write_text("Synthetic fallback warning\\n",encoding="utf-8")
print("synthetic-only")
'''


class BatchPdfTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.root = self.home / "sources"
        self.root.mkdir()
        self.output = self.home / "out"
        self.font = self.home / "fallback.ttf"
        self.font.write_bytes(b"synthetic-font-resource")
        self.cli = self.home / "pub-synthetic"
        self.cli.write_text(FAKE_CLI, encoding="utf-8")
        self.cli.chmod(0o700)
        self.calls = self.home / "calls.txt"

    def fake_converter(self, command: list[str], **kwargs) -> subprocess.CompletedProcess:
        self.assertEqual(command[1], "convert")
        self.assertEqual(command[3:5], ["--to", "pdf"])
        self.assertEqual(command[5], "--output")
        self.assertEqual(command[7], "--fallback-font")
        self.assertEqual(command[9:], ["--json"])
        contents = Path(command[2]).read_bytes()
        with self.calls.open("a", encoding="utf-8") as log:
            log.write("called\n")
        if b"FAIL" in contents:
            return subprocess.CompletedProcess(command, 9)
        output = Path(command[6])
        digest = hashlib.sha256(contents + Path(command[8]).read_bytes()).hexdigest()
        output.write_bytes(b"%PDF-1.7\n" + digest.encode() + b"\n")
        Path(str(output) + ".loss.json").write_text('{"schema":"synthetic-only"}', encoding="utf-8")
        Path(str(output) + ".loss.txt").write_text("Synthetic fallback warning\n", encoding="utf-8")
        return subprocess.CompletedProcess(command, 0)

    def invoke(self, root: Path | None = None, output: Path | None = None,
               real_process: bool = False):
        args = [str(root or self.root), "--output", str(output or self.output),
                "--fallback-font", str(self.font), "--pub-cli", str(self.cli), "--json"]
        if real_process:
            return subprocess.run([sys.executable, str(SCRIPT), *args], text=True,
                                  capture_output=True, env={**os.environ, "RESCUE_FAKE_CALLS": str(self.calls)},
                                  timeout=60, check=False)
        stdout, stderr = StringIO(), StringIO()
        with patch.object(batch.subprocess, "run", side_effect=self.fake_converter):
            with redirect_stdout(stdout), redirect_stderr(stderr):
                code = batch.main(args)
        return SimpleNamespace(returncode=code, stdout=stdout.getvalue(), stderr=stderr.getvalue())

    def calls_count(self) -> int:
        return len(self.calls.read_text(encoding="utf-8").splitlines()) if self.calls.exists() else 0

    def manifest(self) -> dict:
        return json.loads((self.output / "manifest.json").read_text(encoding="utf-8"))

    def test_folder_recursive_and_deterministic_order(self):
        (self.root / "B").mkdir()
        (self.root / "B" / "b.PUB").write_bytes(b"second PUB")
        (self.root / "a.pub").write_bytes(b"first PUB")
        (self.root / "other.txt").write_text("ignored", encoding="utf-8")
        result = self.invoke(real_process=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = self.manifest()
        self.assertEqual(receipt["schema"], "chaptera.rescue-batch-pdf.v1")
        self.assertEqual(receipt["total_sources"], 2)
        self.assertEqual([e["source_relative_path"] for e in receipt["entries"]], ["B/b.PUB", "a.pub"])
        self.assertEqual([e["status"] for e in receipt["entries"]], ["converted"]*2)
        self.assertEqual(self.calls_count(), 2)
        for entry in receipt["entries"]:
            self.assertTrue((self.output / entry["output_pdf"]).read_bytes().startswith(b"%PDF-"))
            self.assertTrue((self.output / (entry["output_pdf"]+".loss.json")).exists())
            self.assertTrue((self.output / (entry["output_pdf"]+".loss.txt")).exists())
            self.assertEqual(len(entry["pdf_sha256"]), 64)

    def test_resume_reuses_verified_results(self):
        (self.root / "page.pub").write_bytes(b"one source")
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(self.manifest()["entries"][0]["status"], "reused")
        self.assertEqual(self.calls_count(), 1)

    def test_mutated_source_changes_output_name_and_reconverts(self):
        source = self.root / "page.pub"
        source.write_bytes(b"one")
        self.assertEqual(self.invoke().returncode, 0)
        first = self.manifest()["entries"][0]["output_pdf"]
        source.write_bytes(b"two")
        self.assertEqual(self.invoke().returncode, 0)
        second = self.manifest()["entries"][0]["output_pdf"]
        self.assertNotEqual(first, second)
        self.assertEqual(self.calls_count(), 2)

    def test_font_change_invalidates_reuse(self):
        (self.root / "page.pub").write_bytes(b"one")
        self.assertEqual(self.invoke().returncode, 0)
        self.font.write_bytes(b"another-font")
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(self.calls_count(), 2)
        self.assertEqual(self.manifest()["entries"][0]["status"], "converted")

    def test_corrupted_loss_sidecar_invalidates_reuse(self):
        (self.root / "page.pub").write_bytes(b"one")
        self.assertEqual(self.invoke().returncode, 0)
        row = self.manifest()["entries"][0]
        (self.output / (row["output_pdf"]+".loss.json")).write_text("tamper", encoding="utf-8")
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(self.calls_count(), 2)
        self.assertEqual(self.manifest()["entries"][0]["status"], "converted")

    def test_converter_error_is_per_file_and_resume_only_failed(self):
        (self.root / "a.pub").write_bytes(b"FAIL me")
        (self.root / "b.pub").write_bytes(b"okay")
        self.assertEqual(self.invoke().returncode, 1)
        self.assertEqual([r["status"] for r in self.manifest()["entries"]], ["failed", "converted"])
        self.assertEqual(self.manifest()["entries"][0]["reason"], "converter_failed")
        self.assertEqual(self.invoke().returncode, 1)
        self.assertEqual(self.calls_count(), 3)
        self.assertEqual([r["status"] for r in self.manifest()["entries"]], ["failed", "reused"])

    def test_zip_import_and_resume(self):
        archive = self.home / "book.zip"
        with zipfile.ZipFile(archive, "w") as handle:
            handle.writestr("root/one.pub", b"source One")
            handle.writestr("root/another.PUB", b"source Two")
            handle.writestr("root/readme.txt", b"no PDF")
        self.assertEqual(self.invoke(root=archive).returncode, 0)
        self.assertEqual(self.manifest()["input_kind"], "zip")
        self.assertEqual(self.invoke(root=archive).returncode, 0)
        self.assertEqual(self.calls_count(), 2)
        self.assertEqual({r["status"] for r in self.manifest()["entries"]}, {"reused"})
        self.assertFalse((self.output / "source.pub").exists())

    def test_zip_path_traversal_rejected_before_file_write(self):
        archive = self.home / "bad.zip"
        with zipfile.ZipFile(archive, "w") as handle:
            handle.writestr("../escape.pub", b"evil")
        result = self.invoke(root=archive)
        self.assertEqual(result.returncode, 2)
        self.assertIn("unsafe_archive_member", result.stderr)
        self.assertFalse((self.output / "manifest.json").exists())
        self.assertEqual(self.calls_count(), 0)

    def test_zip_symlink_rejected(self):
        archive = self.home / "link.zip"
        info = zipfile.ZipInfo("source.pub")
        info.create_system = 3
        info.external_attr = (stat.S_IFLNK | 0o777) << 16
        with zipfile.ZipFile(archive, "w") as handle:
            handle.writestr(info, "target.pub")
        result = self.invoke(root=archive)
        self.assertEqual(result.returncode, 2)
        self.assertIn("unsafe_archive_file_type", result.stderr)

    def test_input_output_nested_rejected(self):
        (self.root / "a.pub").write_bytes(b"one")
        result = self.invoke(output=self.root / "converted")
        self.assertEqual(result.returncode, 2)
        self.assertIn("source_and_output_trees_overlap", result.stderr)
        self.assertEqual(self.calls_count(), 0)

    def test_symlink_source_rejected(self):
        (self.root / "a.pub").write_bytes(b"good")
        try:
            (self.root / "b.pub").symlink_to(self.root / "a.pub")
        except (NotImplementedError, OSError):
            self.skipTest("symlinks unavailable")
        result = self.invoke()
        self.assertEqual(result.returncode, 2)
        self.assertIn("symlink_in_source", result.stderr)

    def test_stale_lock_rejected(self):
        (self.root / "a.pub").write_bytes(b"good")
        self.output.mkdir()
        (self.output / ".rescue-batch.lock").write_text("another worker", encoding="utf-8")
        result = self.invoke()
        self.assertEqual(result.returncode, 2)
        self.assertIn("output_locked", result.stderr)

    def test_collision_resistant_names(self):
        a = output_name("a/news.pub", sha256(b"same"))
        b = output_name("b/news.pub", sha256(b"same"))
        self.assertNotEqual(a, b)
        self.assertEqual(a, output_name("a/news.pub", sha256(b"same")))

    def test_safe_relative_rejects_windows_and_posix_escape(self):
        for unsafe in ("../a.pub", "/a.pub", "C:/a.pub", "foo\\bar.pub", "./a.pub", "a//b.pub", ""):
            with self.subTest(path=unsafe), self.assertRaises(InputRejected):
                safe_relative(unsafe)

    def test_invalid_manifest_is_not_silently_ignored(self):
        (self.root / "a.pub").write_bytes(b"good")
        self.output.mkdir()
        (self.output / "manifest.json").write_text("{}", encoding="utf-8")
        result = self.invoke()
        self.assertEqual(result.returncode, 2)
        self.assertIn("invalid_prior_manifest", result.stderr)

    def test_empty_root_rejected(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 2)
        self.assertIn("no_pub_sources", result.stderr)


if __name__ == "__main__":
    unittest.main()
