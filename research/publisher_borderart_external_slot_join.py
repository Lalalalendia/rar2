#!/usr/bin/env python3
import argparse
import hashlib
import json
import time
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

from publisher_borderart_payload_layout import (
    children,
    decode_publisher_uue,
    load,
    named,
    one,
    owned,
    value_int,
)

ASPOSE_REPO = "alaeddinejebali/Android-ConvertToPDF"
ASPOSE_COMMIT = "8f52e47e492b2853a47dbaa1d5c378ba61d96434"
ASPOSE_BASE = (
    "https://raw.githubusercontent.com/"
    f"{ASPOSE_REPO}/{ASPOSE_COMMIT}/"
    "ConvertToPdf-Aspose/app/src/main/assets/resources/PageBorderArt"
)
DEFINITIONS_GIT_BLOB_SHA1 = "33fe88386761d1d77756b9385e7da691d30dcb19"
POSITIONS = ("tl", "t", "tr", "r", "br", "b", "bl", "l")
EMU_PER_TWIP = 635

TARGETS = {
    "publisher11-help-paired": {
        "publisher_name": "Basic...Wide Inline",
        "external_name": "BasicWide Inline",
        "external_id": "77",
        "git_blob_sha1": [
            "97f47fce6d43cbd40288397e66c662d4d3945b6e",
            "a331a3512b018b5dc542cc5320cc8b9084d3df8b",
            "301d044bbf0d0464346de6a3c223094155d85581",
            "919b9e55a8b8a51e5f6c2eeb90f756f69e76f0dc",
            "510fb29addf7a327fd66a4baaeab571aa48d2b11",
            "006c471ee8ab3498b7c742c97a4fdc82c05bce35",
            "1d3d4b2385a098856c8c5694c193ddc3008a1765",
            "38d505667e196e57e2e99bf5c56fc936ac141afe",
        ],
    },
    "publisher11-northpark-white-dashes": {
        "publisher_name": "Basic...White Dashes",
        "external_name": "BasicWhite Dashes",
        "external_id": "74",
        "git_blob_sha1": [
            "3a760961cf360e825fa3be661097bc409c397a00",
            "8900a8a2a37213ad386e69e915be3301daff3da1",
            "e2f0d427975f4e47a5eccfa04efe3098350cad14",
            "4d90dba74624827a7607a5c54c8321e5c93325fb",
            "2b1eb27408f8e110f476535dc96989b01adaee11",
            "95b71d1c4b36e660797bf74c4fe2a93fafc4f3ae",
            "068a4e2b897cac5d7d904137aa0ac9d2e2d62f14",
            "aeeb38f6cc3e7bd15c9881c00455f76e3564ca6c",
        ],
    },
    "publisher11-pfoltz-corner-triangles": {
        "publisher_name": "Corner Triangles",
        "external_name": "Corner Triangles",
        "external_id": "104",
        "git_blob_sha1": [
            "dbaeb89a7e65e212a937c52f48c371bf3d38714e",
            "0ad21310b910e49719bd2de5c6498304bf966af0",
            "4dbb1d08a8ebc95c79a7d5ed6af8195b0eae336f",
            "bbd9ad09d9b8f6f2cdd939839425459483b74841",
            "d0c02aef81df1b708441acc9db2b5f5a3a8b5230",
            "9eb890cdc19f052694efe7b74fe7749e42f82df7",
            "10e7f77be58db8c94069f7618dd5a0ee153a150c",
            "76d8458261692cde065c175f6f3a8211c31a082f",
        ],
    },
}


def git_blob_sha1(data):
    header = f"blob {len(data)}\0".encode("ascii")
    return hashlib.sha1(header + data).hexdigest()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def fetch(url, attempts=3):
    last_error = None
    for attempt in range(attempts):
        try:
            request = urllib.request.Request(
                url,
                headers={"User-Agent": "chaptera-borderart-external-slot-join/1"},
            )
            with urllib.request.urlopen(request, timeout=45) as response:
                return response.read()
        except Exception as exc:
            last_error = exc
            if attempt + 1 < attempts:
                time.sleep(1 + attempt)
    raise SystemExit(f"fetch failed after {attempts} attempts: {url}: {last_error}")


def find_source_entry(source_id, publisher_name, objects, properties):
    source_objects = [row for row in objects if row.get("source_id") == source_id]
    rgfb = one(
        [
            row for row in source_objects
            if row.get("tag") == "Rgfb" and row.get("type") == "OplFb"
        ],
        f"{source_id} Rgfb",
    )
    entries = [
        row for row in children(objects, source_id, rgfb["object_index"])
        if row.get("tag") == "OplFb" and row.get("type") == "OplFb"
    ]
    matches = []
    for entry in entries:
        props = owned(properties, source_id, entry["object_index"])
        name_rows = named(props, "SzFBrdName")
        if len(name_rows) == 1 and str(name_rows[0].get("value") or "") == publisher_name:
            matches.append((entry, props))
    if len(matches) != 1:
        raise SystemExit(
            f"{source_id}: expected exactly one OplFb named {publisher_name!r}, "
            f"got {len(matches)}"
        )
    return matches[0]


