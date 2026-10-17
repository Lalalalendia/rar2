"""Pure manifest tests for the physical Windows window probe."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from tools.probe_rescue_gui_window_windows import REQUIRED_EXES, validate_package


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "_internal").mkdir()
        rows = []
        for name in REQUIRED_EXES:
            payload = ("synthetic " + name).encode()
            (self.root / name).write_bytes(payload)
            rows.append({"file": name, "sha256": hashlib.sha256(payload).hexdigest(),
                         "byte_len": len(payload)})
        self.manifest = {"schema_version": "chaptera.rescue-portable-build.v1",
                         "source_commit": "a" * 40, "target": "windows-x64",
                         "signing": "unsigned_preview", "files": rows}
        self.save()

    def save(self):
        (self.root / "portable-build.json").write_text(json.dumps(self.manifest))

    def test_verifies_all_three_executables(self):
        self.assertEqual(validate_package(self.root)["source_commit"], "a" * 40)

    def test_rejects_tampered_binary(self):
        (self.root / "chaptera-rescue.exe").write_bytes(b"tampered")
        with self.assertRaisesRegex(AssertionError, "integrity mismatch"):
            validate_package(self.root)

    def test_rejects_missing_binary(self):
        (self.root / "rescue-engine.exe").unlink()
        with self.assertRaisesRegex(AssertionError, "component absent"):
            validate_package(self.root)

    def test_rejects_duplicate_inventory_row(self):
        self.manifest["files"].append(self.manifest["files"][0].copy())
        self.save()
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            validate_package(self.root)

    def test_rejects_path_traversal_even_when_file_exists(self):
        self.manifest["files"].append({"file": "../unexpected", "sha256": "a" * 64,
                                       "byte_len": 0})
        self.save()
        with self.assertRaisesRegex(AssertionError, "unsafe portable inventory path"):
            validate_package(self.root)

    def test_rejects_unlisted_file(self):
        (self.root / "unlisted.dll").write_bytes(b"unexpected")
        with self.assertRaisesRegex(AssertionError, "archive contents"):
            validate_package(self.root)

    def test_rejects_unexpected_signing_status(self):
        self.manifest["signing"] = "signed"
        self.save()
        with self.assertRaisesRegex(AssertionError, "unexpected portable"):
            validate_package(self.root)


if __name__ == "__main__":
    unittest.main()
