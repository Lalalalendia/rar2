#!/usr/bin/env python3
"""Synthetic negative controls for the exact-EMU oracle; NOT real-PUB acceptance."""
from __future__ import annotations

import copy
import hashlib
import json
import tempfile
import unittest
import uuid
import xml.etree.ElementTree as ET
import zipfile
from decimal import Decimal, localcontext
from pathlib import Path

from verify_temporary_guest_move_nodes_export import (
    SCHEMA, point_to_emu, verify, verified_witness,
)

PAGE = "019a2d80-1112-7001-8001-aabbccddee00"
A = "019a2d80-1112-7001-8001-aabbccddee01"
B = "019a2d80-1112-7001-8001-aabbccddee02"
C = "019a2d80-1112-7001-8001-aabbccddee03"
IDS = [A, B, C]
FILL = "123456"
STROKE = "AABBCC"


def bounds(x: int) -> dict:
    return dict(x=x, y=100_000, width=500_000, height=250_000)


def create_shape(node_id: str, x: int) -> dict:
    return {
        "kind": "create_shape", "node_id": node_id, "page_id": PAGE,
        "parent_id": PAGE, "shape_kind": "rectangle",
        "provenance": {"kind": "author_created"}, "transform": {"kind": "identity"},
        "bounds": bounds(x),
        "paint": {"provenance": {"kind": "author_created"},
                  "fill": {"visible": True, "color": {"r": 18, "g": 52, "b": 86}},
                  "stroke": {"visible": True, "color": {"r": 170, "g": 187, "b": 204},
                             "width_emu": 12700}},
    }


def witness(*, move: bool = True) -> dict:
    before = {A: bounds(100000), B: bounds(300000), C: bounds(900000)}
    operations = [create_shape(A, 100000), create_shape(B, 300000), create_shape(C, 900000)]
    after = copy.deepcopy(before)
    changed = []
    if move:
        operations.append({"kind": "move_nodes", "page_id": PAGE,
                           "entries": [{"node_id": B, "before": bounds(300000),
                                        "after": bounds(500000)}]})
        after[B] = bounds(500000)
        changed = [B]
    return {"schema": SCHEMA, "source_sha256": "f" * 64,
            "revision_id": "sha256:" + "0" * 64, "page_id": PAGE,
            "page_size": {"width": 3_000_000, "height": 2_000_000},
            "selected_node_ids": IDS[:], "operations": operations,
            "expected_bounds": after, "expected_moved_node_ids": changed}


def pt(emu: int) -> str:
    with localcontext() as ctx:
        ctx.prec = 80
        return f"{Decimal(emu) / Decimal(12700):.15f}".rstrip("0").rstrip(".")


