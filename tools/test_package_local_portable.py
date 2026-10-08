#!/usr/bin/env python3
from __future__ import annotations

import tempfile
from pathlib import Path
import unittest
import zipfile

from package_local_portable import ROOT, WEB_FILES, sha256_file, stage_package, write_zip


EXPECTED_WEB_FILES = (
    "local-editor.html",
    "editor-shell-v1.mjs",
    "editor-service-client-v1.mjs",
    "observability-v1.mjs",
    "render-v1.mjs",
    "interaction-v1.mjs",
    "export-preview-v1.schema.json",
)


class LocalPortablePackageTests(unittest.TestCase):
    def test_embedded_web_assets_stage_exact_bytes_and_hash_membership(self) -> None:
        self.assertEqual(WEB_FILES, EXPECTED_WEB_FILES)

        with tempfile.TemporaryDirectory(prefix="chaptera-local-package-test-") as temp:
            root = Path(temp)
            chaptera = root / "chaptera.exe"
            producer = root / "chaptera-producer-a.exe"
            chaptera.write_bytes(b"synthetic chaptera executable")
            producer.write_bytes(b"synthetic producer executable")

            runtime = root / "python-runtime"
            runtime.mkdir()
            (runtime / "python.exe").write_bytes(b"synthetic python executable")
            (runtime / "python313.zip").write_bytes(b"synthetic stdlib zip")
            (runtime / "python313._pth").write_text(
                "python313.zip\n.\n",
                encoding="utf-8",
            )
            (runtime / "LICENSE.txt").write_text(
                "synthetic CPython license fixture\n",
                encoding="utf-8",
            )

            packages = root / "python-packages"
            packages.mkdir()
            (packages / "FROZEN.txt").write_text(
                "synthetic-package==1.0\n",
                encoding="ascii",
            )

            stage = root / "stage"
            stage.mkdir()
            build = stage_package(
                stage,
                chaptera_exe=chaptera,
                producer_exe=producer,
                python_runtime=runtime,
                python_packages=packages,
                source_commit="synthetic-head",
                workflow_run="synthetic-run",
            )

            self.assertEqual(build["chaptera_sha256"], sha256_file(chaptera))
            self.assertEqual(build["producer_sha256"], sha256_file(producer))

            sums = (stage / "SHA256SUMS").read_text(encoding="ascii").splitlines()
            summed_paths = {line.split("  ", 1)[1] for line in sums}
            for name in EXPECTED_WEB_FILES:
                source = ROOT / "apps/web" / name
                staged = stage / "apps/web" / name
                self.assertTrue(staged.is_file(), name)
                self.assertEqual(staged.read_bytes(), source.read_bytes(), name)
                self.assertEqual(sha256_file(staged), sha256_file(source), name)
                self.assertIn("apps/web/" + name, summed_paths)

            archive = root / "Chaptera-Local-Windows-x86_64.zip"
            write_zip(stage, archive)
            with zipfile.ZipFile(archive, "r") as zipped:
                names = set(zipped.namelist())
            for name in EXPECTED_WEB_FILES:
                self.assertIn("apps/web/" + name, names)


if __name__ == "__main__":
    unittest.main()
