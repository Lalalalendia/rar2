import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch
from batch01_scene_source_links_probe import ERROR_PREFIX, GEOMETRY_PREFIX, LINKS_PREFIX, ERROR_ARM, instrument, observe, safe_diagnostics, valid_source_links


def node_descriptor():
    return {"graph_present": True, "kind": "shape", "source_extent": "both_zero",
            "page_relation": "selected_page", "parent_kind": "page", "direct_parent_selected": True,
            "source_ref_count": 1, "projection_ref_count": 1, "story_frame": False,
            "story_resolved": False, "table": False, "legacy_ole": False, "image_slot": False,
            "officeart_type": "none", "viewer_fill": False, "viewer_line": False,
            "scene_extent": "both_zero", "bounds_equal_source": True}


def links(control=False):
    descriptor = node_descriptor()
    return {"family": "0x2c", "source_document_pages": 1, "source_registry_pages": 1,
            "source_nodes": 2, "scene_nodes": 2, "page_profile_applied": False,
            "bad_scene_nodes": 0 if control else 1,
            "first_bad_scene_node": None if control else descriptor,
            "bad_node_histogram": [] if control else [{**descriptor, "count": 1}],
            "missing_image_placements": 0, "missing_image_resource_nodes": 0,
            "missing_image_histogram": []}


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

    def test_finite_source_links_admit_joint_extents_and_role_counts(self):
        receipt = links()
        self.assertTrue(valid_source_links(receipt))
        result = safe_diagnostics(LINKS_PREFIX + json.dumps(receipt))
        self.assertEqual(result["source_links"]["first_bad_scene_node"]["source_extent"], "both_zero")

    def test_document_fields_and_unbounded_values_rejected(self):
        for key, value in (("node_id", "private-origin"), ("raw_text", "private story")):
            receipt = links()
            receipt["first_bad_scene_node"][key] = value
            self.assertIsNone(safe_diagnostics(LINKS_PREFIX + json.dumps(receipt))["source_links"])
        receipt = links()
        receipt["first_bad_scene_node"]["kind"] = "confidential document label"
        self.assertIsNone(safe_diagnostics(LINKS_PREFIX + json.dumps(receipt))["source_links"])

    def test_histogram_count_mismatch_and_bool_counter_rejected(self):
        receipt = links()
        receipt["bad_node_histogram"][0]["count"] = 2
        self.assertFalse(valid_source_links(receipt))
        receipt = links()
        receipt["source_nodes"] = True
        self.assertFalse(valid_source_links(receipt))

    def test_duplicate_source_links_and_bad_json_are_inconclusive(self):
        marker = LINKS_PREFIX + json.dumps(links())
        self.assertIsNone(safe_diagnostics(marker + "\n" + marker)["source_links"])
        self.assertIsNone(safe_diagnostics(LINKS_PREFIX + "{private text")["source_links"])

    def test_source_drift_fails_closed_and_only_one_failure_arm_changes(self):
        source = "            match from_viewer_geometry_with_fonts(\n" + ERROR_ARM
        patched = instrument(source)
        self.assertIn('Some("reader_scene_projection_failed".to_owned())', patched)
        self.assertNotIn('eprintln!("{error}', patched)
        with self.assertRaises(ValueError):
            instrument(source + ERROR_ARM)
        with self.assertRaises(ValueError):
            instrument("unrelated source")

    def test_relative_output_survives_worker_cwd_change(self):
        with tempfile.TemporaryDirectory(dir=".") as temp:
            root = Path(temp)
            source = root / "producer"
            corpus = root / "corpus"
            corpus.mkdir(parents=True)
            binary = source / "target/scene-projection-probe/debug/chaptera"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"synthetic worker identity")
            worker_source = source / "apps/chaptera-server/src/guest_reader_worker.rs"
            worker_source.parent.mkdir(parents=True)
            worker_source.write_text("synthetic instrumentation identity")
            fixtures = []
            for i, role in enumerate(("control", "target", "target", "target")):
                data = bytes([i + 1])
                sha = hashlib.sha256(data).hexdigest()
                (corpus / (sha + ".pub")).write_bytes(data)
                fixtures.append((str(i), sha, len(data), role, "synthetic"))

            def fake_worker(command, **kwargs):
                out = Path(command[command.index("--output-dir") + 1])
                self.assertTrue(out.is_absolute())
                sha = command[command.index("--expected-sha256") + 1]
                control = sha == fixtures[0][1]
                out.mkdir(parents=True)
                (out / "result.json").write_text(json.dumps({
                    "source_sha256": sha, "source_byte_len": 1,
                    "filesystem_confinement": True,
                    "classification": "partial" if control else "unsupported",
                    "terminal_code": None if control else "reader_scene_projection_failed",
                }))
                stderr = GEOMETRY_PREFIX + "1 2 0 0 0 0"
                stderr += "\n" + LINKS_PREFIX + json.dumps(links(control))
                if not control:
                    stderr += "\n" + ERROR_PREFIX + "node_bounds"
                return CompletedProcess(command, 0, stdout=json.dumps({
                    "status": "success", "exit_code": 0, "timed_out": False,
                    "network_policy": "seccomp_default_deny", "stderr_tail": stderr,
                }))

            with patch("batch01_scene_source_links_probe.FIXTURES", fixtures), \
                    patch("batch01_scene_source_links_probe.subprocess.run", fake_worker), \
                    redirect_stdout(StringIO()):
                result = observe(source, corpus, root / "out/summary.json", "a" * 40)
            self.assertTrue(result["complete"])
            self.assertFalse(list((root / "out/private-worker-results").glob("*/result.json")))


if __name__ == "__main__":
    unittest.main()
