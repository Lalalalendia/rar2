from __future__ import annotations

import copy
import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("paragraph_structural", ROOT / "analysis" / "paragraph_metrics_auth_01_structural.py")
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


def safe(value):
    return {"state": "value", "value": value}


class MatrixTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)
        self.native_path = self.root / "analysis" / "paragraph-metrics-auth-01.json"
        self.native_path.parent.mkdir()
        self.snapshots = {}
        paragraph = {key: safe(0) for key in module.METRICS}
        frame = {key: safe(0) for key in module.FRAME}
        frame.update(width=safe(360), height=safe(180), text_length=safe(3), overflowing=safe(False))
        arms = []
        for index, (name, (kind, value)) in enumerate(module.SPECS.items()):
            payload = f"synthetic-cfb-{index}".encode()
            out = self.write_pub(name, payload)
            arms.append({
                "arm": name, "mutation": {"kind": kind, "requested_value": value},
                "before_mutation": copy.deepcopy(paragraph),
                "after_mutation": copy.deepcopy(paragraph),
                "fresh_reopen": copy.deepcopy(paragraph),
                "frame_before_mutation": copy.deepcopy(frame),
                "frame_after_mutation": copy.deepcopy(frame),
                "frame_fresh_reopen": copy.deepcopy(frame),
                "output": out,
            })
        self.native = {
            "schema": "chaptera.paragraph-metrics-auth-01.native.v1",
            "experiment_id": module.EXPERIMENT,
            "verdict": "native-semantic-arms-captured-with-common-seed",
            "seed": self.write_pub("seed", b"synthetic-seed"), "arms": arms,
        }
        self.write_native()

    def write_pub(self, name, payload):
        directory = self.root / "private" / "paragraph-metrics-auth-01" / name
        directory.mkdir(parents=True)
        path = directory / ("seed.pub" if name == "seed" else "output.pub")
        path.write_bytes(payload)
        raw = 2_438_401 if name == "line-exact-24pt" else 0
        prop = {
            "field_id": 0x234, "wire_type": 0x20, "raw_tag": [0x34, 0x22],
            "byte_len": 6, "sha256": module.digest(raw.to_bytes(4, "little")), "raw_value": raw,
        }
        self.snapshots[str(path)] = {
            "schema": module.SNAPSHOT_SCHEMA, "authority": "raw_observation_only",
            "source": {"sha256": module.digest(payload), "byte_len": len(payload)},
            "text": {"sha256": module.digest(b"A\x00\r\x00B\x00"), "byte_len": 6},
            "stories": [{"index": 0, "syid": 7, "utf16_len": 3}],
            "chunks": [{"name": "FDPP", "descriptor_ordinal": 3, "byte_len": 24, "sha256": prop["sha256"]}],
            "fdpp_styles": [{
                "descriptor_ordinal": 3, "style_ordinal": 0, "start_utf16": 0, "end_utf16": 3,
                "sha256": prop["sha256"], "properties": [prop],
            }],
        }
        return {"sha256": module.digest(payload), "size": len(payload)}

    def write_native(self):
        self.native_path.write_text(json.dumps(self.native), encoding="utf-8")

    def observation(self, name):
        return next(v for key, v in self.snapshots.items() if Path(key).parent.name == name)

    def run_matrix(self):
        return module.analyze(self.root, lambda path: copy.deepcopy(self.snapshots[str(path)]))

    def test_complete_matrix_is_raw_evidence_without_semantic_grant_or_paths(self):
        receipt = self.run_matrix()
        self.assertTrue(receipt["invariants"]["quill_text_byte_invariance_all_arms"])
        self.assertFalse(receipt["invariants"]["paragraph_metric_semantics_granted"])
        self.assertEqual(len(receipt["arms"]), 11)
        exact = next(a for a in receipt["arms"] if a["arm"] == "line-exact-24pt")
        self.assertEqual(exact["raw_fdpp_0x34"][0]["raw_value"], 2_438_401)
        self.assertEqual(exact["raw_fdpp_0x34"][0]["bit0"], 1)
        self.assertNotIn(str(self.root), json.dumps(receipt))
        self.assertNotIn("absolute_points", json.dumps(receipt))

    def test_equal_length_text_mutation_is_rejected(self):
        self.observation("line-exact-18pt")["text"]["sha256"] = module.digest(b"C\x00\r\x00B\x00")
        with self.assertRaisesRegex(module.StructuralError, "TEXT bytes changed"):
            self.run_matrix()

    def test_missing_or_duplicate_arm_is_rejected(self):
        original = copy.deepcopy(self.native)
        self.native["arms"].pop()
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "eleven-arm"):
            self.run_matrix()
        self.native = original
        self.native["arms"][-1] = copy.deepcopy(self.native["arms"][0])
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "duplicate native arm"):
            self.run_matrix()

    def test_wrong_mutation_or_noncausal_native_revision_is_rejected(self):
        self.native["arms"][0]["mutation"]["kind"] = "line-exact"
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "wrong mutation"):
            self.run_matrix()
        self.native["verdict"] = "old-independent-seeds"
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "causal-seed"):
            self.run_matrix()

    def test_tampered_private_file_and_unbound_snapshot_are_rejected(self):
        path = next(Path(k) for k in self.snapshots if Path(k).parent.name == "line-single")
        original = path.read_bytes()
        path.write_bytes(b"tampered")
        with self.assertRaisesRegex(module.StructuralError, "artifact identity mismatch"):
            self.run_matrix()
        path.write_bytes(original)
        self.observation("line-single")["source"]["sha256"] = "f" * 64
        with self.assertRaisesRegex(module.StructuralError, "snapshot source identity mismatch"):
            self.run_matrix()

    def test_all_missing_com_values_cannot_be_an_identical_baseline(self):
        for arm in self.native["arms"]:
            arm["before_mutation"] = None
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "missing COM snapshot"):
            self.run_matrix()

    def test_unavailable_or_different_baseline_is_rejected(self):
        self.native["arms"][2]["before_mutation"]["line_spacing"] = {"state": "unavailable"}
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "line_spacing unavailable"):
            self.run_matrix()
        self.native["arms"][2]["before_mutation"]["line_spacing"] = safe(1)
        self.write_native()
        with self.assertRaisesRegex(module.StructuralError, "paragraph baselines differ"):
            self.run_matrix()

    def test_nonclosing_fdpp_and_raw_tag_mismatch_are_rejected(self):
        snap = self.observation("line-single")
        snap["fdpp_styles"][0]["end_utf16"] = 2
        with self.assertRaisesRegex(module.StructuralError, "incomplete FDPP"):
            self.run_matrix()
        snap["fdpp_styles"][0]["end_utf16"] = 3
        snap["fdpp_styles"][0]["properties"][0]["raw_tag"] = [0x34, 0x20]
        with self.assertRaisesRegex(module.StructuralError, "raw tag mismatch"):
            self.run_matrix()

    def test_unchanged_text_with_changed_story_identity_is_rejected(self):
        self.observation("line-single")["stories"][0]["syid"] = 8
        with self.assertRaisesRegex(module.StructuralError, "Story partition changed"):
            self.run_matrix()

    def test_absent_or_opaque_candidate_does_not_create_a_default_law(self):
        self.observation("line-single")["fdpp_styles"][0]["properties"] = []
        self.observation("line-1p5")["fdpp_styles"][0]["properties"][0]["raw_value"] = None
        receipt = self.run_matrix()
        single = next(a for a in receipt["arms"] if a["arm"] == "line-single")
        opaque = next(a for a in receipt["arms"] if a["arm"] == "line-1p5")
        self.assertEqual(single["raw_fdpp_0x34"], [])
        self.assertIsNone(opaque["raw_fdpp_0x34"][0]["bit0"])
        self.assertFalse(receipt["invariants"]["paragraph_metric_semantics_granted"])

    def test_stsh_delta_and_reopen_normalization_remain_visible(self):
        self.observation("line-single")["chunks"].append({
            "name": "STSH", "descriptor_ordinal": 4, "byte_len": 8, "sha256": "a" * 64,
        })
        self.native["arms"][1]["fresh_reopen"]["line_spacing"] = safe(1.1)
        self.write_native()
        receipt = self.run_matrix()
        single = next(a for a in receipt["arms"] if a["arm"] == "line-single")
        self.assertEqual(single["control_to_arm_chunk_changes"][0]["name"], "STSH")
        self.assertFalse(single["metrics_unchanged_across_save_reopen"])

    def test_unexpected_text_or_chunk_payload_cannot_leak_to_summary(self):
        self.observation("line-single")["text"]["recovered_text"] = "private text"
        with self.assertRaisesRegex(module.StructuralError, "unexpected identity payload"):
            self.run_matrix()
        del self.observation("line-single")["text"]["recovered_text"]
        self.observation("line-single")["chunks"][0]["private_path"] = "secret.pub"
        with self.assertRaisesRegex(module.StructuralError, "unexpected chunk payload"):
            self.run_matrix()

    def test_inflight_file_change_is_rejected(self):
        def reader(path):
            snapshot = copy.deepcopy(self.snapshots[str(path)])
            if path.parent.name == "line-single":
                path.write_bytes(b"changed while observing")
            return snapshot
        with self.assertRaisesRegex(module.StructuralError, "input changed during snapshot"):
            module.analyze(self.root, reader)

    def test_failed_cli_rerun_removes_earlier_success_receipt(self):
        output = self.native_path.with_name("paragraph-metrics-auth-01-structural.json")
        output.write_text('{"old_success": true}', encoding="utf-8")
        self.native["arms"].pop()
        self.write_native()
        args = ["structural", "--output-root", str(self.root), "--snapshot-tool", "unused"]
        with patch.object(module.sys, "argv", args), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(module.main(), 1)
        self.assertFalse(output.exists())

    def test_cli_writes_only_current_validated_receipt(self):
        output = self.native_path.with_name("paragraph-metrics-auth-01-structural.json")
        output.write_text('{"old_success": true}', encoding="utf-8")
        args = ["structural", "--output-root", str(self.root), "--snapshot-tool", "unused"]
        reader = lambda tool, path: copy.deepcopy(self.snapshots[str(path)])
        with patch.object(module.sys, "argv", args), patch.object(module, "run_probe", reader), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(module.main(), 0)
        receipt = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(receipt["schema"], module.SCHEMA)
        self.assertNotIn("old_success", receipt)
        self.assertEqual(receipt["native_receipt_sha256"], module.digest(self.native_path.read_bytes()))


if __name__ == "__main__":
    unittest.main()
