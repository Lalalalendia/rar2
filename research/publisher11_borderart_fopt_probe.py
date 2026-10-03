#!/usr/bin/env python3
import argparse
import collections
import hashlib
import json
from pathlib import Path

EXPECTED_SOURCE_SHA256 = "1e7f38b3ce1d0d956815992b15d361c405fc4bbdced5cdabb3c4581327cc183e"
TARGET_SHAPE_ID = 358
CONTROL_SHAPE_IDS = [350, 353, 356]


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def property_signature(row):
    return (
        row.get("rec_type"),
        row.get("property_id"),
        row.get("opid"),
        row.get("op"),
        row.get("complex_sha256"),
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--census", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    census = load(args.census)
    if census.get("source_sha256") != EXPECTED_SOURCE_SHA256:
        raise SystemExit(
            f"unexpected source SHA: {census.get('source_sha256')} != {EXPECTED_SOURCE_SHA256}"
        )

    by_id = {
        shape["publisher_shape_id"]: shape
        for shape in census.get("shapes", [])
        if shape.get("publisher_shape_id") is not None
    }
    target = by_id.get(TARGET_SHAPE_ID)
    if target is None:
        raise SystemExit(f"target Publisher Shape.ID {TARGET_SHAPE_ID} not found")

    signature_shapes = collections.defaultdict(set)
    property_id_shapes = collections.defaultdict(set)
    flag_shapes = {
        "f_bid": set(),
        "f_complex": set(),
        "op_is_blip_id": set(),
    }

    for shape_id, shape in by_id.items():
        for prop in shape.get("properties", []):
            signature_shapes[property_signature(prop)].add(shape_id)
            property_id_shapes[prop.get("property_id")].add(shape_id)
            for flag in flag_shapes:
                if prop.get(flag):
                    flag_shapes[flag].add(shape_id)

    target_properties = []
    for prop in target.get("properties", []):
        sig = property_signature(prop)
        shape_ids_same_signature = sorted(signature_shapes[sig])
        shape_ids_same_property_id = sorted(property_id_shapes[prop.get("property_id")])
        target_properties.append({
            **prop,
            "exact_signature_shape_count": len(shape_ids_same_signature),
            "exact_signature_shape_ids": shape_ids_same_signature,
            "property_id_shape_count": len(shape_ids_same_property_id),
            "property_id_shape_ids": shape_ids_same_property_id,
            "unique_exact_signature_to_target": shape_ids_same_signature == [TARGET_SHAPE_ID],
            "unique_property_id_to_target": shape_ids_same_property_id == [TARGET_SHAPE_ID],
        })

    target_properties.sort(
        key=lambda p: (
            not p["unique_property_id_to_target"],
            not p["unique_exact_signature_to_target"],
            p.get("property_id", 1 << 30),
            p.get("opid", 1 << 30),
        )
    )

    controls = []
    for shape_id in CONTROL_SHAPE_IDS:
        shape = by_id.get(shape_id)
        controls.append({
            "publisher_shape_id": shape_id,
            "present": shape is not None,
            "officeart_spid": shape.get("officeart_spid") if shape else None,
            "officeart_shape_type": shape.get("officeart_shape_type") if shape else None,
            "property_count": shape.get("property_count") if shape else None,
            "f_bid_property_count": sum(1 for p in shape.get("properties", []) if p.get("f_bid")) if shape else 0,
            "f_complex_property_count": sum(1 for p in shape.get("properties", []) if p.get("f_complex")) if shape else 0,
            "blip_id_property_count": sum(1 for p in shape.get("properties", []) if p.get("op_is_blip_id")) if shape else 0,
        })

    target_flags = {
        "f_bid_property_count": sum(1 for p in target_properties if p.get("f_bid")),
        "f_complex_property_count": sum(1 for p in target_properties if p.get("f_complex")),
        "blip_id_property_count": sum(1 for p in target_properties if p.get("op_is_blip_id")),
    }

    result = {
        "schema": "publisher11-borderart-fopt-probe.v1",
        "source_sha256": census["source_sha256"],
        "target_shape_id": TARGET_SHAPE_ID,
        "target": {
            "officeart_spid": target.get("officeart_spid"),
            "officeart_shape_type": target.get("officeart_shape_type"),
            "fopt_record_count": target.get("fopt_record_count"),
            "property_count": target.get("property_count"),
            **target_flags,
            "properties": target_properties,
        },
        "controls": controls,
        "global_flag_shape_ids": {
            key: sorted(value) for key, value in flag_shapes.items()
        },
        "candidate_summary": {
            "target_unique_property_ids": sorted({
                p["property_id"] for p in target_properties
                if p["unique_property_id_to_target"]
            }),
            "target_unique_exact_signatures": sum(
                1 for p in target_properties if p["unique_exact_signature_to_target"]
            ),
            "target_f_bid_property_ids": sorted({
                p["property_id"] for p in target_properties if p.get("f_bid")
            }),
            "target_complex_property_ids": sorted({
                p["property_id"] for p in target_properties if p.get("f_complex")
            }),
            "target_blip_id_property_ids": sorted({
                p["property_id"] for p in target_properties if p.get("op_is_blip_id")
            }),
        },
        "interpretation_boundary": (
            "This receipt identifies OfficeArt/FOPT candidates by exact same-file rarity and flags only. "
            "It does not assign BorderArt semantics to any property without an independent specification, "
            "mutation, or persistence join."
        ),
    }

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({
        "schema": result["schema"],
        "target_shape_id": TARGET_SHAPE_ID,
        "target_spid": result["target"]["officeart_spid"],
        "target_shape_type": result["target"]["officeart_shape_type"],
        "target_property_count": result["target"]["property_count"],
        "target_f_bid_property_count": result["target"]["f_bid_property_count"],
        "target_f_complex_property_count": result["target"]["f_complex_property_count"],
        "target_blip_id_property_count": result["target"]["blip_id_property_count"],
        **result["candidate_summary"],
    }, indent=2))


if __name__ == "__main__":
    main()
