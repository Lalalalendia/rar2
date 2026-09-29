#!/usr/bin/env python3
"""Build the standalone Reader from exact committed assets, with no bundler."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ("index.html", "reader.css", "reader-app.mjs", "reader-model.mjs", "render-v1.mjs")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def build(output: Path, commit: str | None = None) -> dict:
    if commit is None:
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("release commit must be an exact lowercase Git SHA")
    assets = {name: subprocess.check_output(["git", "show", f"{commit}:apps/cloud-reader/{name}"], cwd=ROOT) for name in ASSETS}
    manifest = {
        "protocol": "chaptera.cloud-reader-static-release.v1",
        "source_commit": commit,
        "scene_protocol": "chaptera.reader-scene.v1",
        "files": {name: {"byte_len": len(data), "sha256": sha256(data)} for name, data in sorted(assets.items())},
    }
    assets["manifest.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    archive = io.BytesIO()
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_STORED) as package:
        for name, data in sorted(assets.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            package.writestr(info, data)
    output.mkdir(parents=True, exist_ok=True)
    site = output / "site"
    site.mkdir(exist_ok=True)
    if any(path.name not in assets or not path.is_file() or path.is_symlink() for path in site.iterdir()):
        raise ValueError("static release directory contains unexpected entries")
    for name, data in assets.items():
        (site / name).write_bytes(data)
    zipped = archive.getvalue()
    (output / "cloud-reader.zip").write_bytes(zipped)
    receipt = {"source_commit": commit, "artifact_sha256": sha256(zipped), "artifact_byte_len": len(zipped), "asset_count": len(ASSETS)}
    (output / "build-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", help="exact source commit; defaults to HEAD")
    parser.add_argument("--output", type=Path, default=ROOT / "target/cloud-reader-release")
    args = parser.parse_args()
    print(json.dumps(build(args.output, args.commit), sort_keys=True))