def build_package(filename: Path, target: str, data: dict,
                  *, emitted: dict | None = None,
                  omit: set[str] | None = None,
                  duplicate: str | None = None,
                  wrong_page: str | None = None,
                  wrong_paint: str | None = None) -> None:
    layout = verified_witness(data)
    positions = copy.deepcopy(layout["bounds"])
    if emitted:
        positions.update(emitted)
    omit = omit or set()
    with zipfile.ZipFile(filename, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        if target == "idml":
            spread = ET.Element("idPkg:Spread", {"xmlns:idPkg":"http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"})
            for node_id, rect in positions.items():
                if node_id in omit:
                    continue
                def append():
                    shape = ET.SubElement(spread, "Rectangle", {
                        "Self": "uar" + uuid.UUID(node_id).hex,
                        "FillColor": "Color/Chaptera-" + ("FFFFFF" if wrong_paint == node_id else FILL),
                        "StrokeColor": "Color/Chaptera-" + STROKE,
                        "StrokeWeight": "1",
                    })
                    path = ET.SubElement(shape, "PathGeometry")
                    x,y = rect["x"], rect["y"]
                    right,bottom = x+rect["width"],y+rect["height"]
                    for a,b in ((x,y),(x,bottom),(right,bottom),(right,y)):
                        ET.SubElement(path, "PathPointType", {"Anchor":pt(a)+" "+pt(b)})
                append()
                if duplicate == node_id:
                    append()
            path = "Spreads/Spread_usp"+uuid.UUID(wrong_page or PAGE).hex+".xml"
            archive.writestr(path, ET.tostring(spread, encoding="utf-8"))
            graph = ET.Element("Graphic")
            ET.SubElement(graph,"Color",Self="Color/Chaptera-"+FILL,
                          ColorValue="18 52 86")
            ET.SubElement(graph,"Color",Self="Color/Chaptera-"+STROKE,
                          ColorValue="170 187 204")
            archive.writestr("Resources/Graphic.xml",ET.tostring(graph,encoding="utf-8"))
        else:
            root = ET.Element("office:document", {"xmlns:office":"urn:oasis:names:tc:opendocument:xmlns:office:1.0", "xmlns:style":"urn:oasis:names:tc:opendocument:xmlns:style:1.0", "xmlns:draw":"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0", "xmlns:svg":"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"})
            styles = ET.SubElement(root,"office:automatic-styles")
            target_page = ET.SubElement(root,"draw:page",{"draw:name":"Page_"+uuid.UUID(wrong_page or PAGE).hex})
            for node_id, rect in positions.items():
                if node_id in omit:
                    continue
                ident = uuid.UUID(node_id).hex
                style = ET.SubElement(styles,"style:style",{"style:name":"AuthoredRectStyle_"+ident})
                ET.SubElement(style,"style:graphic-properties",{
                    "draw:fill":"solid", "draw:fill-color":"#"+("FFFFFF" if wrong_paint==node_id else FILL),
                    "draw:stroke":"solid","svg:stroke-color":"#"+STROKE,
                    "svg:stroke-width":"1pt"})
                def append():
                    ET.SubElement(target_page,"draw:rect",{
                        "draw:name":"AuthoredRect_"+ident,
                        "draw:style-name":"AuthoredRectStyle_"+ident,
                        "svg:x":pt(rect["x"])+"pt", "svg:y":pt(rect["y"])+"pt",
                        "svg:width":pt(rect["width"])+"pt", "svg:height":pt(rect["height"])+"pt"})
                append()
                if duplicate == node_id:
                    append()
            archive.writestr("content.xml",ET.tostring(root,encoding="utf-8"))


class ExactMoveNodesExportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def check_package(self,target="idml",*,data=None,**kwargs):
        data = data or witness()
        dest = self.root / ("edited."+target)
        build_package(dest,target,data,**kwargs)
        return verify(dest,target,data,native=False)

    def test_roundtrip_exact_decimal(self):
        for n in (0,1,100000,300000,500000,900000,9007199254740991,
                  -1,-500000,-9007199254740991):
            with self.subTest(emu=n):
                self.assertEqual(point_to_emu(pt(n)),n)
        self.assertEqual(point_to_emu("1pt",suffix=True),12700)

    def test_three_shapes_exact_idml(self):
        receipt=self.check_package()
        self.assertEqual(receipt["status"],"PASS")
        self.assertTrue(receipt["geometry_exact_emu"])
        self.assertEqual(receipt["verified_shape_count"],3)
        self.assertEqual(receipt["changed_node_ids"],[B])
        self.assertFalse(receipt["individual_native_shape_retention_verified"])

    def test_three_shapes_exact_odg(self):
        receipt=self.check_package(target="odg")
        self.assertEqual(receipt["status"],"PASS")
        self.assertEqual(receipt["verified_node_ids"],sorted(IDS))

    def test_undo_state_has_zero_moves(self):
        data=witness(move=False)
        for target in ("idml","odg"):
            with self.subTest(target=target):
                r=self.check_package(target=target,data=data)
                self.assertEqual(r["changed_node_ids"],[])

    def test_one_emu_drift_rejected_in_both_formats(self):
        for target in ("idml","odg"):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError,"exact_emu_mismatch"):
                    self.check_package(target=target,emitted={B:bounds(500001)})

    def test_old_pre_move_geometry_rejected(self):
        for target in ("idml","odg"):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError,"exact_emu_mismatch"):
                    self.check_package(target=target,emitted={B:bounds(300000)})

    def test_missing_identity_rejected(self):
        for target in ("idml","odg"):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError,"rectangle_set_mismatch"):
                    self.check_package(target=target,omit={B})

    def test_duplicate_identity_rejected(self):
        for target in ("idml","odg"):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError,"count_mismatch|set_mismatch"):
                    self.check_package(target=target,duplicate=B)

    def test_wrong_page_rejected(self):
        other="019a2d80-1112-7001-8001-aabbccddee44"
        for target in ("idml","odg"):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError,"page_spread_missing|page_missing"):
                    self.check_package(target=target,wrong_page=other)

    def test_wrong_fill_rejected(self):
        for target in ("idml","odg"):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError,"paint_mismatch|style_mismatch"):
                    self.check_package(target=target,wrong_paint=B)

    def test_mutated_anchor_rejected(self):
        data=witness()
        data["expected_bounds"][A]["x"]=100001
        with self.assertRaisesRegex(AssertionError,"history_differs_from"):
            verified_witness(data)

    def test_stale_before_state_rejected(self):
        data=witness()
        data["operations"][-1]["entries"][0]["before"]["x"]+=1
        with self.assertRaisesRegex(AssertionError,"move_before_state_mismatch"):
            verified_witness(data)

    def test_duplicate_create_shape_rejected(self):
        data=witness()
        data["operations"][1]["node_id"]=A
        with self.assertRaisesRegex(AssertionError,"duplicate_created_node"):
            verified_witness(data)

    def test_extra_source_backed_operation_rejected(self):
        data=witness()
        data["operations"].insert(3,{"kind":"move_node"})
        with self.assertRaisesRegex(AssertionError,"unadmitted_operation_in_fixture"):
            verified_witness(data)

    def test_out_of_page_rejected(self):
        data=witness()
        data["operations"][-1]["entries"][0]["after"]["x"]=2_800_000
        with self.assertRaisesRegex(AssertionError,"rectangle_outside_page"):
            verified_witness(data)

    def test_untrusted_fixture_fields_rejected(self):
        data=witness()
        data["bearer"]="should-never-be-there"
        with self.assertRaisesRegex(AssertionError,"unexpected_object_fields"):
            verified_witness(data)

    def test_mismatched_original_pub_sha_rejected(self):
        data=witness()
        filename=self.root/"original.pub"
        filename.write_bytes(b"real bytes stay outside the source repository")
        dest=self.root/"edited.odg"
        build_package(dest,"odg",data)
        with self.assertRaisesRegex(AssertionError,"original_pub_source_mutated"):
            verify(dest,"odg",data,source=filename)


if __name__ == "__main__":
    unittest.main()
