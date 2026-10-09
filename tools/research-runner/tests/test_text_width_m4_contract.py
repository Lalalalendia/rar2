#!/usr/bin/env python3
"""Fail-closed M4 native Publisher three-arm CFB attribution and restoration contract."""

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PACKET = ROOT / "tools/research-runner/experiments/text-width-hyphenation-m4-cfb-01.packet.json"
OP = ROOT / "tools/research-runner/operations/text_width_hyphenation_m4_cfb_01.ps1"
M3B = ROOT / "tools/research-runner/operations/text_width_hyphenation_m3b_01.ps1"
WORKER = ROOT / "tools/research-runner/operations/text_width_breakpoint_m1_worker_01.ps1"
WORKFLOW = ROOT / ".github/workflows/pub-re-native.yml"
ANALYZER = ROOT / "tools/pub-re/src/main.rs"


class M4StructuralContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.packet = json.loads(PACKET.read_text(encoding="utf-8"))
        cls.op = OP.read_text(encoding="utf-8")
        cls.m3b = M3B.read_text(encoding="utf-8")
        cls.worker = WORKER.read_text(encoding="utf-8")
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.analyzer = ANALYZER.read_text(encoding="utf-8")

    def test_packet_fixed_operation_and_windows_version(self):
        p = self.packet
        self.assertEqual(p["schema"], "pub-research-experiment.v1")
        self.assertEqual(p["id"], "TEXT-WIDTH-HYPHENATION-M4-CFB-01")
        self.assertEqual(p["publisher_environment"], "publisher-2019")
        self.assertTrue(p["requires_publisher"])
        self.assertEqual(p["publisher"]["exe_sha256"],
            "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b")
        self.assertEqual(p["operation"], {
            "shell": "powershell",
            "script": "tools/research-runner/operations/text_width_hyphenation_m4_cfb_01.ps1",
            "args": [],
        })
        self.assertIsNone(p["fixture"])
        self.assertEqual(p["reset"], {"required": False})
        self.assertIn("analysis/text-width-hyphenation-m4-cfb-01.json", p["evidence"]["required"])
        self.assertIn("analysis/text-width-m4-restoration.json", p["evidence"]["required"])

    def test_no_second_hyphenation_writer_and_trusted_main(self):
        s = self.op
        self.assertIn('"TEXT-WIDTH-HYPHENATION-M3B-01"', self.m3b)
        self.assertIn('M3bOperation = Join-Path $PSScriptRoot', s)
        self.assertIn('& $M3bOperation -PacketPath $M3bPacket', s)
        self.assertIn('m4_m3b_native_or_restore_not_verified', s)
        self.assertIn('$restoration.independent_fresh_process_verified', s)
        self.assertIn('$restoration.quarantine_cleared', s)
        self.assertIn('m4_host_quarantine_present', s)
        self.assertIn('m4_publisher_busy_before_run', s)
        self.assertIn('m4_checkout_not_trusted', s)
        self.assertIn('m4_requires_trusted_main', s)
        self.assertNotIn('.AutoHyphenate =', s)
        self.assertNotIn('Remove-Item -LiteralPath $Marker', s)
        self.assertIn('source_bytes_uploaded = $false', s)

    def test_matched_on1_off_on2_synthetic_policy_control(self):
        s = self.op
        self.assertIn('Run-On2 $source $sourceSha $On2Root', s)
        self.assertIn('M3bSeedWidthPt","162.53125', s)
        self.assertIn('m4_before_after_save_reference_drift', s)
        self.assertIn('m4_reopen_line_signature_drift', s)
        self.assertIn('m4_text_or_stored_width_confounded', s)
        self.assertIn('m4_inset_confounded', s)
        self.assertIn('0:28|28:57|57:86|86:112|112:140|140:155', s)
        self.assertIn('0:28|28:57|57:86|86:107|107:131|131:155', s)
        self.assertIn("c9b76220a5be42ead4733611e417cd65c5fd8aeaa33eb56576ac378a37d130a1", s)
        self.assertIn('6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf', s)
        self.assertIn('m4_m3b_pub_receipt_hash_mismatch', s)
        self.assertIn('m4_on2_pub_receipt_hash_mismatch', s)
        self.assertNotIn(".SaveAs(", s)

    def test_pubre_analyzer_receipt_provenance_and_nondeterminism(self):
        s = self.op
        self.assertIn('cargo build --quiet --manifest-path', s)
        self.assertIn('Write-PrivateComparison "on1-on2"', s)
        self.assertIn('Write-PrivateComparison "on1-off"', s)
        self.assertIn('Write-PrivateComparison "off-on2"', s)
        self.assertIn('max_changed_ranges_per_stream=24', s)
        self.assertIn('m4_pub_re_receipt_not_source_safe', s)
        self.assertIn('raw_document_bytes_emitted', s)
        self.assertIn('raw_stream_bytes_emitted', s)
        self.assertIn('absolute_input_paths_emitted', s)
        self.assertIn('expected_input_hashes_verified', s)
        self.assertIn('m4_source_safe_cfb_receipt_oversize', s)
        self.assertIn('candidate_streams_not_changed_in_on_on_control', s)
        self.assertIn('no_stream_unique_to_treatment_at_cfb_level', s)
        self.assertIn('rule_not_established', s)
        self.assertIn('pub-re-receipt.v1', s)
        self.assertIn('AttributeOfficeart', self.analyzer)
        self.assertIn('changed_streams', s)

    def test_owner_gated_serial_windows_and_upload_allowlist(self):
        w = self.workflow
        self.assertIn('text-width-hyphenation-m4-cfb-01', w)
        self.assertIn('research_width_m4_cfb', w)
        self.assertIn('github.actor == \'Lalalalendia\'', w)
        self.assertIn("github.event.comment.author_association == 'OWNER'", w)
        self.assertGreaterEqual(w.count('group: pub-re-native-publisher-oracle'), 2)
        self.assertIn('dtolnay/rust-toolchain@stable', w)
        self.assertIn('toolchain: "1.94.1"', w)
        for filename in (
            "text-width-hyphenation-m4-cfb-01.json",
            "text-width-m4-suite-stage.json",
            "text-width-m4-restoration.json",
            "text-width-m4-on1.json",
            "text-width-m4-off.json",
            "text-width-m4-on2.json",
            "text-width-m4-cfb-on1-on2.json",
            "text-width-m4-cfb-on1-off.json",
            "text-width-m4-cfb-off-on2.json",
        ):
            self.assertIn(filename, w)
        self.assertIn('m1_source_safe_receipt_contains_local_path', w)
        self.assertIn('Clear private M1 Publisher sources and outputs', w)
        self.assertNotIn('pull_request:', w)


if __name__ == "__main__":
    unittest.main()
