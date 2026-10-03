#!/usr/bin/env python3
import argparse
import json
from collections import defaultdict
from pathlib import Path


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

        catalog_names = [
            p for p in sp
            if p.get("owner_type") == "OplFb"
            and p.get("name") == "SzFBrdName"
            and p.get("priv") is not None
        ]
        fbids = [
            p for p in sp
            if p.get("owner_type") == "OplOdpo"
            and p.get("name") == "FBID"
            and p.get("priv") is not None
        ]
        fancy_handles = [
            p for p in sp
            if p.get("owner_type") == "OplPub"
            and p.get("name") == "OhFancyBorders"
        ]
        fancy_objects = [o for o in so if o.get("type") == "OplPlbFb"]

        catalog = sorted({norm(p.get("value")) for p in catalog_names if norm(p.get("value"))})
        usage = sorted({norm(p.get("value")) for p in fbids if norm(p.get("value"))})
        catalog_set = set(catalog)
        usage_set = set(usage)

        pub_handles = sorted({norm(p.get("value")) for p in fancy_handles if norm(p.get("value"))})
        obj_handles = sorted({norm(o.get("oh")) for o in fancy_objects if norm(o.get("oh"))})
        handle_join = sorted(set(pub_handles) & set(obj_handles))

        if usage and catalog:
            state = "catalog_and_usage"
        elif catalog:
            state = "catalog_only"
        elif usage:
            state = "usage_without_catalog"
        else:
            state = "neither"

        rows.append({
            "source_id": source_id,
            "state": state,
            "catalog_names": catalog,
            "usage_fbid_values": usage,
            "usage_values_resolve_into_catalog": sorted(usage_set & catalog_set),
            "usage_values_missing_from_catalog": sorted(usage_set - catalog_set),
            "publication_fancy_handles": pub_handles,
            "oplplbfb_handles": obj_handles,
            "handle_join": handle_join,
            "catalog_name_count": len(catalog),
            "fbid_count": len(fbids),
        })

    counts = defaultdict(int)
    for row in rows:
        counts[row["state"]] += 1

    catalog_only = [r for r in rows if r["state"] == "catalog_only"]
    catalog_and_usage = [r for r in rows if r["state"] == "catalog_and_usage"]
    usage_without_catalog = [r for r in rows if r["state"] == "usage_without_catalog"]

    result = {
        "schema": "publisher-borderart-catalog-usage-census.v1",
        "source_count": len(rows),
        "state_counts": dict(sorted(counts.items())),
        "catalog_only_source_count": len(catalog_only),
        "catalog_and_usage_source_count": len(catalog_and_usage),
        "usage_without_catalog_source_count": len(usage_without_catalog),
        "all_usage_resolves_into_same_document_catalog": all(
            not r["usage_values_missing_from_catalog"] for r in catalog_and_usage
        ),
        "catalog_presence_is_not_usage_evidence": len(catalog_only) > 0,
        "rows": rows,
        "interpretation": {
            "closed": "OplPlbFb is a document-level BorderArt/FancyBorders catalog that can exist without any observed OplOdpo.FBID shape usage in the same exported document. Where FBID is present, its value can be tested independently against same-document catalog names.",
            "not_closed": "This does not prove the physical current-shape carrier, rendering materialization, ordinary Line interaction, or BorderArt.Delete semantics."
        },
    }

    Path(args.out).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))

    if usage_without_catalog:
        raise SystemExit("Found FBID usage without same-document BorderArt catalog")
    if not catalog_only:
        raise SystemExit("Expected at least one catalog-only document")


if __name__ == "__main__":
    main()
