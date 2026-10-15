#!/usr/bin/env python3
"""Independent physical IDML/ODG authored Rectangle proof from real Guest browser bytes.

The expected identity/geometry/paint come from the actual canonical server
CreateShape ACK, never from a fabricated XML fixture or browser renderer.
Scribus / LibreOffice must reopen and reserialize the downloaded artifact.
"""
from __future__ import annotations

import argparse
import json
import math
import pathlib
import sys
import uuid
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools" / "ci"))
from verify_authored_rectangle_export import (  # noqa: E402
    attribute, consume, local, package_parts, sha256_file,
)

EMU_PER_POINT = 12_700
EXPECTED_FILL = "#123456"
EXPECTED_STROKE = "#AABBCC"


def require(value: bool, message: str) -> None:
    if not value:
        raise AssertionError(message)


def point(text: str) -> float:
    if not text.endswith("pt"):
        raise AssertionError("physical ODG geometry must have point units")
    return float(text[:-2])


def near(actual: float, expected: float, label: str) -> None:
    require(math.isfinite(actual) and abs(actual - expected) <= .02,
            label + " differs from canonical authored EMUs")


def verify_idml(parts: dict[str, bytes], node_hex: str, bounds: dict) -> None:
    rectangles = []
    for path, data in parts.items():
        if path.startswith("Spreads/") and path.endswith(".xml"):
            rectangles.extend(
                node for node in ET.fromstring(data).iter()
                if local(node.tag) == "Rectangle" and node.attrib.get("Self") == "uar" + node_hex
            )
    require(len(rectangles) == 1, "IDML exactly-one authored Rectangle identity violated")
    shape = rectangles[0]
    require(shape.attrib.get("FillColor") == "Color/Chaptera-123456" and
            shape.attrib.get("StrokeColor") == "Color/Chaptera-AABBCC" and
            shape.attrib.get("StrokeWeight") == "1",
            "IDML explicit authored fill/stroke lost")
    actual = [
        v.attrib.get("Anchor") for v in shape.iter()
        if local(v.tag) == "PathPointType" and v.attrib.get("Anchor")
    ]
    require(len(actual) == 4, "IDML authored rectangle lost four anchors")
    x, y = bounds["x"]/EMU_PER_POINT, bounds["y"]/EMU_PER_POINT
    w, h = bounds["width"]/EMU_PER_POINT, bounds["height"]/EMU_PER_POINT
    expected = [(x,y),(x,y+h),(x+w,y+h),(x+w,y)]
    for position, target in zip(actual, expected):
        coords = list(map(float, position.split()))
        require(len(coords) == 2, "IDML anchor is not two-dimensional")
        near(coords[0],target[0],"IDML X")
        near(coords[1],target[1],"IDML Y")
    require("Resources/Graphic.xml" in parts and
            b"Chaptera-123456" in parts["Resources/Graphic.xml"] and
            b"Chaptera-AABBCC" in parts["Resources/Graphic.xml"],
            "IDML authored RGB palette is absent")


def verify_odg(parts: dict[str, bytes], node_hex: str, bounds: dict) -> None:
    require("content.xml" in parts, "ODG lacks content.xml")
    root = ET.fromstring(parts["content.xml"])
    rectangles = [
        node for node in root.iter()
        if local(node.tag) == "rect" and attribute(node,"name") == "AuthoredRect_" + node_hex
    ]
    require(len(rectangles) == 1, "ODG exactly-one authored Rectangle identity violated")
    rect = rectangles[0]
    for key in ("x", "y", "width", "height"):
        near(point(attribute(rect, key) or ""),bounds[key]/EMU_PER_POINT,"ODG "+key)
    style_name = "AuthoredRectStyle_" + node_hex
    require(attribute(rect,"style-name") == style_name, "ODG authored style binding absent")
    styles = [node for node in root.iter()
              if local(node.tag) == "style" and attribute(node,"name") == style_name]
    require(len(styles) == 1, "ODG duplicate/missing authored graphic style")
    graphics = [node for node in styles[0] if local(node.tag) == "graphic-properties"]
    require(len(graphics) == 1, "ODG authored graphic style has no properties")
    props = graphics[0]
    require(attribute(props,"fill") == "solid" and
            attribute(props,"fill-color") == EXPECTED_FILL and
            attribute(props,"stroke") == "solid" and
            attribute(props,"stroke-color") == EXPECTED_STROKE and
            attribute(props,"stroke-width") == "1pt",
            "ODG authored fill, RGB, or stroke width differs")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", required=True, type=pathlib.Path)
    parser.add_argument("--target", required=True, choices=("idml","odg"))
    parser.add_argument("--node-id", required=True)
    parser.add_argument("--bounds-json", required=True)
    args = parser.parse_args()
    identifier = uuid.UUID(args.node_id)
    require(identifier.version == 7, "server must mint a UUIDv7 authored NodeId")
    bounds = json.loads(args.bounds_json)
    require(isinstance(bounds,dict) and set(bounds) == {"x","y","width","height"}
            and all(type(n) is int for n in bounds.values())
            and bounds["x"] >= 0 and bounds["y"] >= 0
            and bounds["width"] > 0 and bounds["height"] > 0,
            "canonical authored bounds invalid")
    parts = package_parts(args.artifact)
    if args.target == "idml":
        verify_idml(parts,identifier.hex,bounds)
    else:
        verify_odg(parts,identifier.hex,bounds)
    consumer = consume(args.artifact,args.target)
    require(consumer["reopened"] and
            consumer["application"] == ("Scribus" if args.target == "idml" else "LibreOffice Draw"),
            "independent native app did not open the browser-downloaded artifact")
    print(json.dumps({
        "schema":"chaptera.guest-created-shape-independent-consumer.v1",
        "status":"PASS","target":args.target,
        "artifact_sha256":sha256_file(args.artifact),
        "node_id":str(identifier),
        "geometry_exact_within_point_tolerance":True,
        "explicit_paint_verified":True,
        "consumer":consumer,
    },sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
