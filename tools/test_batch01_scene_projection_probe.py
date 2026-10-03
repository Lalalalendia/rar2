import unittest
from batch01_scene_projection_probe import ERROR_PREFIX, GEOMETRY_PREFIX, ERROR_ARM, instrument, safe_diagnostics


class ProbeContractTests(unittest.TestCase):
    def test_raw_error_and_identifying_paths_never_survive(self):
        raw = "confidential-text /private/customer/source.pub\n" + ERROR_PREFIX + "node_bounds"
        result = safe_diagnostics(raw)
        self.assertEqual(result["projection_reason"], "node_bounds")
        self.assertNotIn("confidential", str(result))
        self.assertNotIn("/private", str(result))

    def test_unknown_and_malformed_markers_are_inconclusive(self):
        for token in ("node_bounds secret", "../secret", "untrusted_reason"):
            self.assertIsNone(safe_diagnostics(ERROR_PREFIX + token)["projection_reason"])
        self.assertIsNone(safe_diagnostics(GEOMETRY_PREFIX + "1 2 3 4 5 -1")["geometry_counts"])

    def test_duplicate_reasons_do_not_create_false_attribution(self):
        result = safe_diagnostics(ERROR_PREFIX + "node_bounds\n" + ERROR_PREFIX + "page_dimensions")
        self.assertIsNone(result["projection_reason"])
        self.assertEqual(result["valid_reason_marker_count"], 2)

    def test_geometry_receipt_contains_counts_only(self):
        result = safe_diagnostics(GEOMETRY_PREFIX + "1 9 2 0 0 0")
        self.assertEqual(result["geometry_counts"]["zero_width_nodes"], 2)

    def test_source_drift_fails_closed_and_only_one_failure_arm_changes(self):
        source = "            match from_viewer_geometry_with_fonts(\n" + ERROR_ARM
        patched = instrument(source)
        self.assertIn('Some("reader_scene_projection_failed".to_owned())', patched)
        self.assertNotIn('eprintln!("{error}', patched)
        with self.assertRaises(ValueError):
            instrument(source + ERROR_ARM)
        with self.assertRaises(ValueError):
            instrument("unrelated source")


if __name__ == "__main__":
    unittest.main()

