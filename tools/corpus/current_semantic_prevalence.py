#!/usr/bin/env python3
"""Run the current Reader corpus receipt over exact PUB bytes and aggregate bounded prevalence."""
from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path


def one(exe: Path, pub: Path) -> dict:
    with tempfile.TemporaryDirectory() as td:
        out=Path(td)/"receipt.json"
        proc=subprocess.run([str(exe),str(pub),str(out)],capture_output=True,text=True)
        if proc.returncode != 0:
            return {
                "source_sha256": pub.stem.casefold(),
                "status":"producer_failed",
                "error_signature": proc.stderr[-4000:],
            }
        return json.loads(out.read_text(encoding="utf-8"))


def aggregate(rows: list[dict]) -> dict:
    opened=[r for r in rows if r.get("opened") is True]
    crop_files=0
    crop_placements=0
    mature_observed=0
    group_files=0
    group_children=0
    master_files=0
    master_relations=0
    mature_unavailable=0
    format_counts=Counter()

    for row in opened:
        format_counts[str(row.get("format_version") or "unknown")]+=1
        sem=row.get("semantic_prevalence") or {}
        crop=(sem.get("image_crop") or {})
        n=int(crop.get("placement_count") or 0)
        crop_placements += n
        crop_files += int(n>0)

        mature=(sem.get("mature_page_roles") or {})
        state=mature.get("state")
        if state=="observed_current_reader_authority":
            mature_observed += 1
            gc=int(mature.get("group_child_count") or 0)
            mr=int(mature.get("applied_master_relation_count") or 0)
            group_children += gc
            master_relations += mr
            group_files += int(gc>0)
            master_files += int(mr>0)
        elif state=="current_reader_authority_unavailable":
            mature_unavailable += 1

    return {
        "schema":"chaptera.current-semantic-prevalence.v1",
        "corpus_file_count":len(rows),
        "opened_file_count":len(opened),
        "failed_or_unsupported_count":len(rows)-len(opened),
        "format_version_counts":dict(sorted(format_counts.items())),
        "image_crop":{
            "authority":"ViewerEmbeddedImage.placements[].source_window",
            "files_with_observed_crop":crop_files,
            "observed_crop_placement_count":crop_placements,
            "denominator":"opened_file_count",
        },
        "mature_group_children":{
            "authority":"analyze_mature_0x2c_page_roles.pages[].group_child_count",
            "mature_files_observed":mature_observed,
            "mature_files_authority_unavailable":mature_unavailable,
            "files_with_group_children":group_files,
            "group_child_count":group_children,
            "denominator":"mature_files_observed",
        },
        "applied_master_relations":{
            "authority":"analyze_mature_0x2c_page_roles.pages[].applied_master_seq_num",
            "mature_files_observed":mature_observed,
            "mature_files_authority_unavailable":mature_unavailable,
            "files_with_applied_master_relation":master_files,
            "applied_master_relation_count":master_relations,
            "denominator":"mature_files_observed",
        },
        "grounded_guides":{
            "state":"not_wired_to_active_corpus_open_bundle",
            "count":None,
        },
        "mail_merge":{
            "state":"not_wired_to_generic_reader_corpus_receipt",
            "count":None,
        },
        "boundary":"Zero is meaningful only for observed authorities. not_wired/authority_unavailable are never coerced to zero.",
    }


def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--exe",required=True,type=Path)
    ap.add_argument("--corpus",required=True,type=Path)
    ap.add_argument("--out",required=True,type=Path)
    ap.add_argument("--rows-out",type=Path)
    ap.add_argument("--workers",type=int,default=8)
    args=ap.parse_args()
    pubs=sorted(args.corpus.glob("*.pub"))
    rows=[]
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures={pool.submit(one,args.exe,p):p for p in pubs}
        for future in as_completed(futures):
            rows.append(future.result())
    rows.sort(key=lambda r:str(r.get("source_sha256") or ""))
    report=aggregate(rows)
    args.out.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n",encoding="utf-8")
    if args.rows_out:
        args.rows_out.write_text(json.dumps(rows,indent=2,sort_keys=True)+"\n",encoding="utf-8")
    print(json.dumps(report,indent=2,sort_keys=True))


if __name__=="__main__":
    main()
