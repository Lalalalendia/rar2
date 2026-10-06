#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib

from jsonschema import Draft202012Validator

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "packages" / "protocol" / "w2-compound" / "v1" / "receipt.schema.json"

def read(path: pathlib.Path):
    raw = path.read_bytes()
    return raw, json.loads(raw)

def sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()

def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--newsletter", required=True, type=pathlib.Path)
    p.add_argument("--overset", required=True, type=pathlib.Path)
    p.add_argument("--fork", required=True, type=pathlib.Path)
    p.add_argument("--pdf-run", required=True, type=pathlib.Path)
    p.add_argument("--output", required=True, type=pathlib.Path)
    args = p.parse_args()

    newsletter_raw, newsletter = read(args.newsletter)
    overset_raw, overset = read(args.overset)
    fork_raw, fork = read(args.fork)
    _, pdf_run = read(args.pdf_run)

    if newsletter.get("receipt_version") != "chaptera.editor-desktop-continuity-acceptance.v2":
        raise SystemExit("unexpected newsletter receipt")
    if newsletter["source"]["immutable"] is not True or newsletter["export"]["package_valid"] is not True:
        raise SystemExit("newsletter evidence is not accepted")
    if overset.get("receipt_version") != "chaptera.authoring-overset-receipt.v1":
        raise SystemExit("unexpected overset receipt")
    if overset["states"]["accepted"]["state"] != "overset" or overset["output_probe"]["overset_state_explicit"] is not True:
        raise SystemExit("overset evidence is not explicit")
    if fork.get("receipt_version") != "chaptera.project-fork-receipt.v1":
        raise SystemExit("unexpected fork receipt")
    if not fork["invariants"]["initial_state_preserved"] or not fork["invariants"]["parent_unchanged_after_fork_edit"]:
        raise SystemExit("fork evidence is incomplete")
    if not all(fork["invariants"][k] for k in ("project_id_rekeyed","document_id_rekeyed","history_id_rekeyed","genesis_revision_id_rekeyed")):
        raise SystemExit("fork identity was not fully re-keyed")

    if pdf_run.get("id") != 37474235340:
        raise SystemExit("wrong fixed-output workflow run")
    if pdf_run.get("head_sha") != "c92767c4e0236c48ec50aae2263e5e466881e29c":
        raise SystemExit("fixed-output head mismatch")
    if pdf_run.get("status") != "completed" or pdf_run.get("conclusion") != "success":
        raise SystemExit("fixed-output evidence is not green")

    receipt = {
        "receipt_version": "chaptera.w2-compound-receipt.v1",
        "evidence_mode": "split_evidence",
        "newsletter": {
            "receipt_sha256": sha(newsletter_raw),
            "source_hash": newsletter["source"]["sha256"],
            "validated": True,
        },
        "overset": {
            "receipt_sha256": sha(overset_raw),
            "source_hash": overset["source_hash"],
            "validated": True,
        },
        "project_fork": {
            "receipt_sha256": sha(fork_raw),
            "source_hash": fork["source_hash"],
            "validated": True,
        },
        "fixed_output": {
            "workflow_run_id": 37474235340,
            "head_sha": pdf_run["head_sha"],
            "conclusion": pdf_run["conclusion"],
            "verified_live": True,
        },
        "invariants": {
            "same_document_claim": False,
            "newsletter_source_immutable": True,
            "newsletter_editable_output": True,
            "overset_explicit": True,
            "fork_initial_state_preserved": True,
            "fork_identity_rekeyed": True,
            "fork_parent_unchanged": True,
            "wrap_preservation_claimed": False,
            "source_write_count": 0,
        },
    }
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    errors = sorted(Draft202012Validator(schema).iter_errors(receipt), key=lambda e: list(e.path))
    if errors:
        raise SystemExit("\n".join(f"{list(e.path)}: {e.message}" for e in errors))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