def source_blob_rows(source_id, entry, objects, properties):
    entry_children = children(objects, source_id, entry["object_index"])
    container = one(
        [
            row for row in entry_children
            if row.get("tag") == "RgFbmd" and row.get("type") == "OplFbmd"
        ],
        f"{source_id} RgFbmd",
    )
    blob_objects = sorted(
        [
            row for row in children(objects, source_id, container["object_index"])
            if row.get("tag") == "OplFbmd" and row.get("type") == "OplFbmd"
        ],
        key=lambda row: row.get("object_index") or 0,
    )
    if len(blob_objects) != 8:
        raise SystemExit(f"{source_id}: expected 8 OplFbmd blobs, got {len(blob_objects)}")

    rows = []
    for slot, blob_object in enumerate(blob_objects):
        prop = one(
            named(owned(properties, source_id, blob_object["object_index"]), "RgbMeta"),
            f"{source_id} slot {slot} RgbMeta",
        )
        decoded, line_lengths = decode_publisher_uue(
            prop.get("value"),
            f"{source_id} slot {slot} RgbMeta",
        )
        rows.append(
            {
                "slot": slot,
                "position": POSITIONS[slot],
                "cb": int(str(prop.get("cb")), 10),
                "decoded": decoded,
                "publisher_sha256": sha256(decoded),
                "uue_line_lengths": line_lengths,
            }
        )
    return rows


