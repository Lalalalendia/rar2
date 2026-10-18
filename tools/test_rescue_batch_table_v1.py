#!/usr/bin/env python3
"""Fail-closed per-source, versioned private table AutoRestore receipt tests."""
from __future__ import annotations

import json
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import TestCase, main

from tools import rescue_batch_table_v1 as table


class TableBatchProfileTests(TestCase):
    def setUp(self):
        self.tmp = TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.sha = "a" * 64
        self.receipt_path = self.root / f"{self.sha}.json"

    def test_verified_profile_has_explicit_immutable_hashes(self):
        verified = table._receipt(self.sha, "verified", 2, "b" * 64, "c" * 64)
        table._save_receipt(self.receipt_path, verified)
        self.assertEqual(table._read_receipt(self.receipt_path, self.sha), verified)
        table._save_receipt(self.receipt_path, verified)

    def test_unavailable_is_not_false_positive_and_is_profile_bound(self):
        value = table._receipt(self.sha, "unavailable", 0)
        table._save_receipt(self.receipt_path, value)
        self.assertEqual(table._read_receipt(self.receipt_path, self.sha)["profile"],
                         "legacy_table_cells_v1")
        with self.assertRaises(table.TableBatchError):
            table._save_receipt(
                self.receipt_path,
                table._receipt(self.sha, "verified", 1, "b" * 64, "c" * 64),
            )

    def test_reject_forged_profile_or_source_sha(self):
        value = table._receipt(self.sha, "unavailable", 0)
        self.receipt_path.write_text(json.dumps(dict(value, profile="new_unsupported_profile")))
        with self.assertRaises(table.TableBatchError):
            table._read_receipt(self.receipt_path, self.sha)
        self.receipt_path.write_text(json.dumps(dict(value, source_sha256="b" * 64)))
        with self.assertRaises(table.TableBatchError):
            table._read_receipt(self.receipt_path, self.sha)

    def test_reject_receipt_tamper_and_boolean_count(self):
        value = table._receipt(self.sha, "verified", 2, "b" * 64, "c" * 64)
        for forged in (dict(value, status="unavailable"),
                       dict(value, story_count=True),
                       dict(value, story_count=-1),
                       dict(value, original_filename="customer.pub"),
                       dict(value, source_immutable=False),
                       dict(value, native_pub_repair_attempted=True)):
            with self.subTest(forged=forged):
                self.receipt_path.write_text(json.dumps(forged))
                with self.assertRaises(table.TableBatchError):
                    table._read_receipt(self.receipt_path, self.sha)

    def test_reject_untrusted_symlink(self):
        foreign = self.root / "foreign.json"
        foreign.write_text("{}")
        try:
            self.receipt_path.symlink_to(foreign)
        except OSError:
            self.skipTest("symlink privilege unavailable on Windows runner")
        with self.assertRaises((table.TableBatchError, ValueError)):
            table._read_receipt(self.receipt_path, self.sha)

    def test_invalid_profile_state_cannot_be_constructed(self):
        for kwargs in (
            ("verified", 0, "b" * 64, "c" * 64),
            ("unavailable", 1, None, None),
            ("verified", 1, None, None),
            ("verified", 1, "bad", "c" * 64),
        ):
            with self.subTest(kwargs=kwargs), self.assertRaises(table.TableBatchError):
                table._receipt(self.sha, *kwargs)


if __name__ == "__main__":
    main()
