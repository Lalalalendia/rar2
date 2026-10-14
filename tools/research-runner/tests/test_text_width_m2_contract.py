#!/usr/bin/env python3
"""Fail-closed static trust contract for bounded Publisher 2019 M2 bisection."""

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PACKET = ROOT / "tools/research-runner/experiments/text-width-breakpoint-m2-01.packet.json"
WORKER = ROOT / "tools/research-runner/operations/text_width_breakpoint_m1_worker_01.ps1"
M2 = ROOT / "tools/research-runner/operations/text_width_breakpoint_m2_01.ps1"
WORKFLOW = ROOT / ".github/workflows/pub-re-native.yml"


class TextWidthM2Contract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.packet = json.loads(PACKET.read_text(encoding="utf-8"))
        cls.worker = WORKER.read_text(encoding="utf-8")
        cls.m2 = M2.read_text(encoding="utf-8")
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_packet_is_pinned_and_not_arbitrary_execution(self):
        self.assertEqual(self.packet["schema"], "pub-research-experiment.v1")
        self.assertEqual(self.packet["id"], "TEXT-WIDTH-BREAKPOINT-M2-01")
        self.assertEqual(self.packet["publisher_environment"], "publisher-2019")
        self.assertTrue(self.packet["requires_publisher"])
        self.assertEqual(
            self.packet["publisher"]["exe_sha256"],
            "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b",
        )
        self.assertEqual(
            self.packet["operation"]["script"],
            "tools/research-runner/operations/text_width_breakpoint_m2_01.ps1",
        )
        self.assertEqual(self.packet["operation"]["args"], [])
        self.assertEqual(self.packet["reset"], {"required": False})
        self.assertIsNone(self.packet["fixture"])

    def test_native_worker_limits_midpoints_and_retains_m1(self):
        for i in range(1, 8):
            self.assertIn(f'"m2-mid-{i}"', self.worker)
        self.assertIn("$WidthPt -le 160.0", self.worker)
        self.assertIn("$WidthPt -ge 172.0", self.worker)
        self.assertIn("$WidthPt * 128.0", self.worker)
        self.assertIn("m2_width_not_bounded_or_quantized", self.worker)
        for arm in ("control-before", "narrow", "wide", "control-after"):
            self.assertIn(f'"{arm}"', self.worker)
        self.assertIn('visible_text_matches_expected = $visibleTextMatches', self.worker)
        self.assertNotIn(".SaveAs(", self.worker)

    def test_study_reuses_known_exact_seed_and_font(self):
        self.assertIn("6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf", self.m2)
        self.assertIn("c9b76220a5be42ead4733611e417cd65c5fd8aeaa33eb56576ac378a37d130a1", self.m2)
        self.assertIn('m2_physical_font_baseline_drift', self.m2)
        self.assertIn('m2_exact_m1_control_not_reproduced', self.m2)
        self.assertIn('m2_exact_m1_positive_not_reproduced', self.m2)
        self.assertIn('m2_earlier_line_break_changed', self.m2)
        self.assertIn('m2_nonbinary_terminal_layout', self.m2)
        self.assertIn('m2_bracketing_controls_disagree', self.m2)

    def test_adaptive_seven_midpoint_bisection_is_bounded(self):
        self.assertIn('for ($index=1; $index -le 7; $index++)', self.m2)
        self.assertIn('$mid = ($low + $high) / 2.0', self.m2)
        self.assertIn('("m2-mid-" + $index)', self.m2)
        self.assertIn('Invoke-M2Arm "control-before" 160.0', self.m2)
        self.assertIn('Invoke-M2Arm "wide" 172.0', self.m2)
        self.assertIn('Invoke-M2Arm "control-after" 160.0', self.m2)
        self.assertIn('m2_fifth_line_outside_dichotomy', self.m2)
        self.assertIn('0.09375', self.m2)
        self.assertIn('$arms.Count -ne 10', self.m2)
        self.assertIn('actual_arm_count = $arms.Count', self.m2)
        self.assertIn('m2_after_save_reopen_or_font_drift', self.m2)

    def test_no_unearned_claim_or_private_upload(self):
        self.assertIn('status = "bounded_fifth_line_breakpoint_only"', self.m2)
        self.assertIn('source_bytes_uploaded = $false', self.m2)
        self.assertIn('carrier_authority_granted = $false', self.m2)
        self.assertIn('product_visual_acceptance_granted = $false', self.m2)
        self.assertIn('text-width-breakpoint-m2-01.json', self.m2)
        self.assertIn('text-width-breakpoint-m2-01.txt', self.m2)
        self.assertNotIn(".SaveAs(", self.m2)

    def test_owner_trust_and_single_serial_native_job(self):
        self.assertIn('text-width-breakpoint-m2-01', self.workflow)
        self.assertIn('research_width_m2', self.workflow)
        self.assertIn("github.actor == 'Lalalalendia'", self.workflow)
        self.assertIn("github.event.comment.author_association == 'OWNER'", self.workflow)
        self.assertIn("needs.gate.outputs.command_mode != 'research_width_m2'", self.workflow)
        self.assertIn("needs.gate.outputs.command_mode == 'research_width_m2'", self.workflow)
        self.assertGreaterEqual(self.workflow.count("group: pub-re-native-publisher-oracle"), 2)
        self.assertNotIn("pull_request:", self.workflow)

    def test_partial_artifact_allowlist_counts_complete_m2(self):
        self.assertIn('"text-width-breakpoint-m2-01.json"', self.workflow)
        self.assertIn('"text-width-m2-suite-stage.json"', self.workflow)
        self.assertIn('foreach ($i in 1..7)', self.workflow)
        self.assertIn('"text-width-m1-m2-mid-$i.json"', self.workflow)
        self.assertIn('"text-width-m1-m2-mid-$i-stage.json"', self.workflow)
        self.assertIn("m1_success_incomplete_receipts", self.workflow)
        self.assertIn("Clear private M1 Publisher sources and outputs", self.workflow)
        self.assertIn("m1_source_safe_receipt_contains_local_path", self.workflow)


if __name__ == "__main__":
    unittest.main()
