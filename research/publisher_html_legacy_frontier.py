#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

WATCH = [
    ("GroupShape", "E13"),
    ("Txwp", "F04"),
    ("TextVertAlign", "1D04"),
    ("TextCopyFit", "1E04"),
    ("EcpRecolor", "2213"),
    ("FBID", "2418"),
    ("PoLnk", "2913"),
    ("BrdLeft", "2E13"),
    ("BrdRight", "2F13"),
    ("BrdTop", "3013"),
    ("BrdBottom", "3113"),
    ("TContainsWhiteText", "3E15"),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--harvest", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    harvest = json.loads(Path(args.harvest).read_text(encoding="utf-8"))
    registry = harvest.get("registry", [])
    by_key = {}
    for row in registry:
        if row.get("owner_type") != "OplOdpo":
            continue
        name = row.get("property_name")
        priv = (row.get("priv") or "").upper()
        by_key[(name, priv)] = row

    rows = []
    for name, priv in WATCH:
        hit = by_key.get((name, priv))
        rows.append({
            "owner_type": "OplOdpo",
            "property_name": name,
            "priv": priv,
            "present": hit is not None,
            "source_count": hit.get("source_count", 0) if hit else 0,
            "observations": hit.get("observations", 0) if hit else 0,
            "source_ids": hit.get("source_ids", []) if hit else [],
            "common_values": hit.get("common_values", []) if hit else [],
            "opyid": hit.get("opyid") if hit else int(priv[:-2], 16),
            "descriptor_type": hit.get("descriptor_type") if hit else int(priv[-2:], 16),
            "raw_tag": hit.get("raw_tag") if hit else None,
        })

    result = {
        "schema": "publisher-html-legacy-odpo-frontier.v1",
        "watch_count": len(rows),
        "present_count": sum(1 for r in rows if r["present"]),
        "missing_count": sum(1 for r in rows if not r["present"]),
        "rows": rows,
    }
    Path(args.out).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
