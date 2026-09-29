#!/usr/bin/env python3
from __future__ import annotations

import hashlib,json,re
from collections import Counter
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
R=ROOT/"tools"/"corpus"/"receipts"
PREV_RECEIPT=R/"corpus-current-authority-2026-09-25.json"
PREV_LIST=R/"current-rar-1521.sha256.txt"
CURRENT=R/"corpus-current-authority-2026-09-29.json"
SUCCESSOR=R/"current-rar-1556.sha256.txt"
INPUT=R/"lalamu-materialized-35-input-2026-09-29.json"
SHA_RE=re.compile(r"^[0-9a-f]{64}$")

def read(path):
    return [x.strip() for x in path.read_text(encoding="ascii").splitlines() if x.strip()]

def digest(shas):
    return hashlib.sha256(("\n".join(sorted(shas))+"\n").encode("ascii")).hexdigest()

def main():
    prev=json.loads(PREV_RECEIPT.read_text(encoding="utf-8"))
    current=json.loads(CURRENT.read_text(encoding="utf-8"))
    inp=json.loads(INPUT.read_text(encoding="utf-8"))
    old=read(PREV_LIST); new=read(SUCCESSOR)
    assert prev["schema"]=="rar-pub-corpus-current-authority-v4"
    assert current["schema"]=="rar-pub-corpus-current-authority-v5"
    assert len(old)==len(set(old))==1521 and old==sorted(old)
    assert len(new)==len(set(new))==1556 and new==sorted(new)
    assert all(SHA_RE.fullmatch(x) for x in new)
    assert digest(old)=="ab9be4a8981ad8f18e9d0a29a4688407e82ffb6ea66d8364d9249228328e6669"
    assert digest(new)=="950af4b1a1346168a55835b80bda062bbe1ebf9d926a4c5dc92a3d8bdb855073"
    rows=inp["files"]
    delta={r["sha256"] for r in rows}
    assert len(rows)==len(delta)==35
    assert digest(delta)=="64167f0e4cc78c3225df9b27c26deae23d9ab7735890b54d6beda1d5082560a4"
    assert set(old).isdisjoint(delta)
    assert set(new)==set(old)|delta
    assert inp["count"]==35
    assert inp["total_bytes"]==29552640
    assert inp["source_type_counts"]=={"github":14,"historical-web":1,"institutional-web":20}
    for row in rows:
        v=row["validation"]
        assert v["cfb_magic"] is True
        assert v["cfb_parseable"] is True
        assert v["sector_aligned"] is True
        assert v["all_streams_readable"] is True
        assert v["publisher_hint"] is True
    exact=current["rar_exact_union"]
    assert exact["count"]==1556
    assert exact["canonical_sorted_sha_lines_digest"]=="sha256:950af4b1a1346168a55835b80bda062bbe1ebf9d926a4c5dc92a3d8bdb855073"
    assert exact["retained_sha_list"]=="tools/corpus/receipts/current-rar-1556.sha256.txt"
    tranche=exact["tranches"]["materialized_lalamu35"]
    assert tranche["count"]==35
    assert tranche["canonical_sorted_sha_lines_digest"]=="sha256:64167f0e4cc78c3225df9b27c26deae23d9ab7735890b54d6beda1d5082560a4"
    assert tranche["total_bytes"]==29552640
    assert all(v==0 for k,v in exact["pairwise_overlap"].items() if k.endswith("__materialized_lalamu35"))
    ev=current["source_evidence"]["lalamu35_registration"]
    assert ev["new_unique_sha_count"]==35
    assert ev["predecessor_overlap"]==0
    assert ev["successor_count"]==1556
    assert ev["successor_digest"]=="sha256:950af4b1a1346168a55835b80bda062bbe1ebf9d926a4c5dc92a3d8bdb855073"
    bounds=current["global_complete_cfb_union"]
    assert bounds["lower_bound"]==1556
    assert bounds["upper_bound"]==1845
    print(json.dumps({
        "schema":current["schema"],
        "predecessor_count":1521,
        "new_tranche":35,
        "rar_exact_union":1556,
        "new_tranche_digest":"sha256:64167f0e4cc78c3225df9b27c26deae23d9ab7735890b54d6beda1d5082560a4",
        "successor_digest":"sha256:950af4b1a1346168a55835b80bda062bbe1ebf9d926a4c5dc92a3d8bdb855073",
        "global_lower_bound":1556,
        "global_upper_bound":1845
    },sort_keys=True))
    return 0

if __name__=="__main__":
    raise SystemExit(main())
