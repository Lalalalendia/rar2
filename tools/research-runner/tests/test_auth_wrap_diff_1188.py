"""Source-free negative and positive fixtures for AUTH-WRAP-1188 comparison."""
import copy
import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "analysis" / "auth_wrap_diff_1188.py"
SPEC = importlib.util.spec_from_file_location("auth_wrap_diff_1188", SCRIPT)
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


def state(refs, *, fopt=8, source="a", moved=False):
    rect = {"x": 12700 if moved else 0, "y": 0, "width": 127000, "height": 254000}
    return {
        "schema": "chaptera.auth-wrap-oracle-tool.v1",
        "source_sha256": source,
        "shapes": [
            {"page_id": 1, "seq_num": 10, "bounds_emu": rect,
             "officeart_spid": 222, "fopt": [{"property_id": 1, "opid": 1, "op": fopt}]},
            {"page_id": 1, "seq_num": 11, "bounds_emu": {"x": 300000, "y": 0, "width": 1000, "height": 1000},
             "officeart_spid": 223, "fopt": []},
            {"page_id": 1, "seq_num": 12, "bounds_emu": {"x": 500000, "y": 0, "width": 1000, "height": 1000},
             "officeart_spid": 224, "fopt": []},
        ],
        "wrap_frames": [{"page_id": 1, "frame_seq_num": 10, "bounds_emu": rect,
                         "count": len(refs), "refs": [{"target_seq_num": a, "value_offset": i * 4} for i, a in enumerate(refs)]}],
    }


class Tests(unittest.TestCase):
    def test_a_patch_and_publisher_preserve(self):
        result = MOD.compare(state([11]), state([12], source="b"),
                             state([12], source="c"), family="A", frame_seq=10)
        self.assertEqual(result["post_save_0x47_outcome"], "patched_ref_preserved")
        self.assertTrue(result["membership_changed_before_open"])

    def test_a_publisher_restores_baseline(self):
        self.assertEqual(MOD.compare(state([11]), state([12], source="b"),
                         state([11], source="c"), family="A", frame_seq=10)["post_save_0x47_outcome"],
                         "baseline_ref_restored")

    def test_b_unchanged_input_with_regenerated_graph(self):
        result = MOD.compare(state([11]), state([11]), state([12], source="c", fopt=9),
                             family="B", frame_seq=10)
        self.assertEqual(result["post_save_0x47_outcome"], "other_regeneration")
        self.assertTrue(result["membership_changed_after_save"])

    def test_negative_inadvertent_second_reference_change(self):
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(state([11, 12]), state([12, 11], source="b"),
                        state([12, 11], source="c"), family="A", frame_seq=10)

    def test_negative_wrong_prepared_source_in_b(self):
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(state([11]), state([12], source="b"), state([12], source="c"),
                        family="B", frame_seq=10)

    def test_negative_duplicate_refs(self):
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(state([11, 11]), state([12, 11], source="b"),
                        state([12, 11], source="c"), family="A", frame_seq=10)

    def test_negative_geometry_drift(self):
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(state([11]), state([12], source="b"),
                        state([12], source="c", moved=True), family="A", frame_seq=10)

    def test_negative_foreign_fopt_before_open(self):
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(state([11]), state([12], fopt=9, source="b"),
                        state([12], fopt=9, source="c"), family="A", frame_seq=10)

    def test_negative_unknown_frame(self):
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(state([11]), state([12], source="b"),
                        state([12], source="c"), family="A", frame_seq=999)

    def test_negative_shape_type_changed(self):
        initial = state([11]); patched = state([12], source="b")
        patched["shapes"][1]["officeart_shape_type"] = 7
        saved = state([12], source="c")
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(initial, patched, saved, family="A", frame_seq=10)

    def test_negative_text_frame_membership_changed(self):
        initial = state([11]); patched = state([12], source="b")
        saved = state([12], source="c")
        saved["shapes"][2]["is_text_frame"] = True
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(initial, patched, saved, family="A", frame_seq=10)

    def test_negative_frame_disappeared_after_publisher(self):
        initial = state([11]); patched = state([12], source="b")
        saved = state([12], source="c")
        saved["wrap_frames"] = []
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(initial, patched, saved, family="A", frame_seq=10)

    def test_negative_foreign_frame_disappeared_after_publisher(self):
        initial = state([11]); patched = state([12], source="b")
        saved = state([12], source="c")
        foreign = {"page_id": 1, "frame_seq_num": 20, "bounds_emu": {},
                   "count": 1, "refs": [{"target_seq_num": 11}]}
        initial["wrap_frames"].append(foreign)
        patched["wrap_frames"].append(copy.deepcopy(foreign))
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(initial, patched, saved, family="A", frame_seq=10)

    def test_negative_other_frame_premutation(self):
        before = state([11]); before["wrap_frames"].append({
            "page_id": 1, "frame_seq_num": 20, "bounds_emu": {},
            "count": 1, "refs": [{"target_seq_num": 11}]})
        prepared = copy.deepcopy(before)
        prepared["source_sha256"] = "b"
        prepared["wrap_frames"][0]["refs"][0]["target_seq_num"] = 12
        prepared["wrap_frames"][1]["refs"][0]["target_seq_num"] = 12
        saved = copy.deepcopy(prepared); saved["source_sha256"] = "c"
        with self.assertRaises(MOD.UnsafeObservation):
            MOD.compare(before, prepared, saved, family="A", frame_seq=10)


if __name__ == "__main__":
    unittest.main()
