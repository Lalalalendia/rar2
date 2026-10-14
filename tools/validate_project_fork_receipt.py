#!/usr/bin/env python3
import json
import pathlib
import sys

from jsonschema import Draft202012Validator

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "packages" / "protocol" / "project-fork" / "v1" / "producer-receipt.schema.json"

def validate_schema(receipt):
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    errors = sorted(Draft202012Validator(schema).iter_errors(receipt), key=lambda e: list(e.path))
    if errors:
        detail = "\n".join(f"{list(e.path)}: {e.message}" for e in errors)
        raise AssertionError("project fork receipt schema validation failed\n" + detail)

def validate_semantics(receipt):
    parent = receipt["parent"]
    fork = receipt["fork_initial"]
    provenance = fork["forked_from"]
    after = receipt["fork_after_edit"]

    if fork["state_id"] != parent["state_id"]:
        raise AssertionError("fork must preserve initial effective state")
    for key in ("project_id","document_id","history_id","genesis_revision_id"):
        if fork[key] == parent[key]:
            raise AssertionError(f"fork must re-key {key}")
    for key in ("project_id","document_id","history_id","state_id"):
        if provenance[key] != parent[key]:
            raise AssertionError(f"fork provenance {key} differs from parent")
    if after["state_id"] == parent["state_id"]:
        raise AssertionError("fork edit must diverge from parent state")
    if after["operation_count"] < 1:
        raise AssertionError("fork edit must emit at least one operation")

    return {
        "receipt_kind": receipt["receipt_version"],
        "source_hash": receipt["source_hash"],
        "initial_state_preserved": True,
        "identity_rekeyed": True,
        "provenance_exact": True,
        "fork_edit_diverged": True,
        "parent_reopen_exact": receipt["reopen"]["parent_exact"],
        "fork_reopen_exact": receipt["reopen"]["fork_exact"],
        "independent_editable_outputs": receipt["editable_output"]["parent_nonempty"] and receipt["editable_output"]["fork_nonempty"],
        "source_write_count": receipt["invariants"]["source_write_count"]
    }

def main():
    if len(sys.argv) != 2:
        print("usage: validate_project_fork_receipt.py RECEIPT.json", file=sys.stderr)
        return 2
    receipt = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
    validate_schema(receipt)
    print(json.dumps(validate_semantics(receipt), indent=2, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
