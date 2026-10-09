#!/usr/bin/env python3
"""Static fail-closed guard for the one-scenario Publisher M1 native lane."""

from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PACKET = ROOT / "tools/research-runner/experiments/text-width-breakpoint-m1-01.packet.json"
WORKER = ROOT / "tools/research-runner/operations/text_width_breakpoint_m1_worker_01.ps1"
SUITE = ROOT / "tools/research-runner/operations/text_width_breakpoint_m1_01.ps1"
WORKFLOW = ROOT / ".github/workflows/pub-re-native.yml"
REGISTRY = ROOT / "tools/pub-re/native-fixtures.json"

EXPECTED_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"


class TextWidthM1Contract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.packet = json.loads(PACKET.read_text(encoding="utf-8"))
        cls.registry = json.loads(REGISTRY.read_text(encoding="utf-8"))
        cls.worker = WORKER.read_text(encoding="utf-8")
        cls.suite = SUITE.read_text(encoding="utf-8")
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_explicit_packet_and_exact_publisher(self):
        self.assertEqual(self.packet["id"], "TEXT-WIDTH-BREAKPOINT-M1-01")
        self.assertEqual(self.packet["publisher_environment"], "publisher-2019")
        self.assertEqual(
            self.packet["operation"]["script"],
            "tools/research-runner/operations/text_width_breakpoint_m1_01.ps1",
        )
        self.assertTrue(self.packet["requires_publisher"])
        self.assertEqual(len(self.packet["publisher"]["exe_sha256"]), 64)

    def test_uses_only_known_compatible_public_source(self):
        fixtures = [x for x in self.registry["fixtures"] if x["alias"] == "sample-newsletter"]
        self.assertEqual(len(fixtures), 1)
        self.assertEqual(fixtures[0]["expected_sha256"], EXPECTED_SHA)
        self.assertEqual(fixtures[0]["expected_byte_len"], 291840)
        self.assertIn(EXPECTED_SHA, self.suite)
        self.assertIn("m1_fixture_hash_mismatch", self.suite)
        self.assertIn("m1_requires_trusted_main", self.suite)
        self.assertIn("m1_checkout_not_trusted", self.suite)

    def test_one_semantic_axis_and_matched_controls(self):
        for arm, width in (
            ("control-before", "160.0"),
            ("narrow", "148.0"),
            ("wide", "172.0"),
            ("control-after", "160.0"),
        ):
            self.assertIn(f'"{arm}" = {width}', self.worker)
            self.assertIn(f'Id="{arm}"; Width={width}', self.suite)
        self.assertIn('$frame.AutoFitText = 0', self.worker)
        self.assertIn('FontName = "Arial"', self.worker)
        self.assertIn('$range.Font.Size = $FontSize', self.worker)
        self.assertEqual(self.worker.count('$shape.Width = $WidthPt'), 1)
        self.assertNotIn(".SaveAs(", self.worker)
        self.assertNotIn(".SaveAs(", self.suite)

    def test_individual_processes_and_fresh_reopen(self):
        self.assertIn('Start-Process -FilePath $ps', self.suite)
        self.assertIn('$child.WaitForExit(180000)', self.suite)
        self.assertIn('m1_publisher_still_running_', self.suite)
        self.assertIn('$app2.Open($SeedPath,$true,$false)', self.worker)
        self.assertIn('$app2.Open($armPath,$true,$false)', self.worker)
        self.assertIn('$doc.Save()', self.worker)
        self.assertIn('m1_save_reopen_changed_text', self.worker)
        self.assertIn('m1_bracketing_controls_disagree', self.suite)

    def test_fail_closed_evidence_and_no_claim_before_product_ab(self):
        self.assertIn("source_bytes_uploaded = $false", self.suite)
        self.assertIn("carrier_authority_granted = $false", self.suite)
        self.assertIn("product_visual_acceptance_granted = $false", self.suite)
        self.assertIn("m1_after_save_reopen_drift", self.suite)
        self.assertIn('"inconclusive_no_break_contrast"', self.suite)
        self.assertIn('"width_sensitivity_observed_no_threshold_law"', self.suite)

    def test_trusted_dispatch_shares_native_resource_lock(self):
        self.assertIn("text-width-breakpoint-m1-01", self.workflow)
        self.assertIn("research_width_m1", self.workflow)
        self.assertIn("github.actor == 'Lalalalendia'", self.workflow)
        self.assertIn("github.event.comment.author_association == 'OWNER'", self.workflow)
        self.assertIn("needs.gate.outputs.command_mode == 'research_width_m1'", self.workflow)
        self.assertIn("needs.gate.outputs.command_mode != 'research_width_m1'", self.workflow)
        native_groups = re.findall(r"group: pub-re-native-publisher-oracle", self.workflow)
        self.assertGreaterEqual(len(native_groups), 2)
        self.assertNotIn("pull_request:", self.workflow)

    def test_artifact_allowlist_and_cleanup(self):
        self.assertIn("text-width-m1-suite-stage.json", self.workflow)
        self.assertIn("text-width-breakpoint-m1-01.json", self.workflow)
        self.assertIn("pub-re-native-text-width-m1-evidence", self.workflow)
        self.assertIn("m1_source_safe_receipt_contains_local_path", self.workflow)
        self.assertIn("m1_success_incomplete_receipts", self.workflow)
        self.assertIn("Clear private M1 Publisher sources and outputs", self.workflow)


if __name__ == "__main__":
    unittest.main()
