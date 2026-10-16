#!/usr/bin/env python3
"""One-import, per-shape native proof for source-neutral PR2790 CI fixtures.

The already pinned Chaptera IDML/ODG package is checked by the exact-EMU
oracle separately. This probe asks an unrelated application to import and save
that package, and pairs all three authored rectangles by resulting geometry.
Native import is allowed a stated 1-point coordinate tolerance, NEVER used by
the exact-EMU package oracle. Shapes are anonymous after conversion; matching
by geometry does not assert that author UUIDs survive the native roundtrip.
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
import xml.etree.ElementTree as ET
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP, localcontext
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from verify_temporary_guest_move_nodes_export import verified_witness, verify

EMU_PER_POINT = Decimal(12700)
NATIVE_TOLERANCE_EMU = 12700  # one point; never an exact-EMU claim
MAX_XML_BYTES = 48 * 1024 * 1024
LENGTH_PATTERN = re.compile(r"^([+-]?(?:\d+(?:\.\d*)?|\.\d+))(pt|cm|mm|in|pc|px)?$")
UNIT_TO_POINTS = {"pt": Decimal(1), "cm": Decimal(3600) / Decimal(127),
                  "mm": Decimal(360) / Decimal(127), "in": Decimal(72),
                  "pc": Decimal(12), "px": Decimal(3) / Decimal(4)}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def local(key: str) -> str:
    return key.rsplit("}", 1)[-1]


def attr(element: ET.Element, name: str) -> str | None:
    return next((value for k, value in element.attrib.items() if local(k) == name), None)


def length_to_emu(text: str, *, bare_unit: str = "pt") -> int:
    require(isinstance(text, str) and len(text) <= 80, "native_length_invalid")
    match = LENGTH_PATTERN.fullmatch(text.strip())
    require(match is not None, "native_length_parse_failed")
    val, unit = match.groups()
    unit = unit or bare_unit
    try:
        with localcontext() as ctx:
            ctx.prec = 56
            scaled = Decimal(val) * UNIT_TO_POINTS[unit] * EMU_PER_POINT
            result = int(scaled.quantize(Decimal(1), rounding=ROUND_HALF_UP))
    except (InvalidOperation, OverflowError, KeyError) as error:
        raise AssertionError("native_length_parse_failed") from error
    require(abs(result) < 10**16, "native_length_unreasonable")
    return result


def collect_scribus_shapes(parsed: ET.Element) -> list[dict]:
    rectangles = []
    for node in parsed.iter():
        if local(node.tag).upper() != "PAGEOBJECT":
            continue
        fields = [attr(node, k) for k in ("XPOS", "YPOS", "WIDTH", "HEIGHT")]
        if any(v is None for v in fields):
            continue
        values = [length_to_emu(v) for v in fields]
        rectangles.append(dict(zip(("x", "y", "width", "height"), values)))
    return rectangles


def collect_libreoffice_shapes(parsed: ET.Element) -> list[dict]:
    candidates = []
    for node in parsed.iter():
        if local(node.tag) not in ("rect", "custom-shape"):
            continue
        fields = [attr(node, k) for k in ("x", "y", "width", "height")]
        if any(v is None for v in fields):
            continue
        values = [length_to_emu(v) for v in fields]
        candidates.append(dict(zip(("x", "y", "width", "height"), values)))
    return candidates


def uniquely_match(expected: dict[str, dict], observed: list[dict],
                   tolerance_emu: int = NATIVE_TOLERANCE_EMU) -> dict:
    """Injective matching: one independent native object for each distinct ID."""
    require(0 < tolerance_emu <= NATIVE_TOLERANCE_EMU, "invalid_native_tolerance")
    require(len(expected) == 3 and len(observed) <= 256,
            "native_rectangle_budget_invalid")
    ids = sorted(expected)
    edges: list[list[tuple[int, int]]] = []
    for identity in ids:
        wanted = expected[identity]
        possible = []
        for j, seen in enumerate(observed):
            delta = max(abs(wanted[key] - seen[key])
                        for key in ("x", "y", "width", "height"))
            if delta <= tolerance_emu:
                possible.append((j, delta))
        edges.append(possible)
    # Bounded three-member bipartite matching; no single native item satisfies 2.
    best = None
    def search(i: int, used: set[int], pairs: list[tuple[int, int]]) -> None:
        nonlocal best
        if i == len(ids):
            candidate = (max(delta for _, delta in pairs), sum(delta for _, delta in pairs), pairs[:])
            if best is None or candidate[:2] < best[:2]:
                best = candidate
            return
        for j, delta in edges[i]:
            if j not in used:
                used.add(j)
                pairs.append((j, delta))
                search(i + 1, used, pairs)
                pairs.pop()
                used.remove(j)
    search(0, set(), [])
    require(best is not None,
            "native_individual_geometry_retention_failed: expected=" +
            json.dumps(expected, sort_keys=True) + "; observed=" +
            json.dumps(observed[:30], sort_keys=True))
    return {"matched_shape_count": 3, "native_object_count": len(observed),
            "max_native_delta_emu": best[0],
            "matching_tolerance_emu": tolerance_emu,
            "geometry_matching_method": "injective_bbox_4d",
            "matched_node_ids": ids,
            "matched_object_indices": [j for j, _ in best[2]]}


def match_native_group(expected: dict[str, dict], observed: list[dict],
                       tolerance_emu: int = NATIVE_TOLERANCE_EMU) -> dict:
    """Prove 3 injective bounds with either absolute or ONE shared canvas shift.

    Scribus imports IDML onto a pasteboard; native XPOS/YPOS can acquire an
    origin offset even when each object and all inter-object distances survive.
    A shared translation is *not* proof of absolute native page placement.
    The original IDML/ODG XML is separately verified at exact integer EMU.
    """
    try:
        absolute = uniquely_match(expected, observed, tolerance_emu)
    except AssertionError as err:
        if not str(err).startswith("native_individual_geometry_retention_failed"):
            raise
    else:
        return {**absolute, "native_registration_mode": "absolute",
                "native_origin_shift_emu": {"x": 0, "y": 0},
                "native_absolute_page_position_verified": True}

    require(len(expected) == 3 and 3 <= len(observed) <= 256,
            "native_rectangle_budget_invalid")
    # At least one distinct geometric signature per selected authored object.
    require(len({tuple(v[key] for key in ("x", "y", "width", "height"))
                 for v in expected.values()}) == 3,
            "native_individual_geometry_nonunique")
    first = expected[sorted(expected)[0]]
    candidates: list[tuple[int, int, int, dict]] = []
    for seen in observed:
        if (abs(first["width"] - seen["width"]) > tolerance_emu or
                abs(first["height"] - seen["height"]) > tolerance_emu):
            continue
        dx, dy = seen["x"] - first["x"], seen["y"] - first["y"]
        shifted = {ident: {**bbox, "x": bbox["x"] + dx, "y": bbox["y"] + dy}
                   for ident, bbox in expected.items()}
        try:
            hit = uniquely_match(shifted, observed, tolerance_emu)
        except AssertionError as err:
            if not str(err).startswith("native_individual_geometry_retention_failed"):
                raise
            continue
        candidates.append((hit["max_native_delta_emu"], dx, dy, hit))
    require(candidates, "native_individual_geometry_retention_failed: shared translation "
            "not found; expected=" + json.dumps(expected, sort_keys=True) +
            "; observed=" + json.dumps(observed[:30], sort_keys=True))
    candidates.sort(key=lambda x: (x[0], abs(x[1])+abs(x[2]), x[1], x[2]))
    best_error, dx, dy, hit = candidates[0]
    # An unrelated second full triple must not be silently confused with the
    # authored group if its registration has equally good residual error.
    for other_error, other_dx, other_dy, other_hit in candidates[1:]:
        if other_error > best_error:
            break
        if (other_hit["matched_object_indices"] != hit["matched_object_indices"]
                and (abs(other_dx - dx) > tolerance_emu or
                     abs(other_dy - dy) > tolerance_emu)):
            raise AssertionError("native_registration_ambiguous")
    return {**hit, "geometry_matching_method": "injective_bbox_4d_uniform_translation",
            "native_registration_mode": "uniform_canvas_translation",
            "native_origin_shift_emu": {"x": dx, "y": dy},
            "native_absolute_page_position_verified": False}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def run_native(package: Path, target: str, expected: dict[str, dict]) -> dict:
    tools = ("scribus", "xvfb-run", "libreoffice")
    for tool in tools:
        require(shutil.which(tool) is not None, "native_consumer_missing: " + tool)
    with tempfile.TemporaryDirectory(prefix="pr2790-native-") as d:
        root = Path(d)
        source = root / ("input." + target)
        shutil.copyfile(package, source)
        env = os.environ.copy()
        env["HOME"] = str(root)
        if target == "idml":
            output = root / "result.sla"
            helper = Path("tools/ci/scribus_idml_consumer.py").resolve()
            require(helper.is_file(), "scribus_consumer_script_missing")
            env["CHAPTERA_SCRIBUS_SLA_OUT"] = str(output)
            cmd = ["xvfb-run", "-a", "scribus", "-g", "-ns", "-py", str(helper),
                   "--", str(source)]
            app = "Scribus"
        else:
            folder = root / "lo"
            folder.mkdir()
            output = folder / "input.fodg"
            cmd = ["libreoffice", "-env:UserInstallation=" +
                   (root / "profile").as_uri(), "--headless", "--convert-to",
                   "fodg", "--outdir", str(folder), str(source)]
            app = "LibreOffice Draw"
        result = subprocess.run(cmd, env=env, capture_output=True, timeout=160, check=False)
        require(result.returncode == 0 and output.exists() and
                0 < output.stat().st_size <= MAX_XML_BYTES,
                app + "_import_or_save_failed: " +
                result.stderr.decode("utf-8", "replace")[-900:])
        shape_root = ET.parse(output).getroot()
        shapes = (collect_scribus_shapes(shape_root) if target == "idml"
                  else collect_libreoffice_shapes(shape_root))
        matching = match_native_group(expected, shapes)
        return {"application": app, "reopened": True,
                "import_sha256": sha256_file(output),
                "individual_native_shape_retention_verified": True,
                **matching}


def main() -> int:
    parser = argparse.ArgumentParser(description="Source-neutral native 3-Rectangle retention proof")
    parser.add_argument("--target", required=True, choices=("idml", "odg"))
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--witness", type=Path, required=True)
    args = parser.parse_args()
    witness_data = json.loads(args.witness.read_text(encoding="utf-8"))
    verified = verified_witness(witness_data)
    package = verify(args.artifact, args.target, witness_data)
    native = run_native(args.artifact, args.target, verified["bounds"])
    receipt = {**package, "native_document_reopen": native,
               "individual_native_shape_retention_verified": True}
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
