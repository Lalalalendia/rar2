#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

TARGET_SOURCE = "publisher11-help-paired"
EXPECTED_NAME = "Basic...Wide Inline"
EXPECTED_FBID_PRIV = "2418"
EXPECTED_NAME_PRIV = "318"
EXPECTED_FANCY_HANDLE = "261"


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def norm(value):
    return (value or "").strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--properties", required=True)
    ap.add_argument("--objects", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    props = load(args.properties)
    objects = load(args.objects)

    sources = sorted({
        *(p.get("source_id") for p in props if p.get("source_id")),
        *(o.get("source_id") for o in objects if o.get("source_id")),
    })

    rows = []
    for source_id in sources:
        sp = [p for p in props if p.get("source_id") == source_id]
        so = [o for o in objects if o.get("source_id") == source_id]

        fbids = [
            p for p in sp
            if p.get("owner_type") == "OplOdpo" and p.get("name") == "FBID"
            and p.get("priv") is not None
        ]
        catalog_names = [
            p for p in sp
            if p.get("owner_type") == "OplFb" and p.get("name") == "SzFBrdName"
            and p.get("priv") is not None
        ]
        fancy_handles = [
            p for p in sp
            if p.get("owner_type") == "OplPub" and p.get("name") == "OhFancyBorders"
        ]
        fancy_objects = [
            o for o in so if o.get("type") == "OplPlbFb"
        ]

        catalog_by_name = {}
        for row in catalog_names:
            catalog_by_name.setdefault(norm(row.get("value")), []).append(row)

        handle_values = {norm(p.get("value")) for p in fancy_handles if norm(p.get("value"))}
        object_handles = {norm(o.get("oh")) for o in fancy_objects if norm(o.get("oh"))}
        handle_join = sorted(handle_values & object_handles)

        for fbid in fbids:
            value = norm(fbid.get("value"))
            matches = catalog_by_name.get(value, [])
            rows.append({
                "source_id": source_id,
                "fbid_value": value,
                "fbid_priv": (fbid.get("priv") or "").upper(),
                "fbid_priv_origin": fbid.get("priv_origin"),
                "fbid_raw_tag": fbid.get("raw_tag"),
                "catalog_name_match": bool(matches),
                "catalog_name_privs": sorted({(m.get("priv") or "").upper() for m in matches}),
                "catalog_name_observations": len(matches),
                "publication_fancy_handles": sorted(handle_values),
                "oplplbfb_handles": sorted(object_handles),
                "handle_join": handle_join,
                "four_way_join": bool(matches and handle_join),
            })

    target = [r for r in rows if r["source_id"] == TARGET_SOURCE]
    if len(target) != 1:
        raise SystemExit(f"expected exactly one {TARGET_SOURCE} FBID row, got {len(target)}")
    t = target[0]

    checks = {
        "fbid_value_exact": t["fbid_value"] == EXPECTED_NAME,
        "fbid_priv_exact": t["fbid_priv"] == EXPECTED_FBID_PRIV,
        "catalog_name_match": t["catalog_name_match"],
        "catalog_name_priv_exact": EXPECTED_NAME_PRIV in t["catalog_name_privs"],
        "publication_handle_exact": EXPECTED_FANCY_HANDLE in t["publication_fancy_handles"],
        "oplplbfb_handle_exact": EXPECTED_FANCY_HANDLE in t["oplplbfb_handles"],
        "handle_join_exact": EXPECTED_FANCY_HANDLE in t["handle_join"],
        "four_way_join": t["four_way_join"],
    }

    result = {
        "schema": "publisher-html-borderart-fbid-join.v1",
        "target_source": TARGET_SOURCE,
        "expected_profile_name": EXPECTED_NAME,
        "checks": checks,
        "verdict": "PASS" if all(checks.values()) else "FAIL",
        "target": t,
        "all_fbid_rows": rows,
        "interpretation": {
            "closed": "OplOdpo.FBID is a same-document profile-name key into the FancyBorders/OplPlbFb catalog for this exact Publisher 11 pair.",
            "not_closed": "The acronym expansion of FBID and whether LastFmt FBID is always the current active Shape.BorderArt authority remain open.",
        },
    }

    Path(args.out).write_text(
        json.dumps(result, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))

    if result["verdict"] != "PASS":
        raise SystemExit("Publisher 11 FBID -> FancyBorders same-document join failed")


if __name__ == "__main__":
    main()
