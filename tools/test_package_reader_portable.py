import hashlib
import json
import pathlib
import tempfile
import unittest
import zipfile

from tools.package_reader_portable import package_reader


class ReaderPortablePackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.exe = self.root / "reader.exe"
        self.exe.write_bytes(b"MZ" + b"chaptera-reader" * 32)
        self.readme = self.root / "README.md"
        self.readme.write_text(
            "# Reader\nContract: chaptera.reader-portable-readme.v1\n",
            encoding="utf-8",
        )
        self.runtime = self.root / "runtime"
        self.runtime.mkdir()
        worker = b"MZ" + b"worker" * 32
        host = b"MZ" + b"host" * 32
        (self.runtime / "chaptera-desktop-open-worker.exe").write_bytes(worker)
        (self.runtime / "chaptera-desktop-open-sandbox-host.exe").write_bytes(host)
        receipt = {
            "schema_version": "chaptera.desktop-open-runtime-stage.v1",
            "worker": {
                "file_name": "chaptera-desktop-open-worker.exe",
                "sha256": hashlib.sha256(worker).hexdigest(),
                "byte_len": len(worker),
            },
            "sandbox_host": {
                "file_name": "chaptera-desktop-open-sandbox-host.exe",
                "sha256": hashlib.sha256(host).hexdigest(),
                "byte_len": len(host),
            },
        }
        (self.runtime / "chaptera-desktop-open-runtime.json").write_text(
            json.dumps(receipt),
            encoding="utf-8",
        )

    def tearDown(self):
        self.tmp.cleanup()

    def test_package_is_product_qualified_and_deterministic(self):
        first = self.root / "first.zip"
        second = self.root / "second.zip"
        one = package_reader(self.exe, first, runtime_dir=self.runtime, readme=self.readme)
        two = package_reader(self.exe, second, runtime_dir=self.runtime, readme=self.readme)
        self.assertEqual(one["product_id"], "chaptera.reader")
        self.assertEqual(one["binary_entry"], "Chaptera-Reader.exe")
        self.assertEqual(one["zip_sha256"], two["zip_sha256"])
        with zipfile.ZipFile(first) as archive:
            self.assertEqual(
                sorted(archive.namelist()),
                sorted(
                    [
                        "Chaptera-Reader.exe",
                        "README.md",
                        "chaptera-desktop-open-worker.exe",
                        "chaptera-desktop-open-sandbox-host.exe",
                        "chaptera-desktop-open-runtime.json",
                    ]
                ),
            )

    def test_generic_entry_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "Chaptera-Reader.exe"):
            package_reader(
                self.exe,
                self.root / "bad.zip",
                runtime_dir=self.runtime,
                readme=self.readme,
                binary_entry="Chaptera.exe",
            )

    def test_non_pe_is_rejected(self):
        self.exe.write_bytes(b"not-pe")
        with self.assertRaisesRegex(RuntimeError, "Windows PE"):
            package_reader(
                self.exe,
                self.root / "bad.zip",
                runtime_dir=self.runtime,
                readme=self.readme,
            )

    def test_runtime_receipt_mismatch_is_rejected(self):
        receipt = json.loads(
            (self.runtime / "chaptera-desktop-open-runtime.json").read_text(encoding="utf-8")
        )
        receipt["worker"]["sha256"] = "0" * 64
        (self.runtime / "chaptera-desktop-open-runtime.json").write_text(
            json.dumps(receipt),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(RuntimeError, "hash mismatch"):
            package_reader(
                self.exe,
                self.root / "bad.zip",
                runtime_dir=self.runtime,
                readme=self.readme,
            )


if __name__ == "__main__":
    unittest.main()
