#!/usr/bin/env python3
"""Independent authored Rectangle ZIP geometry and native application proof.

Only input is a pinned real PUB, a canonical EditorProject replay receipt and
the emitted IDML/ODG files. Fail closed: never substitute synthetic XML.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import uuid
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

FIXTURE_SHA256 = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
SOURCE_BYTES = 291840
AUTHOR_ID = "019a2d80-1112-7001-8001-aabbccddee01"
BOUNDS = {"x": 914400, "y": 914400, "width": 1828800, "height": 914400}
MAX_ZIP = 16 * 1024 * 1024
EMU_PER_POINT = 12700


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def attribute(element: ET.Element, name: str) -> str | None:
    return next((v for k, v in element.attrib.items() if local(k) == name), None)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def check_probe(source: Path, output: Path, receipt: dict) -> None:
    require(source.stat().st_size == SOURCE_BYTES and sha256_file(source) == FIXTURE_SHA256,
            "source PUB is not the pinned original")
    require(receipt.get("schema") == "chaptera.authored-shape-export-probe.v1",
            "wrong canonical probe")
    require(receipt.get("source_sha256") == FIXTURE_SHA256 and
            receipt.get("project_replay") is True, "source/project replay provenance missing")
    require(str(uuid.UUID(receipt.get("node_id", ""))) == AUTHOR_ID,
            "authored NodeId is not deterministic")
    require(receipt.get("bounds") == BOUNDS, "authored bounds are not canonical")
    require(receipt.get("fill_rgb") == [18, 52, 86] and
            receipt.get("stroke_rgb") == [170, 187, 204] and
            receipt.get("stroke_width_emu") == 12700, "authored paint mismatch")
    require((output / "editor-project.json").is_file(),
            "serialized EditorProject not materialized")
    for target in ("idml", "odg"):
        item = receipt[target]
        require(item["bytes"] == (output / ("output." + target)).stat().st_size,
                "report/ZIP size disagreement")
        require(item["blocking"] == 0 and item["unsupported"] >= 1,
                "missing explicit unsupported z-order or unexpected blocking loss")
        require((output / (target + ".report.json")).is_file(),
                "typed ExportReport not materialized")


def package_parts(path: Path) -> dict[str, bytes]:
    require(0 < path.stat().st_size <= MAX_ZIP, "ZIP export oversized or empty")
    with zipfile.ZipFile(path) as package:
        require(package.testzip() is None, "corrupt exported ZIP")
        names = package.namelist()
        require(len(names) == len(set(names)) and len(names) <= 5000,
                "ambiguous ZIP path collision or excessive parts")
        require(sum(item.file_size for item in package.infolist()) <= 64 * 1024 * 1024,
                "ZIP unpacked byte budget exceeded")
        return {name: package.read(name) for name in names}


def check_idml(parts: dict[str, bytes]) -> None:
    self_id = "uar" + uuid.UUID(AUTHOR_ID).hex
    matches = []
    for path, data in parts.items():
        if path.startswith("Spreads/") and path.endswith(".xml"):
            root = ET.fromstring(data)
            matches.extend(el for el in root.iter()
                           if local(el.tag) == "Rectangle" and el.attrib.get("Self") == self_id)
    require(len(matches) == 1, "authored Rectangle absent or duplicated in IDML")
    shape = matches[0]
    require(shape.attrib.get("FillColor") == "Color/Chaptera-123456" and
            shape.attrib.get("StrokeColor") == "Color/Chaptera-AABBCC" and
            shape.attrib.get("StrokeWeight") == "1", "IDML rectangle paint mismatch")
    points = [el.attrib["Anchor"] for el in shape.iter()
              if local(el.tag) == "PathPointType" and "Anchor" in el.attrib]
    require(points == ["72 72", "72 144", "216 144", "216 72"],
            "IDML rectangle anchor geometry mismatch")
    require("Resources/Graphic.xml" in parts and
            b"Chaptera-123456" in parts["Resources/Graphic.xml"] and
            b"Chaptera-AABBCC" in parts["Resources/Graphic.xml"],
            "IDML RGB color resources missing")


def check_odg(parts: dict[str, bytes]) -> None:
    require("content.xml" in parts, "ODG content.xml missing")
    root = ET.fromstring(parts["content.xml"])
    rect_name = "AuthoredRect_" + uuid.UUID(AUTHOR_ID).hex
    rects = [e for e in root.iter()
             if local(e.tag) == "rect" and attribute(e, "name") == rect_name]
    require(len(rects) == 1, "authored ODG rectangle absent or duplicated")
    shape = rects[0]
    for field, value in (("x", "72pt"), ("y", "72pt"),
                         ("width", "144pt"), ("height", "72pt")):
        require(attribute(shape, field) == value, "ODG rectangle " + field + " differs")
    style = "AuthoredRectStyle_" + uuid.UUID(AUTHOR_ID).hex
    require(attribute(shape, "style-name") == style, "ODG authored style binding missing")
    styles = [e for e in root.iter()
              if local(e.tag) == "style" and attribute(e, "name") == style]
    require(len(styles) == 1, "ODG authored graphic style missing")
    properties = [e for e in styles[0] if local(e.tag) == "graphic-properties"]
    require(len(properties) == 1, "ODG graphic properties missing")
    props = properties[0]
    require(attribute(props, "fill") == "solid" and
            attribute(props, "fill-color") == "#123456" and
            attribute(props, "stroke") == "solid" and
            attribute(props, "stroke-color") == "#AABBCC" and
            attribute(props, "stroke-width") == "1pt",
            "ODG authored solid paint differs")


def consume(path: Path, target: str) -> dict:
    for tool in ("scribus", "xvfb-run", "libreoffice"):
        require(shutil.which(tool) is not None, "missing independent consumer: " + tool)
    with tempfile.TemporaryDirectory(prefix="chaptera-authored-consumer-") as tmp:
        root = Path(tmp)
        name = root / ("edited." + target)
        shutil.copyfile(path, name)
        env = os.environ.copy()
        env["HOME"] = str(root)
        if target == "idml":
            saved = root / "reopened.sla"
            env["CHAPTERA_SCRIBUS_SLA_OUT"] = str(saved)
            helper = Path("tools/ci/scribus_idml_consumer.py").resolve()
            require(helper.is_file(), "Scribus import script unavailable")
            command = ["xvfb-run", "-a", "scribus", "-g", "-ns",
                       "-py", str(helper), "--", str(name)]
            consumer = "Scribus"
        else:
            out = root / "libreoffice"
            out.mkdir()
            saved = out / "edited.fodg"
            command = ["libreoffice", "-env:UserInstallation=" + (root / "lo-profile").as_uri(),
                       "--headless", "--convert-to", "fodg", "--outdir", str(out), str(name)]
            consumer = "LibreOffice Draw"
        result = subprocess.run(command, env=env, capture_output=True, timeout=160, check=False)
        require(result.returncode == 0 and saved.is_file() and
                0 < saved.stat().st_size < 48 * 1024 * 1024,
                consumer + " failed to import/save real artifact")
        parsed = ET.parse(saved).getroot()
        kinds = {local(e.tag).lower() for e in parsed.iter()}
        if target == "idml":
            require("pageobject" in kinds, "Scribus SLA contains no page objects")
        else:
            require("rect" in kinds or "custom-shape" in kinds,
                    "LibreOffice imported no drawn rectangle")
        return {"application": consumer, "reopened": True,
                "import_sha256": sha256_file(saved)}


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--directory", type=Path, required=True)
    p.add_argument("--probe-receipt", type=Path, required=True)
    p.add_argument("--xml-only", action="store_true")
    args = p.parse_args()
    data = json.loads(args.probe_receipt.read_text(encoding="utf-8"))
    check_probe(args.source, args.directory, data)
    parts = {}
    for target in ("idml", "odg"):
        path = args.directory / ("output." + target)
        parts[target] = package_parts(path)
    check_idml(parts["idml"])
    check_odg(parts["odg"])
    consumers = {}
    if not args.xml_only:
        for target in ("idml", "odg"):
            consumers[target] = consume(args.directory / ("output." + target), target)
    result = {
        "schema": "chaptera.authored-rectangle-real-consumers.v1",
        "status": "XML_ONLY_PASS" if args.xml_only else "PASS",
        "source_sha256": FIXTURE_SHA256,
        "project_replay": True,
        "authored_node_id": AUTHOR_ID,
        "idml": {"zip_sha256": sha256_file(args.directory / "output.idml"),
                 "exact_geometry": True, "consumer": consumers.get("idml")},
        "odg": {"zip_sha256": sha256_file(args.directory / "output.odg"),
                "exact_geometry": True, "consumer": consumers.get("odg")},
    }
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
