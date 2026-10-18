#!/usr/bin/env python3
"""Source-free protocol tests for private legacy table cell evidence."""
from __future__ import annotations

import json
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import TestCase, main
from unittest.mock import patch
from types import SimpleNamespace

from tools import verify_rescue_table_text_evidence_v1 as proof


class PrivateTableEvidenceTests(TestCase):
    def setUp(self):
        self.tmp = TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "source.pub"
        self.source.write_bytes(b"synthetic-for-structure-only")
        self.target = self.root / "private"
        self.target.mkdir()
        story = {
            "story_key": "table:1", "utf16_start": 0, "utf16_end": 3,
            "text": "a\rb", "utf8_byte_len": 3,
            "text_sha256": proof.sha(b"a\rb"),
            "evidence_class": proof.EVIDENCE,
            "projection_carrier": "/Contents",
            "projection_byte_offset": 10, "projection_byte_len": 10,
            "rows": 1, "columns": 2,
            "cells": [
                {"row": 0, "column": i, "utf16_start": i,
                 "utf16_end": 1 if i == 0 else 3,
                 "carrier_byte_offset": 10 + 3*i, "carrier_byte_len": 1,
                 "cell_text_sha256": proof.sha(bytes([97+i])),
                 "boundary_byte_offset": 11 + i*3,
                 "boundary_byte_len": 1, "boundary_sha256": proof.sha(b"\r"),
                 "virtual_cr_before": i != 0}
                for i in range(2)
            ],
        }
        self.txt = b"=== Story table:1 [utf16 0..3] ===\na\rb\n\n"
        self.manifest = {
            "schema_version": proof.SCHEMA,
            "source_sha256": proof.sha(self.source.read_bytes()),
            "source_byte_len": self.source.stat().st_size,
            "extraction_class": proof.CLASS,
            "source_immutable": True,
            "native_pub_repair_attempted": False,
            "native_pub_delivery_allowed": False,
            "page_geometry_reconstructed": False,
            "projected_story_not_semantic": True,
            "story_count": 1,
            "unadmitted_projection_count": 0,
            "stories_txt_sha256": proof.sha(self.txt),
            "stories": [story],
        }
        self.write()

    def write(self):
        (self.target / "stories.json").write_text(
            json.dumps(self.manifest, sort_keys=True), encoding="utf-8"
        )
        (self.target / "stories.txt").write_bytes(self.txt)

    def check(self):
        self.write()
        return proof.verify_private_structure(self.source, self.target)

    def test_valid_private_manifest_structure(self):
        self.assertEqual(self.check()["cell_count"], 2)

    def test_verify_receipt_requires_source_byte_verifier(self):
        receipt = {
            "schema_version": proof.VERIFY_SCHEMA,
            "source_sha256": self.manifest["source_sha256"],
            "verified_table_stories": 1, "verified_source_cells": 2,
            "synthetic_cr_separators": 1, "source_immutable": True,
            "native_pub_repair": False,
        }
        exe = self.root / "pub"
        exe.write_text("binary")
        with patch("subprocess.run", return_value=SimpleNamespace(
                returncode=0, stdout=json.dumps(receipt).encode("utf-8"))):
            self.assertTrue(proof.verify(self.source, self.target, exe)["source_byte_verified"])
        receipt["verified_source_cells"] = 3
        with patch("subprocess.run", return_value=SimpleNamespace(
                returncode=0, stdout=json.dumps(receipt).encode("utf-8"))):
            with self.assertRaises(proof.TableEvidenceError):
                proof.verify(self.source, self.target, exe)

    def test_tamper_original_source_or_txt_is_fatal(self):
        self.source.write_bytes(b"corrupt original")
        with self.assertRaises(proof.TableEvidenceError):
            self.check()
        self.source.write_bytes(b"synthetic-for-structure-only")
        self.check()
        (self.target / "stories.txt").write_bytes(b"tamper")
        with self.assertRaises(proof.TableEvidenceError):
            proof.verify_private_structure(self.source, self.target)

    def test_reject_table_story_payload_tamper(self):
        self.manifest["stories"][0]["text"] = "altered"
        with self.assertRaises(proof.TableEvidenceError):
            self.check()

    def test_reject_false_semantic_promotion(self):
        self.manifest["projected_story_not_semantic"] = False
        with self.assertRaises(proof.TableEvidenceError):
            self.check()
        self.manifest["projected_story_not_semantic"] = True
        self.manifest["stories"][0]["evidence_class"] = "mature_authoritative_exact"
        with self.assertRaises(proof.TableEvidenceError):
            self.check()

    def test_reject_missing_or_extra_cell_field(self):
        self.manifest["stories"][0]["cells"][0]["private_path"] = "customer.pub"
        with self.assertRaises(proof.TableEvidenceError):
            self.check()

    def test_reject_malformed_sha_and_boundary(self):
        self.manifest["stories"][0]["cells"][0]["boundary_sha256"] = "bad"
        with self.assertRaises(proof.TableEvidenceError):
            self.check()

    def test_reject_missing_cr_separator_position(self):
        self.manifest["stories"][0]["cells"][1]["virtual_cr_before"] = False
        with self.assertRaises(proof.TableEvidenceError):
            self.check()

    def test_reject_bad_utf16_end_and_duplicate_key(self):
        self.manifest["stories"][0]["utf16_end"] = 4
        with self.assertRaises(proof.TableEvidenceError):
            self.check()

    def test_reject_unexpected_file_and_symlink(self):
        (self.target / "extra").write_text("x")
        with self.assertRaises(proof.TableEvidenceError):
            proof.verify_private_structure(self.source, self.target)
        (self.target / "extra").unlink()
        link = self.root / "alias"
        link.symlink_to(self.target, target_is_directory=True)
        with self.assertRaises(proof.TableEvidenceError):
            proof.verify_private_structure(self.source, link)


if __name__ == "__main__":
    main()
