#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "tools" / "research-runner" / "reader1050_offline_discriminator.py"
DIFF = ROOT / "tools" / "corpus" / "cfb_physical_diff.py"

spec = importlib.util.spec_from_file_location("reader1050_offline_discriminator", SCRIPT)
discriminator = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(discriminator)

diff_spec = importlib.util.spec_from_file_location("cfb_physical_diff_test_fixture", DIFF)
cfb_diff = importlib.util.module_from_spec(diff_spec)
assert diff_spec.loader is not None
diff_spec.loader.exec_module(cfb_diff)


def fp(sha: str, logical: str, topology: str = "t") -> dict:
    return {
        "sha256": sha,
        "byte_len": 5632,
        "stream_count": 1,
        "storage_count": 0,
        "streams": [
            {
                "path": "/Data",
                "len": 4096,
                "sha256": logical,
                "size_bucket_log2": 12,
            }
        ],
        "carrier_flags": {
            "contents": False,
            "quill": False,
            "escher": False,
            "escher_delay": False,
        },
        "contents_family": "0x22",
        "contents_serialization_revision": 1,
        "path_fingerprint_sha256": "p" * 64,
        "topology_fingerprint_sha256": topology * 64,
        "size_bucket_fingerprint_sha256": "s" * 64,
        "content_topology_fingerprint_sha256": logical * 64,
    }


