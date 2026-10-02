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


if __name__ == "__main__":
    unittest.main()
