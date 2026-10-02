#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "tools" / "research-runner" / "reader1050_frontier.py"
DIFF = ROOT / "tools" / "corpus" / "cfb_physical_diff.py"

spec = importlib.util.spec_from_file_location("reader1050_frontier", SCRIPT)
frontier = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(frontier)

diff_spec = importlib.util.spec_from_file_location("cfb_physical_diff", DIFF)
cfb_diff = importlib.util.module_from_spec(diff_spec)
assert diff_spec.loader is not None
diff_spec.loader.exec_module(cfb_diff)


class Reader1050FrontierTests(unittest.TestCase):
    def test_known_owned_format_gap_is_deprioritized(self) -> None:
        owned = {
            "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
            "salvage_eligibility": "awaiting_typed_corruption_evidence",
            "has_surviving_evidence": False,
            "cfb_inventory_available": False,
            "contents_family": None,
            "open_error_signature_sha256": "a" * 64,
        }
        unowned = {
            **owned,
            "source_sha256": "f" * 64,
        }
        owned_score, owned_reasons = frontier.priority(owned)
        unowned_score, _ = frontier.priority(unowned)
        self.assertLess(owned_score, unowned_score)
        self.assertTrue(any("already owned by" in item for item in owned_reasons))
        gap, _ = frontier.suggested_discriminator(owned)
        self.assertEqual(gap, "existing_format_owner")

    def test_selects_bounded_highest_priority_unsupported_case(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            reader = root / "reader"
            corpus = root / "corpus"
            out = root / "out"
            reader.mkdir()
            corpus.mkdir()

            a = cfb_diff.minimal(0)
            b = cfb_diff.minimal(1)
            sha_a = hashlib.sha256(a).hexdigest()
            sha_b = hashlib.sha256(b).hexdigest()
            (corpus / f"{sha_a}.pub").write_bytes(a)
            (corpus / f"{sha_b}.pub").write_bytes(b)

            salvage = {
                "schema": "chaptera.reader-salvage-1050-acceptance.v1",
                "rows": [
                    {
                        "source_sha256": sha_a,
                        "outcome": "unsupported",
                        "reader_route": "legacy",
                        "pub_profile": "legacy",
                        "salvage_eligibility": "awaiting_typed_corruption_evidence",
                        "corruption_evidence": None,
                        "has_surviving_evidence": True,
                        "cfb_inventory_available": True,
                        "contents_family": "0x22",
                        "open_error_signature_sha256": "a" * 64,
                    },
                    {
                        "source_sha256": sha_b,
                        "outcome": "unsupported",
                        "reader_route": "modern",
                        "pub_profile": "modern",
                        "salvage_eligibility": "awaiting_typed_corruption_evidence",
                        "corruption_evidence": None,
                        "has_surviving_evidence": False,
                        "cfb_inventory_available": True,
                        "contents_family": "0x2c",
                        "open_error_signature_sha256": "b" * 64,
                    },
                ],
            }
            (reader / "salvage-acceptance.json").write_text(
                json.dumps(salvage), encoding="utf-8"
            )
            records = [
                {
                    "source_sha256": sha_a,
                    "opened": False,
                    "open_error_signature_sha256": "a" * 64,
                },
                {
                    "source_sha256": sha_b,
                    "opened": False,
                    "open_error_signature_sha256": "b" * 64,
                },
            ]
            (reader / "reader-records.json").write_text(
                json.dumps(records), encoding="utf-8"
            )

            payload = frontier.build_frontier(
                reader,
                corpus,
                out,
                source_run_id="123",
                source_sha="deadbeef",
            )

            self.assertEqual(payload["unsupported_count"], 2)
            self.assertEqual(payload["status"], "frontier_selected")
            self.assertEqual(payload["selected"]["source_sha256"], sha_a)
            self.assertEqual(
                payload["selected"]["gap_class"],
                "typed_corruption_evidence_gap",
            )
            self.assertEqual(len(payload["queue"]), 2)
            self.assertTrue((out / "frontier.json").is_file())
            self.assertTrue((out / "frontier.md").is_file())
            self.assertTrue((out / "cases" / f"{sha_a}.json").is_file())

            raw = (out / "frontier.json").read_text(encoding="utf-8")
            self.assertNotIn(".pub", raw)
            self.assertNotIn(str(corpus), raw)


if __name__ == "__main__":
    unittest.main()