class Reader1050OfflineDiscriminatorTests(unittest.TestCase):
    def make_inputs(self, root: Path, selected_sha: str, control_sha: str) -> tuple[Path, Path]:
        reader = root / "reader"
        reader.mkdir()
        frontier = root / "frontier.json"
        frontier.write_text(
            json.dumps(
                {
                    "schema": "chaptera.reader1050-hosted-frontier.v1",
                    "source_reader_run_id": "123",
                    "source_main_sha": "deadbeef",
                    "selected": {"source_sha256": selected_sha},
                }
            ),
            encoding="utf-8",
        )
        (reader / "reader-records.json").write_text(
            json.dumps(
                [
                    {"source_sha256": selected_sha, "opened": False},
                    {"source_sha256": control_sha, "opened": True},
                ]
            ),
            encoding="utf-8",
        )
        (reader / "salvage-acceptance.json").write_text(
            json.dumps(
                {
                    "schema": "chaptera.reader-salvage-1050-acceptance.v1",
                    "rows": [
                        {
                            "source_sha256": selected_sha,
                            "outcome": "unsupported",
                            "reader_route": "legacy",
                            "pub_profile": "legacy",
                        },
                        {
                            "source_sha256": control_sha,
                            "outcome": "normal_open",
                            "reader_route": "legacy",
                            "pub_profile": "legacy",
                        },
                    ],
                }
            ),
            encoding="utf-8",
        )
        return reader, frontier

    def test_forced_trigger_same_witness_wins_before_control_search(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            selected_sha = "0" * 64
            control_sha = "f" * 64
            reader, frontier = self.make_inputs(root, selected_sha, control_sha)

            (reader / "fingerprints.json").write_text(
                json.dumps([fp(selected_sha, "a")]),
                encoding="utf-8",
            )
            (reader / "salvage-acceptance.json").write_text(
                json.dumps(
                    {
                        "schema": "chaptera.reader-salvage-1050-acceptance.v1",
                        "rows": [
                            {
                                "source_sha256": selected_sha,
                                "outcome": "unsupported",
                                "reader_route": "mature_0x2c",
                                "pub_profile": "mature_0x2c_complete",
                                "salvage_eligibility": "awaiting_typed_corruption_evidence",
                                "forced_trigger_probe": {
                                    "eligibility": "eligible_known_publisher_corruption",
                                    "cfb_inventory_available": True,
                                    "contents_family": "0x2c",
                                    "has_surviving_evidence": True,
                                    "subsystems": {
                                        "contents": "readable",
                                        "quill": "readable",
                                        "escher": "readable",
                                        "escher_delay": "absent",
                                    },
                                    "source_modified": False,
                                },
                                "forced_partial_graph": {
                                    "status": "constructed",
                                    "fact_counts": {"text_range": 2},
                                    "gap_count": 2,
                                },
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            payload = discriminator.build_discriminator(
                reader,
                frontier,
                root / "out",
                None,
            )
            self.assertEqual(
                payload["verdict"],
                "salvage_path_reachable_if_typed_corruption_were_proven",
            )
            self.assertEqual(payload["control_relation"], "same_witness_forced_trigger")
            self.assertIsNone(payload["control_sha256"])
            self.assertEqual(
                payload["next_discriminator"]["kind"],
                "typed_corruption_evidence_discovery",
            )
            self.assertIn("pub_cfb_inventory_failure", payload["closed_hypotheses"])
            self.assertIn("partial_source_graph_unbuildable", payload["closed_hypotheses"])

    def test_identical_logical_streams_runs_physical_pair_discriminator(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            selected_bytes = cfb_diff.minimal(0)
            control_bytes = cfb_diff.minimal(1)
            selected_sha = hashlib.sha256(selected_bytes).hexdigest()
            control_sha = hashlib.sha256(control_bytes).hexdigest()

            reader, frontier = self.make_inputs(root, selected_sha, control_sha)
            logical = "a"
            (reader / "fingerprints.json").write_text(
                json.dumps(
                    [
                        fp(selected_sha, logical),
                        fp(control_sha, logical),
                    ]
                ),
                encoding="utf-8",
            )

            corpus = root / "corpus"
            corpus.mkdir()
            (corpus / f"{selected_sha}.pub").write_bytes(selected_bytes)
            (corpus / f"{control_sha}.pub").write_bytes(control_bytes)

            out = root / "out"
            payload = discriminator.build_discriminator(
                reader,
                frontier,
                out,
                corpus,
            )

            self.assertEqual(payload["verdict"], "physical_or_directory_divergence")
            self.assertEqual(payload["control_relation"], "identical_logical_streams")
            self.assertEqual(payload["physical_diff"]["status"], "completed")
            self.assertEqual(payload["physical_diff"]["different_byte_count"], 1)
            self.assertIn("state", payload["physical_diff"]["changed_directory_fields"])
            self.assertEqual(
                payload["next_discriminator"]["kind"],
                "reader_physical_cfb_localization",
            )
            raw = (out / "decision.json").read_text(encoding="utf-8")
            self.assertNotIn(".pub", raw)
            self.assertNotIn(str(corpus), raw)

    def test_identical_topology_closes_container_topology_hypothesis(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            selected_sha = "1" * 64
            control_sha = "2" * 64
            reader, frontier = self.make_inputs(root, selected_sha, control_sha)
            selected = fp(selected_sha, "a")
            control = fp(control_sha, "b")
            selected["content_topology_fingerprint_sha256"] = "a" * 64
            control["content_topology_fingerprint_sha256"] = "b" * 64
            selected["topology_fingerprint_sha256"] = "t" * 64
            control["topology_fingerprint_sha256"] = "t" * 64
            selected["streams"] = [
                {
                    "path": "/Contents",
                    "len": 4096,
                    "sha256": "a" * 64,
                    "size_bucket_log2": 12,
                }
            ]
            control["streams"] = [
                {
                    "path": "/Contents",
                    "len": 4096,
                    "sha256": "b" * 64,
                    "size_bucket_log2": 12,
                }
            ]
            selected["carrier_flags"]["contents"] = True
            control["carrier_flags"]["contents"] = True

            (reader / "fingerprints.json").write_text(
                json.dumps([selected, control]),
                encoding="utf-8",
            )

            payload = discriminator.build_discriminator(
                reader,
                frontier,
                root / "out",
                None,
            )
            self.assertEqual(payload["verdict"], "stream_content_divergence")
            self.assertIn("gross_container_topology", payload["closed_hypotheses"])
            self.assertEqual(
                payload["next_discriminator"]["kind"],
                "carrier_semantic_diff",
            )
            self.assertEqual(
                payload["next_discriminator"]["target_carriers"],
                ["contents"],
            )


if __name__ == "__main__":
    unittest.main()
