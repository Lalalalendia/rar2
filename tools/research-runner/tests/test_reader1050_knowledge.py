#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "tools" / "research-runner" / "reader1050_knowledge.py"

spec = importlib.util.spec_from_file_location("reader1050_knowledge", SCRIPT)
knowledge = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(knowledge)


class Reader1050KnowledgeTests(unittest.TestCase):
    def test_registry_and_ledger_are_source_safe_and_linked(self) -> None:
        summary = knowledge.validate_knowledge_bridge()
        self.assertEqual(summary["registry_entries"], 5)
        self.assertEqual(summary["typed_corruption_entries"], 1)
        self.assertEqual(summary["format_owner_entries"], 4)
        self.assertEqual(summary["ledger_entries"], 1)

        opnhous = knowledge.evidence_for_sha(
            "227961e2fba4a6fb814aa2da47e79b19d55ff04e87e49d9c5ceef8d07ce36d0e"
        )
        self.assertIsNotNone(opnhous)
        self.assertEqual(opnhous["kind"], "typed_corruption_evidence")
        self.assertEqual(
            opnhous["corruption_evidence"],
            "publisher97_malformed_or_stale_media_variant",
        )
        self.assertEqual(
            opnhous["authority"]["control_sha256"],
            "0c74bed1b862f4603a77567f817ad22bf1f7c42eb5afbee0c907732953534b5c",
        )

        history = knowledge.ledger_for_sha(opnhous["source_sha256"])
        self.assertEqual(len(history), 1)
        self.assertEqual(history[0]["status"], "superseded")

    def test_runtime_cursor_loads_source_free_proposed_entries(self) -> None:
        import json
        import tempfile

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            nested = root / "run-123" / "artifact"
            nested.mkdir(parents=True)
            (nested / "ledger-entry.proposed.json").write_text(
                json.dumps(
                    {
                        "schema": "chaptera.reader1050-discriminator-ledger-entry.v1",
                        "source_sha256": "f" * 64,
                        "source_reader_run_id": "123",
                        "discriminator_run_id": "456",
                        "source_main_sha": "deadbeef",
                        "discriminator_kind": "same_witness_forced_trigger",
                        "verdict": "x",
                        "decision": "continue_offline",
                        "status": "executed",
                        "next_discriminator": "typed_corruption_evidence_discovery",
                    }
                ),
                encoding="utf-8",
            )
            rows = knowledge.load_runtime_discriminator_cursor(root)
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["source_sha256"], "f" * 64)
            self.assertEqual(rows[0]["status"], "executed")
            self.assertEqual(rows[0]["runtime_cursor_source"], "run-123/artifact/ledger-entry.proposed.json")


if __name__ == "__main__":
    unittest.main()
