#!/usr/bin/env python3
from __future__ import annotations
import importlib.util
from pathlib import Path

HERE=Path(__file__).resolve().parent
SPEC=importlib.util.spec_from_file_location("coverage",HERE/"artifact_family_label_coverage.py")
MOD=importlib.util.module_from_spec(SPEC); assert SPEC.loader; SPEC.loader.exec_module(MOD)

rows=[
 {"sha256":"a"*64,"candidate_filename":"newsletter.pub","provenance_join":"sha:lalamu"},
 {"sha256":"a"*64,"candidate_filename":"newsletter.pub","provenance_join":"sha:lalamu+url:legacy-exact"},
 {"sha256":"b"*64,"candidate_filename":"","provenance_join":"materialized-sha-only"},
]
r=MOD.build_report(rows)
assert r["sha_denominator"]==2,r
assert r["labeled_sha_count"]==1,r
assert r["unlabeled_sha_count"]==1,r
assert r["family_sha_counts"]=={"newsletter":1},r
assert r["nonexclusive_family_assignment_count"]==1,r
print("ok")
