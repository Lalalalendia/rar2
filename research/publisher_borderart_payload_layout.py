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

WMF_FUNCTION_NAMES = {
    0x0000: "META_EOF",
    0x0102: "META_SETBKMODE",
    0x0103: "META_SETMAPMODE",
    0x0104: "META_SETROP2",
    0x0106: "META_SETPOLYFILLMODE",
    0x012D: "META_SELECTOBJECT",
    0x012E: "META_SETTEXTALIGN",
    0x01F0: "META_DELETEOBJECT",
    0x0201: "META_SETBKCOLOR",
    0x0209: "META_SETTEXTCOLOR",
    0x020B: "META_SETWINDOWORG",
    0x020C: "META_SETWINDOWEXT",
    0x020D: "META_SETVIEWPORTORG",
    0x020E: "META_SETVIEWPORTEXT",
    0x02FA: "META_CREATEPENINDIRECT",
    0x02FC: "META_CREATEBRUSHINDIRECT",
    0x0324: "META_POLYGON",
    0x0325: "META_POLYLINE",
    0x0418: "META_ELLIPSE",
    0x041B: "META_RECTANGLE",
    0x061C: "META_ROUNDRECT",
    0x0922: "META_BITBLT",
    0x0B23: "META_STRETCHBLT",
    0x0F43: "META_STRETCHDIB",
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


def decode_publisher_uue(value, label):
    encoded = "".join(str(value or "").split())
    if not encoded:
        raise SystemExit(f"{label}: empty encoded RgbMeta payload")

    out = bytearray()
    line_lengths = []
    pos = 0
    while pos < len(encoded):
        encoded_length = (ord(encoded[pos]) - 32) & 0x3F
        pos += 1
        line_lengths.append(encoded_length)
        if encoded_length == 0:
            continue

        full_groups, remainder = divmod(encoded_length, 3)
        group_count = full_groups + (1 if remainder else 0)
        produced = 0

        for group_index in range(group_count):
            if group_index < full_groups or remainder == 0:
                encoded_chars = 4
            else:
                # Publisher's hidden-XML UUE omits unused final padding characters.
                encoded_chars = remainder + 1

            if pos + encoded_chars > len(encoded):
                raise SystemExit(
                    f"{label}: truncated UUE group at encoded offset {pos}; "
                    f"need {encoded_chars}, have {len(encoded) - pos}"
                )

            sextets = [
                (ord(encoded[pos + i]) - 32) & 0x3F
                for i in range(encoded_chars)
            ]
            pos += encoded_chars
            while len(sextets) < 4:
                sextets.append(0)

            a, b, c, d = sextets
            decoded = (
                (a << 2) | (b >> 4),
                ((b & 0x0F) << 4) | (c >> 2),
                ((c & 0x03) << 6) | d,
            )
            for byte in decoded:
                if produced >= encoded_length:
                    break
                out.append(byte)
                produced += 1

        if produced != encoded_length:
            raise SystemExit(
                f"{label}: UUE line promised {encoded_length} bytes, decoded {produced}"
            )

    if pos != len(encoded):
        raise SystemExit(
            f"{label}: UUE decoder stopped at {pos}/{len(encoded)} encoded bytes"
        )

    return bytes(out), line_lengths


def u16le(data, offset):
    if offset + 2 > len(data):
        raise SystemExit(f"u16 read crosses payload boundary at {offset}")
    return int.from_bytes(data[offset:offset + 2], "little")


def u32le(data, offset):
    if offset + 4 > len(data):
        raise SystemExit(f"u32 read crosses payload boundary at {offset}")
    return int.from_bytes(data[offset:offset + 4], "little")


def parse_wmf(data, label):
    if len(data) < 18:
        raise SystemExit(f"{label}: shorter than 18-byte WMF METAHEADER")

    header = {
        "mt_type": u16le(data, 0),
        "mt_header_size_words": u16le(data, 2),
        "mt_version": u16le(data, 4),
        "mt_size_words": u32le(data, 6),
        "mt_no_objects": u16le(data, 10),
        "mt_max_record_words": u32le(data, 12),
        "mt_no_parameters": u16le(data, 16),
    }

    header_checks = {
        "memory_metafile_type_1": header["mt_type"] == 1,
        "header_size_is_9_words": header["mt_header_size_words"] == 9,
        "wmf_version_is_0x0300": header["mt_version"] == 0x0300,
        "declared_size_matches_payload": header["mt_size_words"] * 2 == len(data),
        "no_header_parameters": header["mt_no_parameters"] == 0,
    }

    records = []
    pos = 18
    while pos < len(data):
        if pos + 6 > len(data):
            raise SystemExit(f"{label}: truncated WMF record header at {pos}")

        size_words = u32le(data, pos)
        function = u16le(data, pos + 4)
        if size_words < 3:
            raise SystemExit(
                f"{label}: invalid WMF record size {size_words} words at {pos}"
            )
        size_bytes = size_words * 2
        end = pos + size_bytes
        if end > len(data):
            raise SystemExit(
                f"{label}: WMF record at {pos} crosses payload boundary "
                f"({end}>{len(data)})"
            )

        records.append({
            "offset": pos,
            "size_words": size_words,
            "function": function,
            "function_hex": f"0x{function:04X}",
            "function_name": WMF_FUNCTION_NAMES.get(function),
        })
        pos = end
        if function == 0x0000:
            break

    record_checks = {
        "has_records": bool(records),
        "ends_with_meta_eof": bool(records) and records[-1]["function"] == 0x0000,
        "eof_is_three_words": bool(records)
        and records[-1]["function"] == 0x0000
        and records[-1]["size_words"] == 3,
        "record_stream_consumes_payload": pos == len(data),
        "max_record_matches_header": bool(records)
        and max(row["size_words"] for row in records) == header["mt_max_record_words"],
    }

    signature = [row["function"] for row in records]
    return {
        "header": header,
        "header_checks": header_checks,
        "records": records,
        "record_checks": record_checks,
        "program_signature": [f"0x{value:04X}" for value in signature],
        "program_names": [
            WMF_FUNCTION_NAMES.get(value, f"UNKNOWN_0x{value:04X}")
            for value in signature
        ],
        "valid": all(header_checks.values()) and all(record_checks.values()),
    }


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

    name = str(
        one(named(entry_props, "SzFBrdName"), f"{source_id} SzFBrdName").get("value")
        or ""
    )
    dzl_corner = value_int(
        one(named(entry_props, "DzlCorner"), f"{source_id} DzlCorner"),
        "DzlCorner",
    )
    dxl_horiz = value_int(
        one(named(entry_props, "DxlHoriz"), f"{source_id} DxlHoriz"),
        "DxlHoriz",
    )
    dyl_vert = value_int(
        one(named(entry_props, "DylVert"), f"{source_id} DylVert"),
        "DylVert",
    )
    cmeta = value_int(
        one(named(entry_props, "CMeta"), f"{source_id} CMeta"),
        "CMeta",
    )
    cfbmd = value_int(
        one(named(entry_props, "CFbmd"), f"{source_id} CFbmd"),
        "CFbmd",
    )

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
            raise SystemExit(
                f"{source_id} OplFbmd[{slot}].RgbMeta invalid cb={cb!r}"
            ) from exc

        label = f"{source_id} OplFbmd[{slot}].RgbMeta"
        decoded, uue_line_lengths = decode_publisher_uue(prop.get("value"), label)
        wmf = parse_wmf(decoded, label)

        blob_sizes.append(cb_int)
        blob_rows.append({
            "slot": slot,
            "cb": cb_int,
            "priv": prop.get("priv"),
            "priv_origin": prop.get("priv_origin"),
            "decoded_size": len(decoded),
            "decoded_size_matches_cb": len(decoded) == cb_int,
            "uue_line_lengths": uue_line_lengths,
            "wmf": wmf,
        })

    deltas = [offsets[i + 1] - offsets[i] for i in range(len(offsets) - 1)]
    alternating_two_class = (
        len(blob_sizes) == 8
        and len(set(blob_sizes[0::2])) == 1
        and len(set(blob_sizes[1::2])) == 1
        and blob_sizes[0] != blob_sizes[1]
    )

    even_signatures = {
        tuple(row["wmf"]["program_signature"])
        for row in blob_rows[0::2]
    }
    odd_signatures = {
        tuple(row["wmf"]["program_signature"])
        for row in blob_rows[1::2]
    }
    alternating_program_classes = (
        len(blob_rows) == 8
        and len(even_signatures) == 1
        and len(odd_signatures) == 1
        and even_signatures != odd_signatures
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
        "all_uue_payloads_decode_to_cb": all(
            row["decoded_size_matches_cb"] for row in blob_rows
        ),
        "all_payloads_are_complete_wmf_streams": all(
            row["wmf"]["valid"] for row in blob_rows
        ),
        "alternating_four_plus_four_wmf_program_classes": alternating_program_classes,
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
        "wmf_program_classes": {
            "even_slots": sorted(next(iter(even_signatures))) if len(even_signatures) == 1 else None,
            "odd_slots": sorted(next(iter(odd_signatures))) if len(odd_signatures) == 1 else None,
            "even_slots_hex": list(next(iter(even_signatures))) if len(even_signatures) == 1 else None,
            "odd_slots_hex": list(next(iter(odd_signatures))) if len(odd_signatures) == 1 else None,
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
        "schema": "publisher-borderart-payload-layout.v2",
        "source_count": len(rows),
        "pass_count": sum(row["verdict"] == "PASS" for row in rows),
        "rows": rows,
        "cross_source_checks": {
            "both_sources_have_eight_metadata_offsets": all(
                row["cmeta"] == 8 for row in rows
            ),
            "both_sources_have_eight_payload_blobs": all(
                row["cfbmd"] == 8 for row in rows
            ),
            "both_sources_start_payload_offsets_at_108": all(
                row["metadata_offsets"][0] == 108 for row in rows
            ),
            "both_sources_offsets_are_exact_blob_prefix_sums": all(
                row["offset_deltas"] == row["payload_blob_sizes"][:-1]
                for row in rows
            ),
            "both_sources_decode_all_eight_payloads_as_complete_wmf": all(
                row["checks"]["all_payloads_are_complete_wmf_streams"]
                for row in rows
            ),
            "both_sources_use_alternating_four_plus_four_wmf_program_classes": all(
                row["checks"]["alternating_four_plus_four_wmf_program_classes"]
                for row in rows
            ),
        },
        "interpretation": {
            "closed": (
                "Publisher 10 and Publisher 11 independently expose the same OplFb "
                "materialization layout: three geometry scalars, CMeta=8, eight cumulative "
                "metadata offsets, CFbmd=8 and eight ordered RgbMeta payloads. Every RgbMeta "
                "decodes to a complete standard WMF METAHEADER/record stream whose declared "
                "WMF byte size equals the XML cb exactly. The first payload starts at 108 "
                "and each later metadata offset is the exact prefix sum of prior WMF cb "
                "values. OplFb therefore stores eight embedded WMF vector programs, not "
                "opaque arbitrary bytes."
            ),
            "bounded_inference": (
                "Within each independent style, slots 0/2/4/6 share one exact WMF function "
                "program and slots 1/3/5/7 share another distinct exact WMF function "
                "program. Together with DzlCorner/DxlHoriz/DylVert and the four-plus-four "
                "cardinality, this is strong structural evidence for four corner-class and "
                "four edge-class decorative pieces. Exact slot orientation is not yet "
                "promoted."
            ),
            "not_closed": (
                "This does not yet assign TL/T/TR/R/BR/B/BL/L slot order, prove that a "
                "shape Fbid alone is sufficient to render the catalog asset, establish "
                "custom BorderArt ownership, or replace the native Delete/Line/materialization "
                "experiment."
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
