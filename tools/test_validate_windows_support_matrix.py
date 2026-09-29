import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import validate_windows_support_matrix as subject

MATRIX = ROOT / "packages/product/reader-portable/v1/windows-support-matrix.v1.json"
SCHEMA = ROOT / "packages/product/reader-portable/v1/windows-support-matrix.schema.v1.json"


class WindowsSupportMatrixTests(unittest.TestCase):
    def load_matrix(self):
        return json.loads(MATRIX.read_text(encoding="utf-8"))

    def test_checked_in_matrix_is_semantically_valid(self):
        self.assertEqual(subject.semantic_errors(self.load_matrix()), [])

    def test_windows_10_cannot_be_promoted_to_supported_without_receipt(self):
        matrix = self.load_matrix()
        row = next(cell for cell in matrix["cells"] if cell["os"] == "Windows 10" and cell["version"] == "22H2")
        row["state"] = "supported"
        errors = subject.semantic_errors(matrix)
        self.assertTrue(any("expected excluded" in error for error in errors))
        self.assertTrue(any("receipt_refs" in error for error in errors))

    def test_public_copy_is_matrix_derived(self):
        rendered = subject.render_copy(self.load_matrix())
        self.assertIn("Windows 11 25H2 x64", rendered)
        self.assertIn("CI mechanics evidence only", rendered)
        self.assertIn("100% and 150%", rendered)

    def test_copy_drift_is_rejected(self):
        matrix = self.load_matrix()
        schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            matrix_path = root / "matrix.json"
            schema_path = root / "schema.json"
            readme_path = root / "README.md"
            matrix_path.write_text(json.dumps(matrix), encoding="utf-8")
            schema_path.write_text(json.dumps(schema), encoding="utf-8")
            readme_path.write_text(subject.render_copy(matrix).replace("Windows 11 25H2", "Windows 10"), encoding="utf-8")
            with self.assertRaises(ValueError):
                subject.validate_matrix(matrix_path, schema_path, readme_path)


if __name__ == "__main__":
    unittest.main()
