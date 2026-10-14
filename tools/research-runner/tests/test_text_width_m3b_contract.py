#!/usr/bin/env python3
"""Fail-closed Publisher 2019 M3b preference transaction and source safety."""

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PACKET = ROOT / "tools/research-runner/experiments/text-width-hyphenation-m3b-01.packet.json"
WORKER = ROOT / "tools/research-runner/operations/text_width_breakpoint_m1_worker_01.ps1"
SUPERVISOR = ROOT / "tools/research-runner/operations/text_width_hyphenation_m3b_01.ps1"
RECOVERY = ROOT / "tools/research-runner/operations/text_width_hyphenation_m3b_recovery_01.ps1"
RUNTIME = ROOT / "tools/windows/pub-runtime/PubRuntime.psm1"
PREPARE = ROOT / "tools/research-runner/prepare_native_run.ps1"
WORKFLOW = ROOT / ".github/workflows/pub-re-native.yml"


class M3bContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.packet = json.loads(PACKET.read_text(encoding="utf-8"))
        cls.worker = WORKER.read_text(encoding="utf-8")
        cls.supervisor = SUPERVISOR.read_text(encoding="utf-8")
        cls.recovery = RECOVERY.read_text(encoding="utf-8")
        cls.runtime = RUNTIME.read_text(encoding="utf-8")
        cls.prepare = PREPARE.read_text(encoding="utf-8")
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_pinned_packet(self):
        self.assertEqual(self.packet["schema"], "pub-research-experiment.v1")
        self.assertEqual(self.packet["id"], "TEXT-WIDTH-HYPHENATION-M3B-01")
        self.assertEqual(self.packet["publisher_environment"], "publisher-2019")
        self.assertTrue(self.packet["requires_publisher"])
        self.assertEqual(self.packet["publisher"]["exe_sha256"],
            "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b")
        self.assertEqual(self.packet["operation"]["script"],
            "tools/research-runner/operations/text_width_hyphenation_m3b_01.ps1")
        self.assertEqual(self.packet["operation"]["args"], [])
        self.assertEqual(self.packet["reset"], {"required": False})
        self.assertIsNone(self.packet["fixture"])

    def test_publisher_quarantine_is_persistent_and_fail_closed(self):
        marker = ".chaptera-publisher-hyphenation-quarantine"
        for source in (self.runtime, self.prepare, self.worker, self.supervisor, self.recovery):
            self.assertIn(marker, source)
        self.assertIn("-not $AllowHyphenationRecovery", self.runtime)
        self.assertIn("publisher_hyphenation_quarantined", self.runtime)
        self.assertIn("publisher_hyphenation_quarantined", self.prepare)
        self.assertIn('if (Test-Path -LiteralPath $Marker -PathType Leaf)', self.supervisor)
        self.assertIn('"TEXT-WIDTH-HYPHENATION-M3B-01" | Set-Content -LiteralPath $Marker', self.supervisor)
        self.assertIn("quarantine_cleared = $false", self.supervisor)
        self.assertIn("Remove-Item -LiteralPath $Marker -Force -ErrorAction Stop", self.supervisor)
        self.assertNotIn("Remove-Item -LiteralPath $Marker", self.worker)

    def test_worker_changes_one_setting_only_after_preflight(self):
        self.assertIn("[switch]$M3bOffDuringCreation", self.worker)
        self.assertIn("[double]$M3bSeedWidthPt = 160.0", self.worker)
        self.assertIn('m3b_seed_width_not_allowlisted', self.worker)
        self.assertIn('m3b_capture_required_for_intervention', self.worker)
        self.assertIn('m3b_preference_initial_not_pinned', self.worker)
        self.assertIn('$options.AutoHyphenate = $false', self.worker)
        self.assertIn('$restoreOptions.AutoHyphenate = $true', self.worker)
        self.assertIn('m3b_preference_restore_failed', self.worker)
        self.assertIn('Close-Document $doc', self.worker)
        self.assertIn('snapshot_after_save', self.worker)
        self.assertNotIn(".SaveAs(", self.worker)

    def test_new_story_on_off_control_and_independent_restore(self):
        self.assertIn('Run-Child "on" $source', self.supervisor)
        self.assertIn('Run-Child "off" $source', self.supervisor)
        self.assertIn('M3bSeedWidthPt', self.supervisor)
        self.assertIn('m3b_on_direct_creation_not_m2_upper_edge', self.supervisor)
        self.assertIn('0:28|28:57|57:86|86:112|112:140|140:155', self.supervisor)
        self.assertIn('Restore-And-Verify', self.supervisor)
        self.assertIn('} finally {\n        $CurrentPhase = "restoration"', self.supervisor)
        self.assertIn('Run-Child "verify"', self.supervisor)
        self.assertIn('Run-Child "restore"', self.supervisor)
        self.assertIn('independent_restoration_verified', self.supervisor)
        self.assertIn('m3b_uncertain_publisher_ownership_quarantine', self.supervisor)
        self.assertIn('m3b_checkout_not_trusted', self.supervisor)
        self.assertIn('m3b_physical_font_drift', self.supervisor)

    def test_rescue_only_restores_measured_m3a_baseline(self):
        self.assertIn('[ValidateSet("verify","restore")]', self.recovery)
        self.assertIn('New-PubPublisherApplication -AllowHyphenationRecovery', self.recovery)
        self.assertIn('$options.AutoHyphenate = $true', self.recovery)
        self.assertIn('$options.HyphenationZone = 18.0', self.recovery)
        self.assertIn('m3b_independent_restore_verification_failed', self.recovery)
        self.assertNotIn('scriptblock', self.recovery.lower())

    def test_one_existing_serial_job_and_source_safe_receipts(self):
        self.assertIn('text-width-hyphenation-m3b-01', self.workflow)
        self.assertIn("research_width_m3b", self.workflow)
        self.assertIn("github.actor == 'Lalalalendia'", self.workflow)
        self.assertIn("github.event.comment.author_association == 'OWNER'", self.workflow)
        self.assertGreaterEqual(self.workflow.count("group: pub-re-native-publisher-oracle"), 2)
        for required in ("text-width-hyphenation-m3b-01.json",
                         "text-width-m3b-suite-stage.json",
                         "text-width-m3b-restoration.json",
                         "text-width-m3b-on-stage.json",
                         "text-width-m3b-off-stage.json"):
            self.assertIn(required, self.workflow)
        self.assertIn("m1_source_safe_receipt_contains_local_path", self.workflow)
        self.assertIn("Clear private M1 Publisher sources and outputs", self.workflow)
        self.assertNotIn("pull_request:", self.workflow)
        self.assertIn('source_bytes_uploaded = $false', self.supervisor)
        self.assertIn('product_visual_acceptance_granted = $false', self.supervisor)
        self.assertNotIn(".SaveAs(", self.supervisor)


if __name__ == "__main__":
    unittest.main()
