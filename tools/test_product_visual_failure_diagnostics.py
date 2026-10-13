#!/usr/bin/env python3
"""Source-free tests: Product 49/111 error cause must be a fixed public code."""
from __future__ import annotations

from pathlib import Path
import tempfile
import unittest

from product_full_case_census_v1 import FAILURES
from run_product_oidc_worst_census_acceptance import (
    OWNER_HTTP_PHASE_CODES,
    PRODUCT_VISUAL_PHASE_CODES,
    classify_owner_failure,
    classify_visual_failure,
    read_private_fixed_code,
)


class PhysicalFailureDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory(prefix="chaptera-diagnostic-test-")
        self.addCleanup(self.work.cleanup)
        self.sidecar = Path(self.work.name) / "private-phase.txt"

    def write(self, value: str) -> str:
        self.sidecar.write_text(value, encoding="utf-8")
        return str(self.sidecar)

    def test_all_fixed_browser_phase_codes_census_admitted(self):
        self.assertTrue(set(PRODUCT_VISUAL_PHASE_CODES.values()) <= FAILURES)

    def test_all_fixed_http_phase_codes_census_admitted(self):
        self.assertTrue(OWNER_HTTP_PHASE_CODES <= FAILURES)

    def test_source_neutral_render_phase(self):
        code = classify_visual_failure(self.write("after_render\n"))
        self.assertEqual(code, {
            "failure_code": "visual_after_render",
            "current_http_status": None,
            "scene_http_status": None,
            "public_error_code": "redacted",
        })

    def test_source_neutral_raster_phase(self):
        x = classify_visual_failure(self.write("after_raster\n"))
        self.assertEqual(x["failure_code"], "visual_after_raster")

    def test_source_neutral_ablation_phase(self):
        x = classify_visual_failure(self.write("ablation_ready\n"))
        self.assertEqual(x["failure_code"], "visual_image_ablation")

    def test_missing_sidecar_does_not_forge_success(self):
        x = classify_visual_failure(str(self.sidecar))
        self.assertEqual(x["failure_code"], "visual_unclassified")

    def test_unknown_private_data_never_exits(self):
        x = classify_visual_failure(self.write("secret-document-raw-text\n"))
        self.assertEqual(x["failure_code"], "visual_unclassified")
        self.assertNotIn("secret", str(x))

    def test_fake_multiline_phase_is_rejected(self):
        x = classify_visual_failure(self.write("after_raster\nsecret-extra\n"))
        self.assertEqual(x["failure_code"], "visual_unclassified")

    def test_large_marker_is_rejected(self):
        x = classify_visual_failure(self.write("X" * 4096))
        self.assertEqual(x["failure_code"], "visual_unclassified")

    def test_wrong_encoding_is_rejected(self):
        self.sidecar.write_bytes(b"\xff\xff\xff")
        x = classify_visual_failure(str(self.sidecar))
        self.assertEqual(x["failure_code"], "visual_unclassified")

    def test_http_scene_requested_no_response(self):
        p = self.write("http_scene_unanswered\n")
        x = classify_owner_failure(
            "Error: Product owner HTTP response not observed", p)
        self.assertEqual(x["failure_code"], "http_scene_unanswered")
        self.assertEqual(x["public_error_code"], "redacted")

    def test_http_scene_not_requested(self):
        p = self.write("http_scene_unrequested\n")
        x = classify_owner_failure("Product owner HTTP response not observed", p)
        self.assertEqual(x["failure_code"], "http_scene_unrequested")

    def test_http_current_not_requested(self):
        p = self.write("http_current_unrequested\n")
        x = classify_owner_failure("Product owner HTTP response not observed", p)
        self.assertEqual(x["failure_code"], "http_current_unrequested")

    def test_http_both_mixed(self):
        p = self.write("http_both_mixed\n")
        x = classify_owner_failure("Product owner HTTP response not observed", p)
        self.assertEqual(x["failure_code"], "http_both_mixed")

    def test_http_phase_unknown_stays_unclassified(self):
        p = self.write("s3://secret/customer/doc.pub")
        x = classify_owner_failure("Product owner HTTP response not observed", p)
        self.assertEqual(x["failure_code"], "unclassified_browser_failure")
        self.assertNotIn("s3://", str(x))

    def test_marker_cannot_override_successful_other_failure(self):
        p = self.write("http_scene_unanswered\n")
        x = classify_owner_failure("real OIDC browser session not preserved", p)
        self.assertEqual(x["failure_code"], "oidc_session_contract")

    def test_preserve_worker_scene500_diagnostic(self):
        x = classify_owner_failure(
            "UI Product HTTP failed: current=200 scene=500 "
            "fixed_code=product_replay_worker_failed")
        self.assertEqual(x["failure_code"], "product_http_denial")
        self.assertEqual(x["current_http_status"], 200)
        self.assertEqual(x["scene_http_status"], 500)
        self.assertEqual(x["public_error_code"], "product_replay_worker_failed")

    def test_untrusted_http_error_code_redacted(self):
        x = classify_owner_failure(
            "UI Product HTTP failed: current=403 scene=500 "
            "fixed_code=not_an_allowlisted_code")
        self.assertEqual(x["failure_code"], "product_http_denial")
        self.assertEqual(x["public_error_code"], "redacted")

    def test_http_phase_without_fixed_message_cannot_override(self):
        p = self.write("http_both_unanswered\n")
        x = classify_owner_failure("some private worker error", p)
        self.assertEqual(x["failure_code"], "unclassified_browser_failure")

    def test_fixed_sidecar_only_never_run_source_code(self):
        p = self.write('after_raster";print("data")\n')
        val = read_private_fixed_code(
            p, set(PRODUCT_VISUAL_PHASE_CODES), "unclassified")
        self.assertEqual(val, "unclassified")


if __name__ == "__main__":
    unittest.main()
