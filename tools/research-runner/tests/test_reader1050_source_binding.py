#!/usr/bin/env python3
"""Source-free regressions for the Reader 1050 baseline/code SHA fence."""
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "reader1050_source_binding.py"
spec = importlib.util.spec_from_file_location("reader1050_source_binding", SCRIPT)
binding = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(binding)

SHA_A = "a" * 40
SHA_B = "b" * 40


class Reader1050SourceBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.write_receipt(SHA_A)

    def write_receipt(self, sha: str, schema: str = "chaptera.reader-1050-corpus-baseline.v1") -> None:
        (self.root / "acceptance.json").write_text(
            json.dumps({"schema": schema, "rar_ref": f"rar2:{sha}"}),
            encoding="utf-8",
        )

    def test_exact_checkout_and_receipt_are_admitted(self) -> None:
        result = binding.validate_source_binding(self.root, SHA_A, SHA_A)
        self.assertEqual(result["source_sha"], SHA_A)

    def test_moving_main_head_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "checkout mismatch"):
            binding.validate_source_binding(self.root, SHA_A, SHA_B)

    def test_receipt_from_different_run_is_rejected(self) -> None:
        self.write_receipt(SHA_B)
        with self.assertRaisesRegex(ValueError, "receipt SHA"):
            binding.validate_source_binding(self.root, SHA_A, SHA_A)

    def test_malformed_manual_source_sha_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "40 lowercase"):
            binding.validate_source_binding(self.root, "main", "main")

    def test_missing_receipt_fails_closed(self) -> None:
        (self.root / "acceptance.json").unlink()
        with self.assertRaisesRegex(ValueError, "expected one"):
            binding.validate_source_binding(self.root, SHA_A, SHA_A)

    def test_duplicate_receipts_fail_closed(self) -> None:
        nested = self.root / "duplicate"
        nested.mkdir()
        (nested / "acceptance.json").write_text(
            (self.root / "acceptance.json").read_text(encoding="utf-8"),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "expected one"):
            binding.validate_source_binding(self.root, SHA_A, SHA_A)

    def test_schema_mismatch_fails_closed(self) -> None:
        self.write_receipt(SHA_A, schema="chaptera.unrelated.v1")
        with self.assertRaisesRegex(ValueError, "schema mismatch"):
            binding.validate_source_binding(self.root, SHA_A, SHA_A)


if __name__ == "__main__":
    unittest.main()
