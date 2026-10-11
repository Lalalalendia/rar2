import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "warning_receipt", Path(__file__).with_name("build_parser_warning_contract_receipt.py")
)
receipt = importlib.util.module_from_spec(spec)
spec.loader.exec_module(receipt)


class ReaderReceiptTests(unittest.TestCase):
    def setUp(self):
        self.runtime = {
            "schema": "chaptera.contents-missing-directory-fields.runtime.v1",
            "source_free": True,
            "diagnostics": [
                {"code": code, "seq_num": 1, "source": {"stream": "/Synthetic", "offset": 2, "len": 18}, "parent_seq_num": 23}
                for code in ["missing_raw_type", "missing_chunk_offset"]
            ],
            "present_valid": {"diagnostic_count": 0, "chunk_offset": 0, "fully_decoded": True},
            "present_invalid": {"code": "declared_range_out_of_bounds", "offset": 4294967295, "declared_length": 0, "stream_len": 4},
        }

    def read(self, data):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "runtime.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            return receipt.reader_contract(path)

    def test_actual_contract_can_be_forwarded_without_health_reclassification(self):
        self.assertEqual(self.read(self.runtime), self.runtime)

    def test_rejects_missing_negative_control_and_lost_provenance(self):
        for field in ["diagnostics", "present_valid", "present_invalid", "source_free"]:
            with self.subTest(field=field):
                data = copy.deepcopy(self.runtime)
                del data[field]
                with self.assertRaises(ValueError):
                    self.read(data)
        data = copy.deepcopy(self.runtime)
        data["diagnostics"][1]["source"]["offset"] = -1
        with self.assertRaises(ValueError):
            self.read(data)

    def test_rejects_sentinel_and_automatic_health_verdict(self):
        for key, value in [("chunk_offset", -1), ("health", "damaged")]:
            with self.subTest(key=key):
                data = copy.deepcopy(self.runtime)
                data["diagnostics"][1][key] = value
                with self.assertRaises(ValueError):
                    self.read(data)
        data = copy.deepcopy(self.runtime)
        data["present_invalid"]["code"] = "missing_chunk_offset"
        with self.assertRaises(ValueError):
            self.read(data)


if __name__ == "__main__":
    unittest.main()
