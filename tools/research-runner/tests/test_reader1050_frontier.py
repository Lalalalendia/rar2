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
    def test_known_owned_format_gap_is_resolved_and_not_selectable(self) -> None:
        owned = {
            "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
            "salvage_eligibility": "awaiting_typed_corruption_evidence",
            "has_surviving_evidence": False,
            "cfb_inventory_available": False,
            "contents_family": None,
            "open_error_signature_sha256": "a" * 64,
        }
        evidence = {
            "owner": "QUILL-STORY-EARLY-TEXT-BOUNDARY-01",
            "evidence_class": "format_gap",
            "disposition": "existing_format_owner",
        }
        resolution = frontier.research_resolution(
            owned["source_sha256"],
            evidence,
            [],
        )
        self.assertIsNotNone(resolution)
        self.assertFalse(resolution["selectable"])
        owned_score, owned_reasons = frontier.priority(owned, resolution)
        unowned_score, _ = frontier.priority(owned)
        self.assertLess(owned_score, unowned_score)
        self.assertTrue(any("resolved by" in item for item in owned_reasons))
        gap, _ = frontier.suggested_discriminator(owned, resolution)
        self.assertEqual(gap, "existing_format_owner")

    def test_typed_corruption_resolution_requires_matching_evidence_digest(self) -> None:
        sha = "2" * 64
        evidence = {
            "owner": "PUB-T-650",
            "evidence_class": "typed_corruption",
            "authority_receipt": {
                "evidence_digest": "sha256:authority",
                "classification": "malformed-or-stale-publisher97-media-variant",
            },
            "disposition": "existing_typed_corruption_evidence",
        }
        stale = [{
            "source_sha256": sha,
            "discriminator": "typed_corruption_evidence_discovery",
            "evidence_digest": "sha256:stale",
            "state": "closed",
        }]
        self.assertIsNone(frontier.research_resolution(sha, evidence, stale))

        current = [{
            "source_sha256": sha,
            "discriminator": "typed_corruption_evidence_discovery",
            "evidence_digest": "sha256:authority",
            "state": "closed",
        }]
        resolution = frontier.research_resolution(sha, evidence, current)
        self.assertIsNotNone(resolution)
        self.assertEqual(
            resolution["kind"],
            "existing_typed_corruption_evidence",
        )
        self.assertFalse(resolution["selectable"])

    def test_forced_probe_becomes_effective_frontier_state(self) -> None:
        row = {
            "salvage_eligibility": "awaiting_typed_corruption_evidence",
            "cfb_inventory_available": False,
            "contents_family": None,
            "has_surviving_evidence": False,
            "forced_trigger_probe": {
                "cfb_inventory_available": True,
                "contents_family": "0x2c",
                "has_surviving_evidence": True,
            },
            "forced_partial_graph": {
                "status": "constructed",
                "gap_count": 2,
            },
        }
        effective = frontier.effective_reader_state(row)
        self.assertTrue(effective["cfb_inventory_available"])
        self.assertEqual(effective["contents_family"], "0x2c")
        self.assertTrue(effective["has_surviving_evidence"])
        gap, _ = frontier.suggested_discriminator(row)
        self.assertEqual(gap, "typed_corruption_evidence_gap")

    def test_closed_evidence_case_is_skipped_for_next_unresolved(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            reader = root / "reader"
            corpus = root / "corpus"
            out = root / "out"
            reader.mkdir()
            corpus.mkdir()

            closed_bytes = cfb_diff.minimal(0)
            next_bytes = cfb_diff.minimal(1)
            closed_sha = hashlib.sha256(closed_bytes).hexdigest()
            next_sha = hashlib.sha256(next_bytes).hexdigest()
            (corpus / f"{closed_sha}.pub").write_bytes(closed_bytes)
            (corpus / f"{next_sha}.pub").write_bytes(next_bytes)

            def unsupported(sha: str) -> dict:
                return {
                    "source_sha256": sha,
                    "outcome": "unsupported",
                    "reader_route": "legacy",
                    "pub_profile": "legacy",
                    "salvage_eligibility": "awaiting_typed_corruption_evidence",
                    "corruption_evidence": None,
                    "has_surviving_evidence": False,
                    "cfb_inventory_available": False,
                    "contents_family": None,
                    "open_error_signature_sha256": "a" * 64,
                    "forced_trigger_probe": {
                        "eligibility": "eligible_known_publisher_corruption",
                        "cfb_inventory_available": True,
                        "contents_family": "0x22",
                        "has_surviving_evidence": True,
                        "source_modified": False,
                    },
                    "forced_partial_graph": {
                        "status": "constructed",
                        "fact_counts": {},
                        "gap_count": 1,
                    },
                }

            (reader / "salvage-acceptance.json").write_text(
                json.dumps({
                    "schema": "chaptera.reader-salvage-1050-acceptance.v1",
                    "rows": [unsupported(closed_sha), unsupported(next_sha)],
                }),
                encoding="utf-8",
            )
            (reader / "reader-records.json").write_text(
                json.dumps([
                    {"source_sha256": closed_sha, "opened": False},
                    {"source_sha256": next_sha, "opened": False},
                ]),
                encoding="utf-8",
            )

            evidence = root / "evidence.json"
            evidence.write_text(
                json.dumps({
                    "schema": "chaptera.reader1050-evidence-registry.v1",
                    "entries": [{
                        "source_sha256": closed_sha,
                        "owner": "PUB-T-650",
                        "evidence_class": "typed_corruption",
                        "authority_receipt": {
                            "evidence_digest": "sha256:authority",
                            "classification": "malformed",
                        },
                        "disposition": "existing_typed_corruption_evidence",
                    }],
                }),
                encoding="utf-8",
            )
            ledger = root / "ledger.json"
            ledger.write_text(
                json.dumps({
                    "schema": "chaptera.reader1050-research-ledger.v1",
                    "entries": [{
                        "source_sha256": closed_sha,
                        "discriminator": "typed_corruption_evidence_discovery",
                        "evidence_digest": "sha256:authority",
                        "state": "closed",
                    }],
                }),
                encoding="utf-8",
            )

            payload = frontier.build_frontier(
                reader,
                corpus,
                out,
                source_run_id="123",
                source_sha="deadbeef",
                evidence_registry_path=evidence,
                research_ledger_path=ledger,
            )

            self.assertEqual(payload["unsupported_count"], 2)
            self.assertEqual(payload["resolved_count"], 1)
            self.assertEqual(payload["unresolved_count"], 1)
            self.assertEqual(payload["selected"]["source_sha256"], next_sha)
            closed = next(
                row for row in payload["queue"]
                if row["source_sha256"] == closed_sha
            )
            self.assertFalse(closed["selectable"])
            self.assertEqual(
                closed["research_resolution"]["kind"],
                "existing_typed_corruption_evidence",
            )
            self.assertEqual(
                payload["selected"]["gap_class"],
                "typed_corruption_evidence_gap",
            )

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
