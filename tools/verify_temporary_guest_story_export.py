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
import sys
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


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--artifact", required=True, type=pathlib.Path)
    ap.add_argument("--target", choices=("idml", "odg"), required=True)
    ap.add_argument("--expected-text", required=True)
    args = ap.parse_args()
    if not args.expected_text or len(args.expected_text) > 8192 or \
            any(ord(char) < 32 or ord(char) == 127 for char in args.expected_text):
        raise AssertionError("expected Story text is outside the source-neutral proof contract")
    result = verify(args.artifact, args.target, args.expected_text)
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
