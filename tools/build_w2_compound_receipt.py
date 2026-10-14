#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib

from jsonschema import Draft202012Validator

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "packages" / "protocol" / "w2-compound" / "v1" / "receipt.schema.json"
SOURCE_SHA = "aeac4c03181582008c18655ad77b90a957b00eeddcc0b1c45f6d40ca88c765eb"
SOURCE_BYTES = 4_286_464


def read(path: pathlib.Path) -> tuple[bytes, dict]:
    raw = path.read_bytes()
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise SystemExit(f"{path} must contain a JSON object")
    return raw, value


def sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def require(value: bool, message: str) -> None:
    if not value:
        raise SystemExit(message)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kernel", required=True, type=pathlib.Path)
    parser.add_argument("--fixed-output", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    kernel_raw, kernel = read(args.kernel)
    fixed_raw, fixed = read(args.fixed_output)

    require(
        kernel.get("receipt_version") == "chaptera.w2-same-document-kernel.v1",
        "unexpected W2 same-document kernel receipt",
    )
    ksource = kernel.get("source")
    require(
        isinstance(ksource, dict)
        and ksource.get("sha256") == SOURCE_SHA
        and ksource.get("byte_len") == SOURCE_BYTES
        and ksource.get("immutable") is True,
        "W2 kernel source identity is not the exact May-2023 fixture",
    )
    parent = kernel.get("parent")
    require(
        isinstance(parent, dict) and parent.get("operation_count") == 0,
        "W2 parent must be the unmodified imported previous issue",
    )
    fork = kernel.get("fork_initial")
    require(
        isinstance(fork, dict)
        and fork.get("initial_state_preserved") is True
        and fork.get("identity_rekeyed") is True
        and fork.get("provenance_exact") is True,
        "W2 canonical fork invariants are incomplete",
    )
    next_issue = kernel.get("next_issue")
    require(
        isinstance(next_issue, dict)
        and next_issue.get("operation_count") == 4
        and next_issue.get("recipe_state_equal") is True
        and next_issue.get("recipe_operations_equal") is True
        and next_issue.get("recipe_assets_equal") is True
        and next_issue.get("fork_identity_preserved") is True
        and next_issue.get("fresh_reopen_exact") is True,
        "W2 next issue does not reproduce the accepted edit recipe exactly",
    )
    next_state = next_issue.get("state_id")
    require(
        isinstance(next_state, str)
        and next_state.startswith("sha256:")
        and next_state == next_issue.get("recipe_state_id"),
        "W2 next-issue state identity is missing or differs from the accepted recipe",
    )
    layout = kernel.get("layout")
    require(
        isinstance(layout, dict)
        and layout.get("explicit") is True
        and layout.get("state") in {"fits", "overset"},
        "W2 Story layout state is not explicit",
    )
    editable = kernel.get("editable_output")
    require(
        isinstance(editable, dict)
        and editable.get("format") == "odg"
        and editable.get("nonempty") is True
        and isinstance(editable.get("byte_len"), int)
        and editable["byte_len"] > 0,
        "W2 next-issue editable output is missing",
    )
    kinv = kernel.get("invariants")
    require(
        isinstance(kinv, dict)
        and kinv.get("same_document") is True
        and kinv.get("parent_unchanged_after_next_issue_edit") is True
        and kinv.get("source_pub_immutable") is True
        and kinv.get("source_write_count") == 0
        and kinv.get("wrap_preservation_claimed") is False,
        "W2 same-document invariants are incomplete",
    )

    require(
        fixed.get("receipt_version")
        == "chaptera.editor-fixed-pdf-current-revision-receipt.v2",
        "unexpected fixed-output receipt",
    )
    fsource = fixed.get("source")
    require(
        isinstance(fsource, dict)
        and fsource.get("sha256") == SOURCE_SHA
        and fsource.get("byte_len") == SOURCE_BYTES
        and fsource.get("immutable") is True,
        "fixed-output source identity differs from W2 fixture",
    )
    current = fixed.get("current_revision")
    require(
        isinstance(current, dict)
        and current.get("project_state_id") == next_state
        and current.get("mutation_target_count") == 4
        and current.get("story_target_state_current") is True
        and current.get("move_geometry_current") is True
        and current.get("resize_geometry_current") is True
        and current.get("replacement_image_current") is True,
        "fixed output is not bound to the exact forked next-issue effective state",
    )
    renderer = fixed.get("renderer")
    summary = renderer.get("summary") if isinstance(renderer, dict) else None
    require(
        isinstance(summary, dict) and summary.get("page_count") == 10,
        "W2 fixed output must contain exactly 10 customer-visible pages",
    )
    finv = fixed.get("invariants")
    require(
        isinstance(finv, dict)
        and finv.get("source_pub_immutable") is True
        and finv.get("source_reparse_after_edit_count") == 0
        and finv.get("current_editor_project_authoritative") is True,
        "fixed-output current-state invariants are incomplete",
    )
    artifact = fixed.get("artifact")
    require(
        isinstance(artifact, dict)
        and artifact.get("format") == "pdf"
        and isinstance(artifact.get("sha256"), str)
        and len(artifact["sha256"]) == 64,
        "W2 fixed PDF artifact identity is missing",
    )

    receipt = {
        "receipt_version": "chaptera.w2-compound-receipt.v1",
        "evidence_mode": "same_document",
        "source": {
            "sha256": SOURCE_SHA,
            "byte_len": SOURCE_BYTES,
        },
        "kernel": {
            "receipt_sha256": sha(kernel_raw),
            "next_issue_state_id": next_state,
            "layout_state": layout["state"],
            "validated": True,
        },
        "fixed_output": {
            "receipt_sha256": sha(fixed_raw),
            "project_state_id": current["project_state_id"],
            "page_count": summary["page_count"],
            "artifact_sha256": artifact["sha256"],
            "validated": True,
        },
        "invariants": {
            "same_document_claim": True,
            "source_immutable": True,
            "fork_initial_state_preserved": True,
            "fork_identity_rekeyed": True,
            "fork_provenance_exact": True,
            "parent_unchanged": True,
            "next_issue_recipe_state_equal": True,
            "next_issue_fresh_reopen_exact": True,
            "editable_output_proven": True,
            "layout_state_explicit": True,
            "fixed_output_state_matches_next_issue": True,
            "fixed_output_page_count_correct": True,
            "wrap_preservation_claimed": False,
            "source_write_count": 0,
        },
    }

    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    errors = sorted(
        Draft202012Validator(schema).iter_errors(receipt),
        key=lambda error: list(error.path),
    )
    if errors:
        raise SystemExit(
            "\n".join(f"{list(error.path)}: {error.message}" for error in errors)
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
