#!/usr/bin/env python3
from __future__ import annotations
import argparse, hashlib, json
from pathlib import Path

SCHEMA="chaptera.autonomus-parser-warning-contract.v1"
CASES=[
"unsupported_block_wire",
"truncated_u32",
"invalid_container_length",
"chunk_declared_range_oob",
"directory_wrong_slot_id",
"directory_wrong_supported_wire",
"reference_context_unsupported_wire",
"reference_seqnum_oob",
]

def reader_contract(path:Path)->dict:
    runtime=json.loads(path.read_text(encoding="utf-8"))
    if runtime.get("schema")!="chaptera.contents-missing-directory-fields.runtime.v1" or runtime.get("source_free") is not True:
        raise ValueError("invalid source-free Reader runtime receipt")
    diagnostics=runtime.get("diagnostics")
    if not isinstance(diagnostics,list) or [d.get("code") for d in diagnostics]!=["missing_raw_type","missing_chunk_offset"]:
        raise ValueError("Reader receipt must contain distinct missing-field codes")
    for diagnostic in diagnostics:
        if diagnostic.get("seq_num")!=1 or diagnostic.get("source")!={"stream":"/Synthetic","offset":2,"len":18} or diagnostic.get("parent_seq_num")!=23:
            raise ValueError("Reader receipt lost synthetic slot provenance or parent")
        if "chunk_offset" in diagnostic or "health" in diagnostic:
            raise ValueError("missing evidence cannot synthesize an offset or health verdict")
    if runtime.get("present_valid")!={"diagnostic_count":0,"chunk_offset":0,"fully_decoded":True}:
        raise ValueError("present valid reference must decode without missing-field warnings")
    if runtime.get("present_invalid")!={"code":"declared_range_out_of_bounds","offset":4294967295,"declared_length":0,"stream_len":4}:
        raise ValueError("explicit invalid offset must retain the physical range error")
    return runtime

def digest(path:Path)->str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main()->int:
    p=argparse.ArgumentParser()
    p.add_argument("--test-source",required=True,type=Path)
    p.add_argument("--commit-sha",required=True)
    p.add_argument("--output",required=True,type=Path)
    p.add_argument("--reader-runtime-receipt",type=Path)
    a=p.parse_args()
    if len(a.commit_sha)!=40 or any(c not in "0123456789abcdef" for c in a.commit_sha.lower()):
        raise SystemExit("invalid commit sha")
    r={
      "schema":SCHEMA,
      "task":"AUTONOMUS-GH-PARSER-WARNING-REGRESSION-01",
      "git_commit":a.commit_sha.lower(),
      "parser_surface":"vendor/producer-a/crates/pub-contents",
      "case_count":len(CASES),
      "cases":CASES,
      "test_source_sha256":digest(a.test_source),
      "source_free":True,
      "private_pub_bytes_used":False,
      "taxonomy_claim":"bounded parser diagnostic categories only",
      "limitations":[
        "No private warning logs or corpus bytes are inputs.",
        "Frequency of real warnings is not measured here.",
        "This does not assign PUB feature semantics."
      ]
    }
    if a.reader_runtime_receipt is not None:
        r["tasks"]=[r["task"],"PUB-CONTENTS-MISSING-DIR-FIELDS-DIAG-01"]
        r["reader_runtime"]=reader_contract(a.reader_runtime_receipt)
        r["reader_runtime_sha256"]=digest(a.reader_runtime_receipt)
        r["diagnostic_classification"]={
            "missing_raw_type":"missing_evidence",
            "missing_chunk_offset":"missing_evidence",
            "declared_range_out_of_bounds":"physical_range_failure",
        }
        r["cases"]=[*CASES,"missing_directory_fields_18_byte","independent_missing_fields","noncanonical_and_duplicate_fields","present_zero_offset","explicit_present_invalid_offset"]
        r["case_count"]=len(r["cases"])
        r["limitations"].append("Missing evidence alone assigns neither unknown-slot semantics nor document health.")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(r,indent=2,sort_keys=True)+"\n",encoding="utf-8")
    return 0

if __name__=="__main__":
    raise SystemExit(main())
