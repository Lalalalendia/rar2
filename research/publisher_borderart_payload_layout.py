#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

TARGETS = {
    "publisher11-northpark-white-dashes": {
        "name": "Basic...White Dashes",
        "dzl_corner": 76200,
        "dxl_horiz": 184150,
        "dyl_vert": 184150,
        "offsets": [108, 324, 480, 696, 852, 1068, 1224, 1440],
        "blob_sizes": [216, 156, 216, 156, 216, 156, 216, 156],
    },
    "publisher11-pfoltz-corner-triangles": {
        "name": "Corner Triangles",
        "dzl_corner": 127000,
        "dxl_horiz": 127000,
        "dyl_vert": 127000,
        "offsets": [108, 642, 850, 1384, 1592, 2126, 2334, 2868],
        "blob_sizes": [534, 208, 534, 208, 534, 208, 534, 208],
    },
}


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def one(rows, label):
    if len(rows) != 1:
        raise SystemExit(f"expected exactly one {label}, got {len(rows)}")
    return rows[0]


def value_int(prop, label):
    try:
        return int(str(prop.get("value", "")).strip(), 10)
    except Exception as exc:
        raise SystemExit(f"{label} is not an integer: {prop.get('value')!r}") from exc


def children(objects, source_id, parent_index):
    return [
        row
        for row in objects
        if row.get("source_id") == source_id
        and row.get("parent_object_index") == parent_index
    ]


def owned(properties, source_id, owner_index):
    return [
        row
        for row in properties
        if row.get("source_id") == source_id
        and row.get("owner_object_index") == owner_index
    ]


def named(props, name):
    return [row for row in props if row.get("name") == name]


