#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

TARGET_SOURCE = "publisher11-help-paired"
TARGET_OH = "358"
TARGET_NAME = "Basic...Wide Inline"
CATALOG_ONLY_CONTROLS = (
    "publisher11-northpark-white-dashes",
    "publisher11-pfoltz-corner-triangles",
)


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def norm(value):
    return (value or "").strip()


def source_rows(rows, source_id):
    return [row for row in rows if row.get("source_id") == source_id]


def one(rows, label):
    if len(rows) != 1:
        raise SystemExit(f"expected exactly one {label}, got {len(rows)}")
    return rows[0]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--properties", required=True)
    ap.add_argument("--objects", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    props = load(args.properties)
    objects = load(args.objects)

    sp = source_rows(props, TARGET_SOURCE)
    so = source_rows(objects, TARGET_SOURCE)

    current_shape = one([
        o for o in so
        if o.get("type") == "OplPo" and norm(o.get("oh")) == TARGET_OH
    ], f"{TARGET_SOURCE} OplPo oh={TARGET_OH}")

    current_fbid = one([
        p for p in sp
        if p.get("owner_type") == "OplPo"
        and norm(p.get("owner_oh")) == TARGET_OH
        and p.get("name") == "Fbid"
    ], f"{TARGET_SOURCE} OplPo.Fbid for oh={TARGET_OH}")

    fbid_index = int(norm(current_fbid.get("value")), 10)

    ifbmax = one([
        p for p in sp
        if p.get("owner_type") == "OplPlbFb"
        and p.get("name") == "IfbMax"
    ], f"{TARGET_SOURCE} OplPlbFb.IfbMax")
    ifbmax_value = int(norm(ifbmax.get("value")), 10)

    catalog_rows = sorted([
        p for p in sp
        if p.get("owner_type") == "OplFb"
        and p.get("name") == "SzFBrdName"
        and norm(p.get("value"))
    ], key=lambda p: (
        p.get("owner_object_index") is None,
        p.get("owner_object_index") if p.get("owner_object_index") is not None else 1 << 60,
        p.get("property_index") if p.get("property_index") is not None else 1 << 60,
    ))
    catalog_names = [norm(p.get("value")) for p in catalog_rows]

    lastfmt_fbid = one([
        p for p in sp
        if p.get("owner_type") == "OplOdpo"
        and p.get("name") == "FBID"
    ], f"{TARGET_SOURCE} OplOdpo.FBID")

    track = one([
        p for p in sp
        if p.get("owner_type") == "OplOt"
        and p.get("name") == "OhTrack"
        and norm(p.get("value")) == TARGET_OH
    ], f"{TARGET_SOURCE} OplOt.OhTrack={TARGET_OH}")

    controls = []
    for source_id in CATALOG_ONLY_CONTROLS:
        cp = source_rows(props, source_id)
        co = source_rows(objects, source_id)
        control_ifbmax = one([
            p for p in cp
            if p.get("owner_type") == "OplPlbFb"
            and p.get("name") == "IfbMax"
        ], f"{source_id} OplPlbFb.IfbMax")
        control_catalog = sorted([
            p for p in cp
            if p.get("owner_type") == "OplFb"
            and p.get("name") == "SzFBrdName"
            and norm(p.get("value"))
        ], key=lambda p: (
            p.get("owner_object_index") is None,
            p.get("owner_object_index") if p.get("owner_object_index") is not None else 1 << 60,
            p.get("property_index") if p.get("property_index") is not None else 1 << 60,
        ))
        control_current_fbid = [
            p for p in cp
            if p.get("owner_type") == "OplPo" and p.get("name") == "Fbid"
        ]
        control_lastfmt_fbid = [
            p for p in cp
            if p.get("owner_type") == "OplOdpo" and p.get("name") == "FBID"
        ]
        controls.append({
            "source_id": source_id,
            "ifbmax": int(norm(control_ifbmax.get("value")), 10),
            "catalog_names": [norm(p.get("value")) for p in control_catalog],
            "current_oplpo_fbid_count": len(control_current_fbid),
            "lastfmt_odpo_fbid_count": len(control_lastfmt_fbid),
            "oplpo_object_count": sum(1 for o in co if o.get("type") == "OplPo"),
        })

    in_range = 0 <= fbid_index < ifbmax_value
    resolved_name = catalog_names[fbid_index] if in_range and fbid_index < len(catalog_names) else None

    checks = {
        "current_shape_exact": norm(current_shape.get("oh")) == TARGET_OH,
        "current_fbid_priv_0903": (current_fbid.get("priv") or "").upper() == "903",
        "current_fbid_zero": fbid_index == 0,
        "ifbmax_one": ifbmax_value == 1,
        "catalog_cardinality_matches_ifbmax": len(catalog_names) == ifbmax_value,
        "fbid_is_in_catalog_range": in_range,
        "catalog_index_resolves_target_name": resolved_name == TARGET_NAME,
        "lastfmt_fbid_priv_2418": (lastfmt_fbid.get("priv") or "").upper() == "2418",
        "lastfmt_name_matches_index_resolution": norm(lastfmt_fbid.get("value")) == resolved_name,
        "ohtrack_binds_same_current_shape": norm(track.get("value")) == TARGET_OH,
        "catalog_only_controls_have_catalog": all(c["ifbmax"] == 1 and len(c["catalog_names"]) == 1 for c in controls),
        "catalog_only_controls_have_no_current_fbid": all(c["current_oplpo_fbid_count"] == 0 for c in controls),
        "catalog_only_controls_have_no_lastfmt_fbid": all(c["lastfmt_odpo_fbid_count"] == 0 for c in controls),
    }

    result = {
        "schema": "publisher-borderart-fbid-index-join.v1",
        "source_id": TARGET_SOURCE,
        "shape_oh": TARGET_OH,
        "current_fbid": {
            "value": fbid_index,
            "priv": current_fbid.get("priv"),
            "owner_object_index": current_fbid.get("owner_object_index"),
        },
        "catalog": {
            "ifbmax": ifbmax_value,
            "names_in_observed_order": catalog_names,
            "resolved_name_at_fbid": resolved_name,
        },
        "lastfmt_fbid": {
            "value": norm(lastfmt_fbid.get("value")),
            "priv": lastfmt_fbid.get("priv"),
            "owner_object_index": lastfmt_fbid.get("owner_object_index"),
        },
        "track": {
            "ohtrack": norm(track.get("value")),
            "owner_object_index": track.get("owner_object_index"),
        },
        "catalog_only_controls": controls,
        "checks": checks,
        "verdict": "PASS" if all(checks.values()) else "FAIL",
        "interpretation": {
            "closed": (
                "For this exact Publisher 11 document, current OplPo.Fbid=0 is "
                "zero-based-index-consistent with the sole OplPlbFb catalog entry, "
                "and that resolved catalog name equals the same-shape LastFmt "
                "OplOdpo.FBID string. Two independent catalog-only controls expose "
                "a one-entry OplPlbFb catalog but no OplPo.Fbid/LastFmt.FBID usage."
            ),
            "not_closed": (
                "A single positive index=0 case does not prove universal Fbid indexing. "
                "Promotion to a format law requires a multi-entry catalog or Fbid>0 "
                "witness, or a native BorderArt mutation showing Fbid changes with the "
                "selected catalog item."
            ),
        },
    }

    Path(args.out).write_text(
        json.dumps(result, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))

    if result["verdict"] != "PASS":
        raise SystemExit("BorderArt Fbid index-consistency guard failed")


if __name__ == "__main__":
    main()
