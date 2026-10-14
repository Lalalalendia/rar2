#!/usr/bin/env python3
"""Independent physical IDML/ODG ResizeNode geometry + target-app reopening.

Input is an actual browser-downloaded ZIP, not Rust/Scene data. The precise
source-owned EMU rectangle is first checked by the established independent
export geometry oracle; then Scribus / LibreOffice Draw must import *the same*
bytes. The emitted receipt never includes document text or private paths.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

from verify_editable_export_geometry import RectEmu, verify_export

MAX_APP_REOPEN_BYTES = 16 * 1024 * 1024


def reimport(artifact: Path, target: str) -> dict:
    if not (shutil.which("scribus") and shutil.which("libreoffice")
            and shutil.which("xvfb-run")):
        raise AssertionError("real Scribus and LibreOffice consumers are required")
    if not 0 < artifact.stat().st_size <= MAX_APP_REOPEN_BYTES:
        raise AssertionError("physical resize export exceeds bounded app input")
    with tempfile.TemporaryDirectory(prefix="chaptera-resize-consumer-") as tmp:
        root = Path(tmp)
        source = root / ("edited." + target)
        shutil.copyfile(artifact, source)
        env = os.environ.copy()
        env["HOME"] = str(root)
        if target == "idml":
            helper = Path("tools/ci/scribus_idml_consumer.py").resolve()
            exported = root / "reimported.sla"
            if not helper.is_file():
                raise AssertionError("pinned Scribus source importer unavailable")
            env["CHAPTERA_SCRIBUS_SLA_OUT"] = str(exported)
            command = [
                "xvfb-run", "-a", "scribus", "-g", "-ns", "-py",
                str(helper), "--", str(source),
            ]
            application = "Scribus"
        else:
            profile = (root / "lo-profile").as_uri()
            directory = root / "libreoffice"
            directory.mkdir()
            exported = directory / "edited.fodg"
            command = [
                "libreoffice", "-env:UserInstallation=" + profile,
                "--headless", "--convert-to", "fodg", "--outdir",
                str(directory), str(source),
            ]
            application = "LibreOffice Draw"
        try:
            run = subprocess.run(
                command, capture_output=True, timeout=160, check=False,
                env=env,
            )
        except subprocess.TimeoutExpired as exc:
            raise AssertionError("independent resize application timed out") from exc
        if (run.returncode != 0 or not exported.is_file()
                or not 0 < exported.stat().st_size <= 48 * 1024 * 1024):
            raise AssertionError("independent resize target application failed to reopen")
        # Both SLA and FODG must be real parseable saved XML, not a process
        # that exited successfully without reading the edited artifact.
        root_xml = ET.parse(exported).getroot()
        if target == "idml":
            if not any(tag.tag.upper().endswith("PAGEOBJECT")
                       for tag in root_xml.iter()):
                raise AssertionError("Scribus import lacked any page object")
        else:
            if not any(str(tag.tag).endswith("}frame")
                       for tag in root_xml.iter()):
                raise AssertionError("LibreOffice Draw import lacked frames")
        digest = hashlib.sha256(exported.read_bytes()).hexdigest()
        return {
            "application": application,
            "reopened": True,
            "serialized_import_sha256": digest,
        }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--target", choices=("idml", "odg"), required=True)
    parser.add_argument("--node-id", required=True)
    parser.add_argument("--x-emu", type=int, required=True)
    parser.add_argument("--y-emu", type=int, required=True)
    parser.add_argument("--width-emu", type=int, required=True)
    parser.add_argument("--height-emu", type=int, required=True)
    args = parser.parse_args()
    rect = RectEmu(args.x_emu, args.y_emu, args.width_emu, args.height_emu)
    if rect.width <= 0 or rect.height <= 0:
        raise AssertionError("ResizeNode created nonpositive bounds")
    geometric = verify_export(args.artifact, args.target, args.node_id, rect)
    if geometric["geometry_matches_edit"] is not True:
        raise AssertionError("independent XML geometry does not match ResizeNode")
    application = reimport(args.artifact, args.target)
    geometric["target_application_consumer"] = application
    geometric["exact_xml_geometry_verified"] = True
    print(json.dumps(geometric, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
