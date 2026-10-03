#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

SCHEMA = "chaptera.mature-multimaster-origin-029.v1"
PAGE_ROLE_SCHEMA = "chaptera.pub-page-role-observation.v1"

SOURCES = [
    "273d283c93fafb0f14b772ba8475a6b48db0a4f9c1a0789605ce26ea811996d3",
    "5eb5055bc75918ca8dbc7093fa267a3365f25129fd2c3dc88c7be3540f44e4cd",
    "7c630704ce369f775fe7a24ec180f5c7997eb779f28bd57ab8c7dda66297a17c",
    "9915c5612d7d8ce49fc733bbbe748c80325fb0094af0ba0d42a2b15fc648e5bd",
    "a8c8a3c36a925fdc9de7c4f5e43e014a606751b2d09635cbc1b04f50ccb7fcea",
    "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e",
]

RAW_SHAPE=0x01
RAW_TABLE=0x10
RAW_GROUP=0x30

def load(path: Path)->dict[str,Any]:
    return json.loads(path.read_text(encoding="utf-8"))

def payload_count(page:dict[str,Any])->int:
    c=page.get("child_raw_type_counts",{})
    return sum(int(c.get(str(t),c.get(t,0))) for t in (RAW_SHAPE,RAW_TABLE,RAW_GROUP))

def field05(page:dict[str,Any])->dict[str,Any]|None:
    rows=[f for f in page.get("fields",[]) if int(f.get("id",-1))==0x05]
    if len(rows)!=1:
        return None
    row=rows[0]
    return {
        "block_type":int(row["block_type"]),
        "declared_length":row.get("declared_length"),
        "container_sha256":row.get("container_sha256"),
    }

def main()->None:
    ap=argparse.ArgumentParser()
    ap.add_argument("receipt_dir",type=Path)
    ap.add_argument("output",type=Path)
    args=ap.parse_args()

    documents=[]
    first=[]
    second=[]
    roots=[]
    for sha in SOURCES:
        d=load(args.receipt_dir/f"{sha}.json")
        if d.get("schema")!=PAGE_ROLE_SCHEMA:
            raise ValueError(f"unexpected schema for {sha}")
        pages=sorted(d["pages"],key=lambda p:int(p["document_ordinal"]))
        by_seq={int(p["contents_seq_num"]):int(p["document_ordinal"]) for p in pages}
        applied={}
        for p in pages:
            m=p.get("applied_master_seq_num")
            if m is None:
                continue
            mo=by_seq.get(int(m))
            if mo is not None:
                applied.setdefault(mo,[]).append(p)

        masters=[]
        for mo,children in sorted(applied.items()):
            positive=sorted((p for p in children if payload_count(p)>0),key=lambda p:int(p["document_ordinal"]))
            rows=[{"document_ordinal":int(p["document_ordinal"]),"origin":field05(p)} for p in positive]
            if len(positive)>=1: first.append(field05(positive[0]))
            if len(positive)>=2: second.append(field05(positive[1]))
            masters.append({"master_ordinal":mo,"positive_applied":rows})

        root_rows=[]
        for p in pages:
            if p.get("applied_master_seq_num") is None:
                root_rows.append({"document_ordinal":int(p["document_ordinal"]),"origin":field05(p)})
                roots.append(field05(p))
        documents.append({"source_sha256":sha,"roots":root_rows,"masters":masters})

    def hist(rows):
        return [
            {"origin":json.loads(key),"count":count}
            for key,count in Counter(
                json.dumps(row,sort_keys=True) for row in rows
            ).most_common()
        ]

    first_keys={json.dumps(x,sort_keys=True) for x in first}
    second_keys={json.dumps(x,sort_keys=True) for x in second}

    out={
        "schema":SCHEMA,
        "source_count":len(documents),
        "documents":documents,
        "comparison":{
            "first_positive_origin_histogram":hist(first),
            "second_positive_origin_histogram":hist(second),
            "root_origin_histogram":hist(roots),
            "first_only_origin_classes":[json.loads(x) for x in sorted(first_keys-second_keys)],
            "second_only_origin_classes":[json.loads(x) for x in sorted(second_keys-first_keys)],
            "discovery_only":True,
            "origin_promoted_to_authority":False,
        },
        "claims":{
            "measurement_only":True,
            "raw_container_bytes_emitted":False,
            "only_container_sha256_emitted":True,
            "publisher_or_pdf_reference_used":False,
            "product_selection_changed":False,
        },
    }
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(out,indent=2,sort_keys=True)+"\n",encoding="utf-8")
    print(json.dumps(out["comparison"],indent=2,sort_keys=True))

if __name__=="__main__":
    main()