def definition_entry(root, target):
    matches = [
        element
        for element in root.findall("BorderArt")
        if element.attrib.get("name") == target["external_name"]
        and element.attrib.get("id") == target["external_id"]
    ]
    if len(matches) != 1:
        raise SystemExit(
            f"Definitions.xml: expected one {target['external_name']!r}/"
            f"id={target['external_id']}, got {len(matches)}"
        )
    return matches[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--objects", required=True)
    parser.add_argument("--properties", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    objects = load(args.objects)
    properties = load(args.properties)

    definitions_url = f"{ASPOSE_BASE}/Definitions.xml"
    definitions = fetch(definitions_url)
    definitions_blob = git_blob_sha1(definitions)
    if definitions_blob != DEFINITIONS_GIT_BLOB_SHA1:
        raise SystemExit(
            "Definitions.xml git blob mismatch: "
            f"expected {DEFINITIONS_GIT_BLOB_SHA1}, got {definitions_blob}"
        )
    root = ET.fromstring(definitions.decode("utf-8-sig"))

    rows = []
    for source_id, target in TARGETS.items():
        entry, entry_props = find_source_entry(
            source_id,
            target["publisher_name"],
            objects,
            properties,
        )
        source_blobs = source_blob_rows(source_id, entry, objects, properties)
        definition = definition_entry(root, target)

        contraction = int(definition.attrib["contraction"], 10)
        hexpansion = int(definition.attrib["hexpansion"], 10)
        vexpansion = int(definition.attrib["vexpansion"], 10)

        publisher_geometry = {
            "dzl_corner": value_int(
                one(named(entry_props, "DzlCorner"), f"{source_id} DzlCorner"),
                f"{source_id} DzlCorner",
            ),
            "dxl_horiz": value_int(
                one(named(entry_props, "DxlHoriz"), f"{source_id} DxlHoriz"),
                f"{source_id} DxlHoriz",
            ),
            "dyl_vert": value_int(
                one(named(entry_props, "DylVert"), f"{source_id} DylVert"),
                f"{source_id} DylVert",
            ),
        }
        external_twips = {
            "contraction": contraction,
            "hexpansion": hexpansion,
            "vexpansion": vexpansion,
        }
        projected_emu = {
            "dzl_corner": contraction * EMU_PER_TWIP,
            "dxl_horiz": hexpansion * EMU_PER_TWIP,
            "dyl_vert": vexpansion * EMU_PER_TWIP,
        }

        slots = []
        for slot, position in enumerate(POSITIONS):
            filename = definition.attrib[position]
            expected_filename = f"{target['external_id']}_{slot + 1}.wmf"
            if filename != expected_filename:
                raise SystemExit(
                    f"{source_id}: expected {position}={expected_filename}, "
                    f"got {filename}"
                )

            external_url = f"{ASPOSE_BASE}/{filename}"
            external = fetch(external_url)
            external_blob_sha1 = git_blob_sha1(external)
            expected_blob_sha1 = target["git_blob_sha1"][slot]
            if external_blob_sha1 != expected_blob_sha1:
                raise SystemExit(
                    f"{source_id} {filename}: git blob mismatch; "
                    f"expected {expected_blob_sha1}, got {external_blob_sha1}"
                )

            source = source_blobs[slot]
            identical = source["decoded"] == external
            slots.append(
                {
                    "slot": slot,
                    "position": position.upper(),
                    "external_filename": filename,
                    "publisher_cb": source["cb"],
                    "publisher_decoded_size": len(source["decoded"]),
                    "external_size": len(external),
                    "publisher_sha256": source["publisher_sha256"],
                    "external_sha256": sha256(external),
                    "external_git_blob_sha1": external_blob_sha1,
                    "byte_identical": identical,
                }
            )

        checks = {
            "publisher_name_matches_target": str(
                one(named(entry_props, "SzFBrdName"), f"{source_id} SzFBrdName").get("value")
                or ""
            ) == target["publisher_name"],
            "external_definition_exact": (
                definition.attrib.get("name") == target["external_name"]
                and definition.attrib.get("id") == target["external_id"]
            ),
            "eight_directional_files": len(slots) == 8,
            "all_eight_wmfs_byte_identical": all(row["byte_identical"] for row in slots),
            "all_external_sizes_match_publisher_cb": all(
                row["external_size"] == row["publisher_cb"] for row in slots
            ),
            "geometry_twips_to_emu_exact": publisher_geometry == projected_emu,
        }

        rows.append(
            {
                "source_id": source_id,
                "publisher_name": target["publisher_name"],
                "external_definition": {
                    "name": target["external_name"],
                    "id": target["external_id"],
                    "positions": {
                        position.upper(): definition.attrib[position]
                        for position in POSITIONS
                    },
                    "twips": external_twips,
                },
                "publisher_geometry_emu": publisher_geometry,
                "projected_geometry_emu": projected_emu,
                "slots": slots,
                "checks": checks,
                "verdict": "PASS" if all(checks.values()) else "FAIL",
            }
        )

    all_slots = [slot for row in rows for slot in row["slots"]]
    result = {
        "schema": "publisher-borderart-external-slot-join.v1",
        "external_source": {
            "repository": ASPOSE_REPO,
            "commit": ASPOSE_COMMIT,
            "definitions_url": definitions_url,
            "definitions_git_blob_sha1": definitions_blob,
            "definitions_sha256": sha256(definitions),
        },
        "emu_per_twip": EMU_PER_TWIP,
        "slot_order": [position.upper() for position in POSITIONS],
        "style_count": len(rows),
        "slot_comparison_count": len(all_slots),
        "byte_identical_slot_count": sum(
            1 for slot in all_slots if slot["byte_identical"]
        ),
        "rows": rows,
        "checks": {
            "three_independent_styles": len(rows) == 3,
            "all_24_directional_wmfs_byte_identical": (
                len(all_slots) == 24
                and all(slot["byte_identical"] for slot in all_slots)
            ),
            "all_geometry_twip_projections_exact": all(
                row["checks"]["geometry_twips_to_emu_exact"] for row in rows
            ),
        },
        "interpretation": {
            "closed": (
                "For three independently hosted Publisher 10/11 BorderArt styles, every "
                "one of the eight hidden-XML OplFbmd.RgbMeta WMF payloads is byte-for-byte "
                "identical to the corresponding pinned external BorderArt WMF. The exact "
                "slot order is TL,T,TR,R,BR,B,BL,L. Publisher DzlCorner/DxlHoriz/DylVert "
                "are exact EMU projections of the external contraction/hexpansion/"
                "vexpansion twip scalars at 635 EMU per twip."
            ),
            "active_help_chain": (
                "For the exact Publisher11 help witness, current OplPo.Fbid=0 resolves to "
                "Basic...Wide Inline, whose eight catalog WMFs match external BasicWide "
                "Inline id=77 in the exact directional slot order."
            ),
            "not_closed": (
                "This static join does not prove Save/reopen mutation authority, Delete "
                "semantics, ordinary Shape.Line independence, custom BorderArt ownership, "
                "or whether a catalog relation is sufficient without any additional "
                "per-shape materialized state."
            ),
        },
    }
    result["verdict"] = (
        "PASS"
        if all(row["verdict"] == "PASS" for row in rows)
        and all(result["checks"].values())
        else "FAIL"
    )

    Path(args.out).write_text(
        json.dumps(result, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))

    if result["verdict"] != "PASS":
        raise SystemExit("BorderArt external slot join failed")


if __name__ == "__main__":
    main()
