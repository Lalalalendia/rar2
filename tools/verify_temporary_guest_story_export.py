#!/usr/bin/env python3
"""Independent, bounded XML consumer of a browser-downloaded edited Story IDML/ODG.

Only source-neutral proof is printed; extracted user/fixture Story contents and
download bytes never enter CI logs or artifacts.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
import xml.etree.ElementTree as ET

MAX_UNCOMPRESSED = 24 * 1024 * 1024
MAX_MEMBERS = 512
TEXT_NS = "{urn:oasis:names:tc:opendocument:xmlns:text:1.0}p"


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def verify(artifact: pathlib.Path, target: str, expected: str) -> dict:
    raw = artifact.read_bytes()
    if not 0 < len(raw) <= 16 * 1024 * 1024 or not raw.startswith(b"PK\x03\x04"):
        raise AssertionError("physical Story artifact is not a bounded ZIP package")
    found = []
    with zipfile.ZipFile(artifact, "r") as package:
        members = package.infolist()
        if (not members or len(members) > MAX_MEMBERS or
                sum(item.file_size for item in members) > MAX_UNCOMPRESSED):
            raise AssertionError("physical Story archive exceeds bounded consumer limits")
        if any(pathlib.PurePosixPath(item.filename).is_absolute() or
               ".." in pathlib.PurePosixPath(item.filename).parts for item in members):
            raise AssertionError("physical Story archive has unsafe member names")
        if package.testzip() is not None:
            raise AssertionError("physical Story archive has a corrupt CRC")
        names = package.namelist()
        if target == "idml":
            xml_names = [name for name in names
                         if re.fullmatch(r"Stories/Story_[^/]+\.xml", name)]
            if not xml_names or "designmap.xml" not in names:
                raise AssertionError("exported IDML lacks package and imported Story XML")
            for name in xml_names:
                root = ET.fromstring(package.read(name))
                parts = ["".join(element.itertext()) for element in root.iter()
                         if local_name(element.tag) == "Content"]
                found.append("".join(parts))
        elif target == "odg":
            if "content.xml" not in names or "mimetype" not in names:
                raise AssertionError("exported ODG lacks OpenDocument content and mimetype")
            mime = package.read("mimetype")
            if mime != b"application/vnd.oasis.opendocument.graphics":
                raise AssertionError("exported ODG declares a different mimetype")
            root = ET.fromstring(package.read("content.xml"))
            found = ["".join(element.itertext()) for element in root.iter()
                     if element.tag == TEXT_NS]
        else:
            raise AssertionError("unknown Story consumer target")
    if not found or not any(expected in paragraph for paragraph in found):
        raise AssertionError("current-revision Story marker absent from real editable content")
    return {
        "schema": "chaptera.tempedit-independent-story-xml-consumer.v1",
        "target": target,
        "artifact_sha256": hashlib.sha256(raw).hexdigest(),
        "artifact_byte_len": len(raw),
        "physical_zip_crc_valid": True,
        "target_story_xml_parsed": True,
        "edited_text_recovered": True,
        "current_story_sha256": hashlib.sha256(expected.encode("utf-8")).hexdigest(),
        "target_story_units": len(found),
    }




def verify_target_application(artifact: pathlib.Path, target: str, expected: str) -> dict:
    """Reimport *the same downloaded bytes* in independent publishing software."""
    if not (shutil.which("libreoffice") and shutil.which("scribus") and
            shutil.which("xvfb-run")):
        raise AssertionError("independent target consumer applications unavailable")
    with tempfile.TemporaryDirectory(prefix="chaptera-story-app-consumer-") as folder:
        root = pathlib.Path(folder)
        source = root / ("edited." + target)
        shutil.copyfile(artifact, source)
        if target == "idml":
            script = pathlib.Path("tools/ci/scribus_idml_consumer.py").resolve()
            output = root / "reimported.sla"
            if not script.is_file():
                raise AssertionError("pinned Scribus IDML importer script unavailable")
            import os
            env = os.environ.copy()
            env["CHAPTERA_SCRIBUS_SLA_OUT"] = str(output)
            try:
                process = subprocess.run(
                    ["xvfb-run", "-a", "scribus", "-g", "-ns", "-py",
                     str(script), "--", str(source)],
                    capture_output=True, timeout=160, check=False, env=env,
                )
            except subprocess.TimeoutExpired as exc:
                raise AssertionError("Scribus IDML import exceeded bounded timeout") from exc
            if process.returncode != 0 or not output.is_file():
                raise AssertionError("Scribus failed to reopen browser-downloaded IDML")
            reopened = output.read_bytes()
            if expected.encode("utf-8") not in reopened:
                raise AssertionError("Scribus SLA reimport lost the edited canonical Story")
            return {
                "application": "Scribus",
                "reopened": True, "story_marker_retained": True,
                "reopened_sha256": hashlib.sha256(reopened).hexdigest(),
            }
        # LibreOffice Draw's FODG is an independent importer/serializer, not
        # just a second zipfile view of the source archive.
        import os
        outdir = root / "libreoffice"
        outdir.mkdir()
        env = os.environ.copy()
        env["HOME"] = str(root)
        profile_url = (root / "lo-profile").as_uri()
        try:
            process = subprocess.run(
                ["libreoffice", "-env:UserInstallation=" + profile_url,
                 "--headless", "--convert-to", "fodg",
                 "--outdir", str(outdir), str(source)],
                capture_output=True, timeout=150, check=False, env=env,
            )
        except subprocess.TimeoutExpired as exc:
            raise AssertionError("LibreOffice ODG import exceeded bounded timeout") from exc
        reopened_path = outdir / "edited.fodg"
        if process.returncode != 0 or not reopened_path.is_file():
            raise AssertionError("LibreOffice Draw failed to reopen browser-downloaded ODG")
        reopened = reopened_path.read_bytes()
        document = ET.fromstring(reopened)
        paragraphs = ["".join(node.itertext()) for node in document.iter()
                      if node.tag == TEXT_NS]
        if not any(expected in paragraph for paragraph in paragraphs):
            raise AssertionError("LibreOffice Draw FODG reimport lost the edited Story")
        return {
            "application": "LibreOffice Draw",
            "reopened": True, "story_marker_retained": True,
            "reopened_sha256": hashlib.sha256(reopened).hexdigest(),
        }

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--artifact", required=True, type=pathlib.Path)
    ap.add_argument("--target", choices=("idml", "odg"), required=True)
    ap.add_argument("--expected-text", required=True)
    ap.add_argument("--real-application-consumer", action="store_true")
    args = ap.parse_args()
    if not args.expected_text or len(args.expected_text) > 8192 or \
            any(ord(char) < 32 or ord(char) == 127 for char in args.expected_text):
        raise AssertionError("expected Story text is outside the source-neutral proof contract")
    result = verify(args.artifact, args.target, args.expected_text)
    if args.real_application_consumer:
        application = verify_target_application(args.artifact, args.target, args.expected_text)
        if not application["reopened"] or not application["story_marker_retained"]:
            raise AssertionError("target application did not retain the edited Story")
        result["target_application_consumer"] = application
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
