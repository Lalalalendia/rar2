#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PACKAGER = ROOT / "tools/package_local_portable.py"
WEB_FILES = (
    "local-editor.html",
    "editor-shell-v1.mjs",
    "editor-service-client-v1.mjs",
    "observability-v1.mjs",
    "render-v1.mjs",
    "interaction-v1.mjs",
    "export-preview-v1.schema.json",
)


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def make_fixture(root: Path) -> tuple[Path, Path, Path, Path]:
    root.mkdir(parents=True, exist_ok=True)
    chaptera = root / "chaptera.exe"
    producer = root / "chaptera-producer-a.exe"
    chaptera.write_bytes(b"chaptera-web-asset-contract\n")
    producer.write_bytes(b"producer-a-web-asset-contract\n")

    runtime = root / "python"
    runtime.mkdir()
    (runtime / "python.exe").write_bytes(b"embedded-python-fixture\n")
    (runtime / "python313.zip").write_bytes(b"stdlib-fixture\n")
    (runtime / "python313._pth").write_text("python313.zip\n.\n", encoding="utf-8")
    (runtime / "LICENSE.txt").write_text("fixture Python license\n", encoding="utf-8")

    packages = root / "python-packages"
    packages.mkdir()
    (packages / "FROZEN.txt").write_text("fixture-package==1.0\n", encoding="ascii")
    return chaptera, producer, runtime, packages


def build_once(root: Path, name: str) -> tuple[Path, dict]:
    chaptera, producer, runtime, packages = make_fixture(root / (name + "-inputs"))
    out = root / (name + ".zip")
    manifest = root / (name + ".json")
    subprocess.run(
        [
            sys.executable,
            str(PACKAGER),
            "--chaptera-exe", str(chaptera),
            "--producer-exe", str(producer),
            "--python-runtime", str(runtime),
            "--python-packages", str(packages),
            "--output-zip", str(out),
            "--manifest", str(manifest),
            "--source-commit", "asset-contract",
            "--workflow-run", "asset-contract",
        ],
        cwd=ROOT,
        check=True,
    )
    return out, json.loads(manifest.read_text(encoding="utf-8"))


def assert_zip(zip_path: Path, manifest: dict) -> None:
    payload = zip_path.read_bytes()
    assert manifest["zip_sha256"] == sha256_bytes(payload)
    assert manifest["zip_size"] == len(payload)

    with zipfile.ZipFile(zip_path) as archive:
        names = set(archive.namelist())
        for name in WEB_FILES:
            packaged = "apps/web/" + name
            assert packaged in names, packaged
            assert archive.read(packaged) == (ROOT / packaged).read_bytes(), packaged

        sums = archive.read("SHA256SUMS").decode("ascii").splitlines()
        expected = {}
        for row in sums:
            digest, relative = row.split("  ", 1)
            assert relative not in expected
            expected[relative] = digest
        for relative, digest in expected.items():
            assert relative in names, relative
            assert sha256_bytes(archive.read(relative)) == digest, relative

        assert not any(name.lower().endswith(".pub") for name in names)
        assert not any("/test_" in ("/" + name) for name in names)


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="chaptera-local-web-assets-") as temp:
        root = Path(temp)
        first_zip, first_manifest = build_once(root, "first")
        second_zip, second_manifest = build_once(root, "second")
        assert_zip(first_zip, first_manifest)
        assert_zip(second_zip, second_manifest)
        assert first_zip.read_bytes() == second_zip.read_bytes()
        assert first_manifest["zip_sha256"] == second_manifest["zip_sha256"]
        print(
            "local portable web asset package contract: PASS "
            f"entries={first_manifest['entry_count']} sha256={first_manifest['zip_sha256']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
