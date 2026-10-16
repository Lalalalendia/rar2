#!/usr/bin/env python3
"""Source-neutral, exact-EMU physical export oracle for Guest authored Rectangle MoveNodes.

Checks one real, Chromium-downloaded IDML or ODG ZIP against a bounded witness
constructed from canonical Current.project.operations and an independently pinned
three-rectangle geometry expectation. No browser-rendered geometry is trusted.

For full native-document reopening, --native reuses the repository's existing
Scribus / LibreOffice consumer. That consumer proves whole-document reopen,
NOT individual preservation of all three authored shapes after reserialization.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import uuid
import xml.etree.ElementTree as ET
import zipfile
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP, localcontext
from pathlib import Path

EMU_PER_POINT = 12_700
MAX_ZIP = 16 * 1024 * 1024
MAX_UNPACKED = 64 * 1024 * 1024
MAX_PARTS = 5000
MAX_OPS = 128
MAX_EMU = 9_007_199_254_740_991  # Rust MAX_SAFE_EMU_V1; JS-safe too
SCHEMA = "chaptera.temporary-editor-move-nodes-export-witness.v1"
PROOF_SCHEMA = "chaptera.temporary-editor-move-nodes-physical-geometry.v1"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
REVISION = re.compile(r"^sha256:[0-9a-f]{64}$")


def require(condition: bool, code: str) -> None:
    if not condition:
        raise AssertionError(code)


def canonical_uuid(value: str, *, version7: bool = False) -> uuid.UUID:
    try:
        parsed = uuid.UUID(value)
    except (AttributeError, TypeError, ValueError):
        raise AssertionError("invalid_canonical_uuid") from None
    require(str(parsed) == value, "noncanonical_uuid")
    require(not version7 or parsed.version == 7, "expected_authored_uuidv7")
    return parsed


def object_of(value: object, keys: set[str] | None = None) -> dict:
    require(type(value) is dict, "expected_object")
    if keys is not None:
        require(set(value) == keys, "unexpected_object_fields")
    return value


def integer(value: object, *, positive: bool = False) -> int:
    require(type(value) is int, "expected_integer")
    require(-MAX_EMU <= value <= MAX_EMU, "integer_out_of_bounds")
    require(not positive or value > 0, "expected_positive_integer")
    return value


def rectangle(value: object, *, page_size: dict | None = None) -> dict:
    rect = object_of(value, {"x", "y", "width", "height"})
    result = {field: integer(rect[field], positive=(field in ("width", "height")))
              for field in ("x", "y", "width", "height")}
    require(result["x"] >= 0 and result["y"] >= 0, "negative_rectangle_origin")
    require(result["x"] + result["width"] <= MAX_EMU and
            result["y"] + result["height"] <= MAX_EMU,
            "rectangle_overflow")
    if page_size is not None:
        require(result["x"] + result["width"] <= page_size["width"] and
                result["y"] + result["height"] <= page_size["height"],
                "rectangle_outside_page")
    return result


def rgb(value: object) -> tuple[int, int, int]:
    item = object_of(value, {"r", "g", "b"})
    numbers = tuple(item[k] for k in ("r", "g", "b"))
    require(all(type(n) is int and 0 <= n <= 255 for n in numbers), "invalid_rgb")
    return numbers


def rgb_hex(value: tuple[int, int, int]) -> str:
    return "".join(f"{n:02X}" for n in value)


def authored_provenance(value: object) -> bool:
    return value == {"kind": "author_created"}


def paint_profile(value: object) -> dict:
    paint = object_of(value)
    require(authored_provenance(paint.get("provenance")), "paint_provenance_invalid")
    fill = object_of(paint.get("fill"))
    stroke = object_of(paint.get("stroke"))
    require(type(fill.get("visible")) is bool and type(stroke.get("visible")) is bool,
            "paint_visibility_invalid")
    fill_rgb = rgb(fill.get("color"))
    stroke_rgb = rgb(stroke.get("color"))
    stroke_width = integer(stroke.get("width_emu"), positive=True)
    return {"fill": fill["visible"], "fill_rgb": fill_rgb,
            "stroke": stroke["visible"], "stroke_rgb": stroke_rgb,
            "stroke_width_emu": stroke_width}


def verified_witness(value: object) -> dict:
    data = object_of(value, {"schema", "source_sha256", "revision_id", "page_id",
                             "page_size", "selected_node_ids", "operations",
                             "expected_bounds", "expected_moved_node_ids"})
    require(data["schema"] == SCHEMA, "witness_schema_mismatch")
    require(type(data["source_sha256"]) is str and
            SHA256.fullmatch(data["source_sha256"]) is not None, "source_hash_invalid")
    require(type(data["revision_id"]) is str and
            REVISION.fullmatch(data["revision_id"]) is not None, "revision_id_invalid")
    page_id = str(canonical_uuid(data["page_id"]))
    page_size = object_of(data["page_size"], {"width", "height"})
    page_size = {key: integer(page_size[key], positive=True)
                 for key in ("width", "height")}
    selected = data["selected_node_ids"]
    require(type(selected) is list and len(selected) == 3, "selection_must_have_three")
    selected = [str(canonical_uuid(node, version7=True)) for node in selected]
    require(len(set(selected)) == 3, "duplicate_selected_node")
    require(type(data["operations"]) is list and 3 <= len(data["operations"]) <= MAX_OPS,
            "operation_history_length_invalid")

    original: dict[str, dict] = {}
    current: dict[str, dict] = {}
    profiles: dict[str, dict] = {}
    moves = 0
    # Deliberately bounded to this task's physical fixture: exactly three
    # CreateShape operations and zero or one canonical MoveNodes operation.
    for index, raw in enumerate(data["operations"]):
        operation = object_of(raw)
        kind = operation.get("kind")
        if kind == "create_shape":
            require(moves == 0, "create_after_move_not_in_fixture")
            node_id = str(canonical_uuid(operation.get("node_id"), version7=True))
            require(node_id not in current, "duplicate_created_node")
            require(operation.get("page_id") == page_id and
                    operation.get("parent_id") == page_id and
                    operation.get("shape_kind") == "rectangle" and
                    authored_provenance(operation.get("provenance")),
                    "create_shape_identity_mismatch")
            require(operation.get("transform") == {"kind": "identity"},
                    "create_shape_transform_not_identity")
            before = rectangle(operation.get("bounds"), page_size=page_size)
            original[node_id] = before.copy()
            current[node_id] = before.copy()
            profiles[node_id] = paint_profile(operation.get("paint"))
        elif kind == "move_nodes":
            require(len(original) == 3 and index == len(data["operations"]) - 1 and
                    moves == 0 and operation.get("page_id") == page_id,
                    "move_nodes_sequence_invalid")
            entries = operation.get("entries")
            require(type(entries) is list and 1 <= len(entries) <= 3,
                    "move_entries_invalid")
            ids = [entry.get("node_id") for entry in entries if type(entry) is dict]
            require(len(ids) == len(entries) and ids == sorted(set(ids)),
                    "move_entries_not_strictly_sorted")
            for entry in entries:
                ident = str(canonical_uuid(entry["node_id"], version7=True))
                require(ident in current and ident in selected, "move_of_unselected_node")
                before = rectangle(entry.get("before"), page_size=page_size)
                after = rectangle(entry.get("after"), page_size=page_size)
                require(before == current[ident], "move_before_state_mismatch")
                require(before != after and before["width"] == after["width"] and
                        before["height"] == after["height"],
                        "move_not_pure_nonzero_translation")
                current[ident] = after
            moves += 1
        else:
            raise AssertionError("unadmitted_operation_in_fixture")
    require(set(original) == set(selected), "created_selected_ids_mismatch")
    require(moves in (0, 1), "move_count_invalid")
    expected = object_of(data["expected_bounds"])
    require(set(expected) == set(selected), "expected_nodes_mismatch")
    expected = {key: rectangle(value, page_size=page_size) for key, value in expected.items()}
    require(current == expected, "history_differs_from_independent_expected_geometry")
    moved = sorted(node for node in selected if original[node] != current[node])
    changed = data["expected_moved_node_ids"]
    require(type(changed) is list and all(type(x) is str for x in changed) and
            changed == sorted(set(changed)) and moved == changed,
            "changed_only_witness_mismatch")
    require((moves == 0) == (len(moved) == 0), "move_count_changed_only_mismatch")
    return {"source_sha256": data["source_sha256"], "revision_id": data["revision_id"],
            "page_id": page_id, "bounds": current, "paint": profiles,
            "moved": moved}


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def attribute(element: ET.Element, name: str) -> str | None:
    return next((value for key, value in element.attrib.items() if local(key) == name), None)


def parse_xml(content: bytes) -> ET.Element:
    require(len(content) < 8 * 1024 * 1024 and
            b"<!DOCTYPE" not in content.upper() and b"<!ENTITY" not in content.upper(),
            "untrusted_xml_rejected")
    return ET.fromstring(content)


def package_parts(path: Path) -> dict[str, bytes]:
    require(path.is_file() and 0 < path.stat().st_size <= MAX_ZIP,
            "zip_size_out_of_bounds")
    with zipfile.ZipFile(path) as archive:
        require(archive.testzip() is None, "zip_crc_failed")
        entries = archive.infolist()
        names = [item.filename for item in entries]
        require(len(names) <= MAX_PARTS and len(set(names)) == len(names),
                "zip_duplicate_or_excessive_parts")
        require(sum(item.file_size for item in entries) <= MAX_UNPACKED,
                "zip_unpacked_size_exceeded")
        require(all(not name.startswith("/") and ".." not in name.split("/")
                    for name in names), "zip_path_invalid")
        return {name: archive.read(name) for name in names}


def point_to_emu(text: str, *, suffix: bool = False) -> int:
    require(type(text) is str and len(text) <= 80, "point_coordinate_invalid")
    if suffix:
        require(text.endswith("pt"), "point_unit_missing")
        text = text[:-2]
    try:
        with localcontext() as ctx:
            ctx.prec = 80
            value = Decimal(text)
            require(value.is_finite(), "nonfinite_coordinate")
            return int((value * EMU_PER_POINT).to_integral_value(rounding=ROUND_HALF_UP))
    except InvalidOperation:
        raise AssertionError("point_coordinate_not_decimal") from None


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for buf in iter(lambda: f.read(1024 * 1024), b""):
            h.update(buf)
    return h.hexdigest()


def expected_idml_anchors(rect: dict) -> list[tuple[int, int]]:
    x, y = rect["x"], rect["y"]
    right, bottom = x + rect["width"], y + rect["height"]
    return [(x, y), (x, bottom), (right, bottom), (right, y)]


def check_color_resources(parts: dict, profiles: dict) -> None:
    colors = set()
    for profile in profiles.values():
        if profile["fill"]:
            colors.add(profile["fill_rgb"])
        if profile["stroke"]:
            colors.add(profile["stroke_rgb"])
    if not colors:
        return
    require("Resources/Graphic.xml" in parts, "idml_color_resources_missing")
    root = parse_xml(parts["Resources/Graphic.xml"])
    color_records = [e for e in root.iter() if local(e.tag) == "Color"]
    for triplet in colors:
        identity = "Color/Chaptera-" + rgb_hex(triplet)
        matches = [e for e in color_records if e.attrib.get("Self") == identity]
        require(len(matches) == 1, "idml_resource_identity_missing_or_duplicate")
        require(matches[0].attrib.get("ColorValue") == " ".join(str(n) for n in triplet),
                "idml_color_resource_payload_mismatch")


def verify_idml(parts: dict[str, bytes], witness: dict) -> None:
    pagehex = canonical_uuid(witness["page_id"]).hex
    wanted_spread = "Spreads/Spread_usp" + pagehex + ".xml"
    require(wanted_spread in parts, "idml_page_spread_missing")
    found: dict[str, list[tuple[str, ET.Element]]] = {}
    for path, data in parts.items():
        if path.startswith("Spreads/") and path.endswith(".xml"):
            root = parse_xml(data)
            for node in root.iter():
                if local(node.tag) == "Rectangle":
                    name = node.attrib.get("Self", "")
                    if name.startswith("uar"):
                        found.setdefault(name, []).append((path, node))
    require(set(found) == {"uar" + canonical_uuid(node).hex for node in witness["bounds"]},
            "idml_authored_rectangle_set_mismatch")
    check_color_resources(parts, witness["paint"])
    for node_id, rect in witness["bounds"].items():
        identity = "uar" + canonical_uuid(node_id).hex
        matches = found[identity]
        require(len(matches) == 1 and matches[0][0] == wanted_spread,
                "idml_rectangle_page_or_count_mismatch")
        node = matches[0][1]
        p = witness["paint"][node_id]
        fill = "Color/Chaptera-" + rgb_hex(p["fill_rgb"]) if p["fill"] else "Swatch/None"
        stroke = "Color/Chaptera-" + rgb_hex(p["stroke_rgb"]) if p["stroke"] else "Swatch/None"
        require(node.attrib.get("FillColor") == fill and
                node.attrib.get("StrokeColor") == stroke and
                point_to_emu(node.attrib.get("StrokeWeight")) ==
                (p["stroke_width_emu"] if p["stroke"] else 0),
                "idml_rectangle_paint_mismatch")
        anchors = [child.attrib.get("Anchor") for child in node.iter()
                   if local(child.tag) == "PathPointType"]
        require(len(anchors) == 4 and all(type(s) is str for s in anchors),
                "idml_anchors_missing")
        observed = []
        for text in anchors:
            parts_xy = text.split()
            require(len(parts_xy) == 2, "idml_anchor_not_pair")
            observed.append(tuple(point_to_emu(x) for x in parts_xy))
        require(observed == expected_idml_anchors(rect), "idml_exact_emu_mismatch")


def verify_odg(parts: dict[str, bytes], witness: dict) -> None:
    require("content.xml" in parts, "odg_content_missing")
    root = parse_xml(parts["content.xml"])
    page_name = "Page_" + canonical_uuid(witness["page_id"]).hex
    pages = [node for node in root.iter() if local(node.tag) == "page" and
             attribute(node, "name") == page_name]
    require(len(pages) == 1, "odg_page_missing_or_duplicate")
    all_rects = [node for node in root.iter() if local(node.tag) == "rect" and
                 (attribute(node, "name") or "").startswith("AuthoredRect_")]
    expected_names = {"AuthoredRect_" + canonical_uuid(n).hex for n in witness["bounds"]}
    require({attribute(rect, "name") for rect in all_rects} == expected_names and
            len(all_rects) == len(expected_names),
            "odg_authored_rectangle_set_mismatch")
    for node_id, expected in witness["bounds"].items():
        ident = canonical_uuid(node_id).hex
        name = "AuthoredRect_" + ident
        matches = [node for node in pages[0].iter() if local(node.tag) == "rect" and
                   attribute(node, "name") == name]
        require(len(matches) == 1, "odg_rectangle_wrong_page_or_duplicate")
        element = matches[0]
        for key in ("x", "y", "width", "height"):
            require(point_to_emu(attribute(element, key), suffix=True) == expected[key],
                    "odg_exact_emu_mismatch")
        p = witness["paint"][node_id]
        style_name = "AuthoredRectStyle_" + ident
        require(attribute(element, "style-name") == style_name,
                "odg_style_binding_mismatch")
        styles = [node for node in root.iter() if local(node.tag) == "style" and
                  attribute(node, "name") == style_name]
        require(len(styles) == 1, "odg_style_missing_or_duplicate")
        props = [node for node in styles[0] if local(node.tag) == "graphic-properties"]
        require(len(props) == 1, "odg_graphic_properties_missing")
        style = props[0]
        require(attribute(style, "fill") == ("solid" if p["fill"] else "none") and
                (not p["fill"] or attribute(style, "fill-color") == "#" + rgb_hex(p["fill_rgb"])) and
                attribute(style, "stroke") == ("solid" if p["stroke"] else "none") and
                (not p["stroke"] or attribute(style, "stroke-color") == "#" + rgb_hex(p["stroke_rgb"])) and
                (not p["stroke"] or point_to_emu(attribute(style, "stroke-width"), suffix=True) ==
                 p["stroke_width_emu"]), "odg_style_mismatch")


def verify(path: Path, target: str, witness_data: dict, *, native: bool = False,
           source: Path | None = None) -> dict:
    require(target in ("idml", "odg"), "target_invalid")
    witness = verified_witness(witness_data)
    if source is not None:
        require(sha256_file(source) == witness["source_sha256"],
                "original_pub_source_mutated_or_mismatched")
    parts = package_parts(path)
    if target == "idml":
        verify_idml(parts, witness)
    else:
        verify_odg(parts, witness)
    consumer = None
    if native:
        # Keep this exactly once per file, using the same native consumer as
        # the already admitted single-Rectangle physical acceptance.
        sys.path.insert(0, str(Path(__file__).resolve().parent / "ci"))
        from verify_authored_rectangle_export import consume  # type: ignore
        consumer = consume(path, target)
        require(consumer["reopened"] is True, "native_document_reopen_failed")
    return {"schema": PROOF_SCHEMA, "status": "PASS", "target": target,
            "artifact_sha256": sha256_file(path),
            "source_sha256": witness["source_sha256"],
            "revision_id": witness["revision_id"],
            "geometry_exact_emu": True,
            "verified_shape_count": len(witness["bounds"]),
            "verified_node_ids": sorted(witness["bounds"]),
            "changed_node_ids": witness["moved"],
            "native_document_reopen": consumer,
            "individual_native_shape_retention_verified": False}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--target", required=True, choices=("idml", "odg"))
    parser.add_argument("--witness", required=True, type=Path,
                        help="Source-neutral canonical Current.project operation witness")
    parser.add_argument("--source", type=Path,
                        help="Optional local original PUB for immutability SHA-256 check")
    parser.add_argument("--native", action="store_true",
                        help="Reopen exactly once using existing Scribus/LibreOffice consumer")
    args = parser.parse_args()
    witness = json.loads(args.witness.read_text(encoding="utf-8"))
    receipt = verify(args.artifact, args.target, witness,
                     native=args.native, source=args.source)
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
