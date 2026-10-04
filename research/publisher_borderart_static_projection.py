#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

TARGET_SOURCE = "publisher11-help-paired"
TARGET_OH = "358"
EXPECTED_CURRENT_FBID_PRIV = "903"
EXPECTED_CURRENT_FBID_VALUE = "0"
EXPECTED_LASTFMT_FBID_PRIV = "2418"
EXPECTED_LASTFMT_FBID_VALUE = "Basic...Wide Inline"


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def norm(v):
    return (v or "").strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--properties", required=True)
    ap.add_argument("--objects", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    props = load(args.properties)
    objects = load(args.objects)

    current_objects = [
        o for o in objects
        if o.get("source_id") == TARGET_SOURCE
        and o.get("type") == "OplPo"
        and norm(o.get("oh")) == TARGET_OH
    ]
    if len(current_objects) != 1:
        raise SystemExit(f"expected exactly one OplPo oh={TARGET_OH}, got {len(current_objects)}")
    current_index = current_objects[0].get("object_index")

    current_fbid = [
        p for p in props
        if p.get("source_id") == TARGET_SOURCE
        and p.get("owner_type") == "OplPo"
        and norm(p.get("owner_oh")) == TARGET_OH
        and p.get("name") == "Fbid"
    ]

    lastfmt_fbid = [
        p for p in props
        if p.get("source_id") == TARGET_SOURCE
        and p.get("owner_type") == "OplOdpo"
        and p.get("name") == "FBID"
    ]

    track_rows = [
        p for p in props
        if p.get("source_id") == TARGET_SOURCE
        and p.get("owner_type") == "OplOt"
        and p.get("name") == "OhTrack"
        and norm(p.get("value")) == TARGET_OH
    ]

    checks = {
        "one_current_oplpo": len(current_objects) == 1,
        "one_current_fbid": len(current_fbid) == 1,
        "current_fbid_priv": len(current_fbid) == 1 and (current_fbid[0].get("priv") or "").upper() == EXPECTED_CURRENT_FBID_PRIV,
        "current_fbid_value": len(current_fbid) == 1 and norm(current_fbid[0].get("value")) == EXPECTED_CURRENT_FBID_VALUE,
        "one_lastfmt_fbid": len(lastfmt_fbid) == 1,
        "lastfmt_fbid_priv": len(lastfmt_fbid) == 1 and (lastfmt_fbid[0].get("priv") or "").upper() == EXPECTED_LASTFMT_FBID_PRIV,
        "lastfmt_fbid_value": len(lastfmt_fbid) == 1 and norm(lastfmt_fbid[0].get("value")) == EXPECTED_LASTFMT_FBID_VALUE,
        "track_to_current_oh": len(track_rows) == 1,
        "distinct_property_names": bool(current_fbid and lastfmt_fbid and current_fbid[0].get("name") != lastfmt_fbid[0].get("name")),
        "distinct_priv_coordinates": bool(current_fbid and lastfmt_fbid and (current_fbid[0].get("priv") or "").upper() != (lastfmt_fbid[0].get("priv") or "").upper()),
        "distinct_value_domains": bool(current_fbid and lastfmt_fbid and norm(current_fbid[0].get("value")) != norm(lastfmt_fbid[0].get("value"))),
    }

    result = {
        "schema": "publisher-borderart-static-projection.v1",
        "source_id": TARGET_SOURCE,
        "shape_oh": TARGET_OH,
        "current_oplpo_object_index": current_index,
        "current_oplpo_fbid": current_fbid,
        "object_tracking_ohtrack": track_rows,
        "lastfmt_odpo_fbid": lastfmt_fbid,
        "checks": checks,
        "verdict": "PASS" if all(checks.values()) else "FAIL",
        "interpretation": {
            "closed": "For the exact Publisher 11 help pair, OplPo.Fbid priv=903 value=0 and OplLastFmt.OplOdpo.FBID priv=2418 profile-name string are distinct same-shape projections. Similar spelling is not evidence of a shared carrier.",
            "not_closed": "This does not establish which projection controls current Shape.BorderArt rendering, whether the LastFmt name is stale history, or how decorative pixels/assets are materialized."
        }
    }

    Path(args.out).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
    if result["verdict"] != "PASS":
        raise SystemExit("BorderArt static projection guard failed")


if __name__ == "__main__":
    main()
