#!/usr/bin/env python3
"""Fail-closed M3a native Publisher application hyphenation audit contract."""

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OP = ROOT / "tools/research-runner/operations/text_width_hyphenation_m3_audit_01.ps1"
WORKER = ROOT / "tools/research-runner/operations/text_width_breakpoint_m1_worker_01.ps1"
PACKET = ROOT / "tools/research-runner/experiments/text-width-hyphenation-m3-audit-01.packet.json"
WORKFLOW = ROOT / ".github/workflows/pub-re-native.yml"


class M3ReadonlyContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.op = OP.read_text(encoding="utf-8")
        cls.worker = WORKER.read_text(encoding="utf-8")
        cls.packet = json.loads(PACKET.read_text(encoding="utf-8"))
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_exact_packet_and_trusted_lane(self):
        self.assertEqual(self.packet["id"], "TEXT-WIDTH-HYPHENATION-M3-AUDIT-01")
        self.assertEqual(self.packet["schema"], "pub-research-experiment.v1")
        self.assertEqual(self.packet["reset"], {"required": False})
        self.assertIsNone(self.packet["fixture"])
        self.assertEqual(self.packet["operation"]["args"], [])
        self.assertEqual(self.packet["publisher"]["exe_sha256"],
            "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b")
        self.assertIn('m3_physical_arial_baseline_drift', self.op)
        self.assertIn('m1_requires_trusted_main', self.op)
        self.assertIn('m1_checkout_not_trusted', self.op)
        self.assertIn('m1_publisher_busy_before_run', self.op)

    def test_explicit_one_arm_and_readonly_global_options(self):
        self.assertIn('Run-Child "arm" $seedPath ([string]$seed.seed_sha256) $CurrentArm 162.53125', self.op)
        self.assertIn('"-CaptureHyphenation"', self.op)
        self.assertIn('m3_readonly_arm_not_allowlisted', self.worker)
        self.assertIn('$options = $frame.Application.Options', self.worker)
        self.assertIn('auto_hyphenate = [bool]$options.AutoHyphenate', self.worker)
        self.assertIn('hyphenation_zone_pt = $zonePt', self.worker)
        self.assertIn('options_mutated = $false', self.worker)
        self.assertNotIn('AutoHyphenate =', self.op)
        self.assertNotIn('HyphenationZone =', self.op)
        self.assertNotIn('.SaveAs(', self.op)

    def test_provenance_and_causal_limits(self):
        self.assertIn('m3_app_options_changed_across_readonly_processes', self.op)
        self.assertIn('m3_m2_reference_not_reproduced', self.op)
        self.assertIn('m3_save_reopen_or_source_drift', self.op)
        self.assertIn('story_hyphenation_policy_observed = $false', self.op)
        self.assertIn('causal_hyphenation_effect_claimed = $false', self.op)
        self.assertIn('source_bytes_uploaded = $false', self.op)
        self.assertIn('status = "app_hyphenation_baseline_only_no_causal_claim"', self.op)

    def test_serial_resource_and_source_safe_upload(self):
        self.assertIn('text-width-hyphenation-m3-audit-01', self.workflow)
        self.assertIn('research_width_m3_readonly', self.workflow)
        self.assertIn('github.event.comment.author_association == \'OWNER\'', self.workflow)
        self.assertIn("github.actor == 'Lalalalendia'", self.workflow)
        self.assertIn('text-width-m1-m3-audit-stage.json', self.workflow)
        self.assertIn('text-width-m3-suite-stage.json', self.workflow)
        self.assertIn('m1_source_safe_receipt_contains_local_path', self.workflow)
        self.assertIn('Clear private M1 Publisher sources and outputs', self.workflow)
        self.assertGreaterEqual(self.workflow.count('group: pub-re-native-publisher-oracle'), 2)
        self.assertNotIn('pull_request:', self.workflow)


if __name__ == "__main__":
    unittest.main()