def analyze_source(source_id, expected, objects, properties):
    source_objects = [row for row in objects if row.get("source_id") == source_id]

    rgfb_containers = [
        row for row in source_objects
        if row.get("tag") == "Rgfb" and row.get("type") == "OplFb"
    ]
    rgfb = one(rgfb_containers, f"{source_id} Rgfb container")

    entries = [
        row for row in children(objects, source_id, rgfb["object_index"])
        if row.get("tag") == "OplFb" and row.get("type") == "OplFb"
    ]
    entry = one(entries, f"{source_id} OplFb catalog entry")
    entry_props = owned(properties, source_id, entry["object_index"])

    name = str(one(named(entry_props, "SzFBrdName"), f"{source_id} SzFBrdName").get("value") or "")
    dzl_corner = value_int(one(named(entry_props, "DzlCorner"), f"{source_id} DzlCorner"), "DzlCorner")
    dxl_horiz = value_int(one(named(entry_props, "DxlHoriz"), f"{source_id} DxlHoriz"), "DxlHoriz")
    dyl_vert = value_int(one(named(entry_props, "DylVert"), f"{source_id} DylVert"), "DylVert")
    cmeta = value_int(one(named(entry_props, "CMeta"), f"{source_id} CMeta"), "CMeta")
    cfbmd = value_int(one(named(entry_props, "CFbmd"), f"{source_id} CFbmd"), "CFbmd")

    entry_children = children(objects, source_id, entry["object_index"])
    meta = one(
        [row for row in entry_children if row.get("type") == "OplRgfbMeta"],
        f"{source_id} OplRgfbMeta",
    )
    offsets = [
        value_int(row, f"{source_id} OplRgfbMeta.Data")
        for row in sorted(
            named(owned(properties, source_id, meta["object_index"]), "Data"),
            key=lambda row: row.get("property_index") or 0,
        )
    ]

    blob_container = one(
        [
            row for row in entry_children
            if row.get("tag") == "RgFbmd" and row.get("type") == "OplFbmd"
        ],
        f"{source_id} RgFbmd container",
    )
    blob_objects = sorted(
        [
            row for row in children(objects, source_id, blob_container["object_index"])
            if row.get("tag") == "OplFbmd" and row.get("type") == "OplFbmd"
        ],
        key=lambda row: row.get("object_index") or 0,
    )

    blob_sizes = []
    blob_rows = []
    for slot, blob in enumerate(blob_objects):
        prop = one(
            named(owned(properties, source_id, blob["object_index"]), "RgbMeta"),
            f"{source_id} OplFbmd[{slot}].RgbMeta",
        )
        cb = prop.get("cb")
        if cb is None:
            raise SystemExit(f"{source_id} OplFbmd[{slot}].RgbMeta is missing cb")
        try:
            cb_int = int(str(cb), 10)
        except ValueError as exc:
            raise SystemExit(f"{source_id} OplFbmd[{slot}].RgbMeta invalid cb={cb!r}") from exc
        blob_sizes.append(cb_int)
        blob_rows.append({
            "slot": slot,
            "cb": cb_int,
            "priv": prop.get("priv"),
            "priv_origin": prop.get("priv_origin"),
        })

    deltas = [offsets[i + 1] - offsets[i] for i in range(len(offsets) - 1)]
    alternating_two_class = (
        len(blob_sizes) == 8
        and len(set(blob_sizes[0::2])) == 1
        and len(set(blob_sizes[1::2])) == 1
        and blob_sizes[0] != blob_sizes[1]
    )

    checks = {
        "name_matches_expected": name == expected["name"],
        "geometry_matches_expected": [
            dzl_corner, dxl_horiz, dyl_vert
        ] == [
            expected["dzl_corner"], expected["dxl_horiz"], expected["dyl_vert"]
        ],
        "cmeta_is_eight": cmeta == 8,
        "cfbmd_is_eight": cfbmd == 8,
        "offset_count_matches_cmeta": len(offsets) == cmeta,
        "blob_count_matches_cfbmd": len(blob_sizes) == cfbmd,
        "first_payload_offset_is_108": bool(offsets) and offsets[0] == 108,
        "offset_deltas_match_prior_blob_sizes": deltas == blob_sizes[:-1],
        "offsets_match_expected": offsets == expected["offsets"],
        "blob_sizes_match_expected": blob_sizes == expected["blob_sizes"],
        "alternating_two_class_eight_slot_layout": alternating_two_class,
    }

    return {
        "source_id": source_id,
        "catalog_name": name,
        "geometry": {
            "dzl_corner": dzl_corner,
            "dxl_horiz": dxl_horiz,
            "dyl_vert": dyl_vert,
        },
        "cmeta": cmeta,
        "cfbmd": cfbmd,
        "metadata_offsets": offsets,
        "payload_blob_sizes": blob_sizes,
        "offset_deltas": deltas,
        "payload_region": {
            "start": offsets[0] if offsets else None,
            "end": offsets[-1] + blob_sizes[-1] if offsets and blob_sizes else None,
            "byte_length": sum(blob_sizes),
        },
        "blob_rows": blob_rows,
        "checks": checks,
        "verdict": "PASS" if all(checks.values()) else "FAIL",
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--objects", required=True)
    ap.add_argument("--properties", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    objects = load(args.objects)
    properties = load(args.properties)
    rows = [
        analyze_source(source_id, expected, objects, properties)
        for source_id, expected in TARGETS.items()
    ]

    result = {
        "schema": "publisher-borderart-payload-layout.v1",
        "source_count": len(rows),
        "pass_count": sum(row["verdict"] == "PASS" for row in rows),
        "rows": rows,
        "cross_source_checks": {
            "both_sources_have_eight_metadata_offsets": all(row["cmeta"] == 8 for row in rows),
            "both_sources_have_eight_payload_blobs": all(row["cfbmd"] == 8 for row in rows),
            "both_sources_start_payload_offsets_at_108": all(
                row["metadata_offsets"][0] == 108 for row in rows
            ),
            "both_sources_offsets_are_exact_blob_prefix_sums": all(
                row["offset_deltas"] == row["payload_blob_sizes"][:-1] for row in rows
            ),
            "both_sources_use_repeated_four_plus_four_size_classes": all(
                row["checks"]["alternating_two_class_eight_slot_layout"] for row in rows
            ),
        },
        "interpretation": {
            "closed": (
                "Publisher 10 and Publisher 11 independently expose the same OplFb layout: "
                "three geometry scalars, CMeta=8, eight cumulative metadata offsets, "
                "CFbmd=8 and eight ordered RgbMeta payload blobs. In both witnesses the "
                "first payload starts at 108 and each later offset equals the prior offset "
                "plus the prior blob cb exactly."
            ),
            "bounded_inference": (
                "The eight blobs split into two alternating four-member size classes in "
                "both styles, which is structurally consistent with a four-corner plus "
                "four-edge decorative decomposition. Slot semantics are not promoted "
                "without a native or decoded-payload discriminator."
            ),
            "not_closed": (
                "This does not yet identify the internal RgbMeta byte grammar, prove which "
                "slot is which edge/corner, establish render sufficiency, or replace the "
                "native BorderArt Delete/materialization experiment."
            ),
        },
    }

    if not all(row["verdict"] == "PASS" for row in rows):
        result["verdict"] = "FAIL"
    elif not all(result["cross_source_checks"].values()):
        result["verdict"] = "FAIL"
    else:
        result["verdict"] = "PASS"

    Path(args.out).write_text(
        json.dumps(result, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))
    if result["verdict"] != "PASS":
        raise SystemExit("BorderArt payload layout guard failed")


if __name__ == "__main__":
    main()
