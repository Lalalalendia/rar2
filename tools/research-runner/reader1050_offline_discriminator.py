#!/usr/bin/env python3
"""Execute one bounded offline discriminator for the Reader-1050 frontier.

The input frontier already chooses one unsupported Reader witness. This layer
does not choose a new global task. It compares that witness with successfully
opened controls from the same 1050 receipt and closes the strongest structural
hypothesis that the retained evidence can actually decide.

If an opened control has identical logical streams, the tool additionally runs
the existing physical-CFB comparator on the exact pair and retains only a
source-free summary of the physical differences.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "tools" / "corpus"))

from cfb_physical_diff import compare as physical_compare  # noqa: E402

SCHEMA = "chaptera.reader1050-offline-discriminator.v1"

KNOWN_STREAMS = {
    "contents": "/Contents",
    "quill": "/Quill/QuillSub/CONTENTS",
    "escher": "/Escher/EscherStm",
    "escher_delay": "/Escher/EscherDelayStm",
}


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def find_unique(root: Path, name: str) -> Path:
    matches = sorted(root.rglob(name))
    if len(matches) != 1:
        raise ValueError(f"expected exactly one {name} below {root}, got {len(matches)}")
    return matches[0]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def forced_trigger_classification(
    selected_acceptance: dict[str, Any],
) -> tuple[str, list[str], dict[str, Any]] | None:
    if selected_acceptance.get("salvage_eligibility") != "awaiting_typed_corruption_evidence":
        return None

    probe = selected_acceptance.get("forced_trigger_probe")
    graph = selected_acceptance.get("forced_partial_graph")
    if not isinstance(probe, dict) or not isinstance(graph, dict):
        return None

    if not probe.get("cfb_inventory_available"):
        return (
            "pub_cfb_inventory_failure_under_forced_trigger",
            ["intake_policy_gate_only_explanation"],
            {
                "kind": "pub_cfb_inventory_error_probe",
                "reason": (
                    "even the diagnostic ProvenStructuralCorruption trigger cannot build the Reader CFB inventory; "
                    "localize the typed pub_cfb inspect error on this exact witness"
                ),
                "target_carriers": [],
            },
        )

    if graph.get("status") != "constructed":
        return (
            "partial_source_graph_failure_under_forced_trigger",
            ["pub_cfb_inventory_failure"],
            {
                "kind": "partial_source_graph_error_probe",
                "reason": (
                    "the forced trigger reaches Reader CFB inventory, but partial salvage graph construction still fails"
                ),
                "error": graph.get("error"),
                "target_carriers": [],
            },
        )

    closed = ["pub_cfb_inventory_failure", "partial_source_graph_unbuildable"]
    if probe.get("has_surviving_evidence"):
        closed.append("no_surviving_salvage_evidence")
    return (
        "salvage_path_reachable_if_typed_corruption_were_proven",
        closed,
        {
            "kind": "typed_corruption_evidence_discovery",
            "reason": (
                "the same witness reaches Reader CFB inventory and constructs a partial salvage graph under the "
                "diagnostic ProvenStructuralCorruption trigger; the remaining admission requirement is independent "
                "typed corruption evidence, not container readability"
            ),
            "forced_contents_family": probe.get("contents_family"),
            "forced_subsystems": probe.get("subsystems") or {},
            "forced_fact_counts": graph.get("fact_counts") or {},
            "forced_gap_count": graph.get("gap_count"),
            "target_carriers": [],
        },
    )


def index_rows(rows: list[dict[str, Any]], key: str) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        value = str(row.get(key) or "").lower()
        if value:
            out[value] = row
    return out


def stream_map(fingerprint: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        str(row.get("path")): row
        for row in fingerprint.get("streams") or []
        if isinstance(row, dict) and row.get("path")
    }


def known_stream_projection(fingerprint: dict[str, Any]) -> dict[str, dict[str, Any] | None]:
    streams = stream_map(fingerprint)
    return {
        key: (
            {
                "len": int(streams[path].get("len") or 0),
                "size_bucket_log2": int(streams[path].get("size_bucket_log2") or 0),
                "sha256": str(streams[path].get("sha256") or ""),
            }
            if path in streams
            else None
        )
        for key, path in KNOWN_STREAMS.items()
    }


def relation(selected: dict[str, Any], control: dict[str, Any]) -> tuple[int, str]:
    if (
        selected.get("content_topology_fingerprint_sha256")
        and selected.get("content_topology_fingerprint_sha256")
        == control.get("content_topology_fingerprint_sha256")
    ):
        return 5, "identical_logical_streams"
    if (
        selected.get("topology_fingerprint_sha256")
        and selected.get("topology_fingerprint_sha256")
        == control.get("topology_fingerprint_sha256")
    ):
        return 4, "identical_stream_paths_and_lengths"
    if (
        selected.get("size_bucket_fingerprint_sha256")
        and selected.get("size_bucket_fingerprint_sha256")
        == control.get("size_bucket_fingerprint_sha256")
    ):
        return 3, "identical_stream_paths_and_size_buckets"
    if (
        selected.get("path_fingerprint_sha256")
        and selected.get("path_fingerprint_sha256")
        == control.get("path_fingerprint_sha256")
    ):
        return 2, "identical_stream_paths"
    if (selected.get("carrier_flags") or {}) == (control.get("carrier_flags") or {}):
        return 1, "identical_major_carrier_set"
    return 0, "different_major_carrier_set"


def relative_size_distance(left: int, right: int) -> float:
    if left <= 0 or right <= 0:
        return 10.0 if left != right else 0.0
    return abs(math.log2(left / right))


def distance(
    selected_fp: dict[str, Any],
    control_fp: dict[str, Any],
    selected_acceptance: dict[str, Any],
    control_acceptance: dict[str, Any],
) -> float:
    score = 0.0
    if selected_fp.get("contents_family") != control_fp.get("contents_family"):
        score += 500.0
    if (
        selected_fp.get("contents_serialization_revision")
        != control_fp.get("contents_serialization_revision")
    ):
        score += 120.0
    if selected_acceptance.get("pub_profile") != control_acceptance.get("pub_profile"):
        score += 80.0
    if selected_acceptance.get("reader_route") != control_acceptance.get("reader_route"):
        score += 40.0

    selected_flags = selected_fp.get("carrier_flags") or {}
    control_flags = control_fp.get("carrier_flags") or {}
    for key in sorted(set(selected_flags) | set(control_flags)):
        if bool(selected_flags.get(key)) != bool(control_flags.get(key)):
            score += 60.0

    score += abs(int(selected_fp.get("stream_count") or 0) - int(control_fp.get("stream_count") or 0)) * 5.0
    score += abs(int(selected_fp.get("storage_count") or 0) - int(control_fp.get("storage_count") or 0)) * 3.0
    score += relative_size_distance(
        int(selected_fp.get("byte_len") or 0),
        int(control_fp.get("byte_len") or 0),
    ) * 20.0

    left_known = known_stream_projection(selected_fp)
    right_known = known_stream_projection(control_fp)
    for key in KNOWN_STREAMS:
        left = left_known[key]
        right = right_known[key]
        if (left is None) != (right is None):
            score += 80.0
        elif left is not None and right is not None:
            score += abs(
                int(left["size_bucket_log2"]) - int(right["size_bucket_log2"])
            ) * 4.0

    relation_rank, _ = relation(selected_fp, control_fp)
    score -= relation_rank * 100.0
    return round(score, 6)


def classify(
    selected_fp: dict[str, Any],
    control_fp: dict[str, Any],
) -> tuple[str, list[str], dict[str, Any]]:
    relation_rank, relation_name = relation(selected_fp, control_fp)
    selected_known = known_stream_projection(selected_fp)
    control_known = known_stream_projection(control_fp)

    differing_known_payloads = []
    differing_known_lengths = []
    missing_known = []
    for key in KNOWN_STREAMS:
        left = selected_known[key]
        right = control_known[key]
        if left is None or right is None:
            if left is not right:
                missing_known.append(key)
            continue
        if left["len"] != right["len"]:
            differing_known_lengths.append(key)
        if left["sha256"] != right["sha256"]:
            differing_known_payloads.append(key)

    if relation_rank == 5:
        return (
            "physical_or_directory_divergence",
            [
                "logical_stream_presence",
                "logical_stream_lengths",
                "logical_stream_payloads",
            ],
            {
                "kind": "physical_cfb_diff",
                "reason": (
                    "an opened control has identical logical stream paths, lengths, and payload hashes; "
                    "the next discriminator is physical CFB/directory allocation"
                ),
                "target_carriers": [],
            },
        )
    if relation_rank == 4:
        return (
            "stream_content_divergence",
            ["stream_presence", "stream_lengths", "gross_container_topology"],
            {
                "kind": "carrier_semantic_diff",
                "reason": (
                    "an opened control has identical stream paths and lengths but different payloads; "
                    "container topology is not sufficient to explain the failure"
                ),
                "target_carriers": differing_known_payloads,
            },
        )
    if relation_rank == 3:
        return (
            "bounded_stream_semantic_divergence",
            ["stream_presence", "stream_size_class"],
            {
                "kind": "stream_framing_probe",
                "reason": (
                    "an opened control has the same stream paths and size buckets; "
                    "localize exact framing/content differences before changing salvage policy"
                ),
                "target_carriers": sorted(
                    set(differing_known_lengths) | set(differing_known_payloads)
                ),
            },
        )
    if relation_rank == 2:
        return (
            "stream_size_or_content_divergence",
            ["stream_presence"],
            {
                "kind": "stream_size_and_framing_probe",
                "reason": (
                    "an opened control has the same stream path inventory; "
                    "the remaining structural discriminator is stream sizing/content"
                ),
                "target_carriers": sorted(
                    set(differing_known_lengths) | set(differing_known_payloads)
                ),
            },
        )
    if relation_rank == 1:
        return (
            "storage_topology_divergence",
            ["major_carrier_absence"],
            {
                "kind": "storage_topology_probe",
                "reason": (
                    "major Publisher carriers match an opened control, but the full stream path inventory differs"
                ),
                "target_carriers": missing_known,
            },
        )
    return (
        "structural_outlier",
        [],
        {
            "kind": "carrier_inventory_probe",
            "reason": (
                "no opened control shares the major carrier set; keep the next step at source-free "
                "inventory/family classification rather than inferring corruption semantics"
            ),
            "target_carriers": missing_known,
        },
    )


def locate_pub(corpus_root: Path, source_sha256: str) -> Path:
    matches = sorted(corpus_root.rglob(f"{source_sha256}.pub"))
    if len(matches) != 1:
        raise ValueError(
            f"expected exactly one corpus PUB for {source_sha256}, got {len(matches)}"
        )
    return matches[0]


def physical_diff_summary(
    corpus_root: Path,
    selected_sha: str,
    control_sha: str,
) -> dict[str, Any]:
    left_path = locate_pub(corpus_root, selected_sha)
    right_path = locate_pub(corpus_root, control_sha)
    left = left_path.read_bytes()
    right = right_path.read_bytes()
    if sha256_bytes(left) != selected_sha or sha256_bytes(right) != control_sha:
        raise ValueError("corpus SHA identity drift before physical diff")

    if len(left) != len(right):
        return {
            "status": "not_run",
            "reason": "physical comparator requires equal byte length",
            "selected_byte_len": len(left),
            "control_byte_len": len(right),
        }

    result = physical_compare(left, right, "reader1050-frontier-pair")
    changed_directory_fields = sorted(
        {
            field
            for row in result.get("directory_metadata_diffs") or []
            for field in (row.get("changed_fields") or {}).keys()
        }
    )
    return {
        "status": "completed",
        "different_byte_count": result["different_byte_count"],
        "classified_byte_count": result["classified_byte_count"],
        "unclassified_byte_count": result["unclassified_byte_count"],
        "broad_category_counts": result["broad_category_counts"],
        "directory_metadata_diff_count": len(result["directory_metadata_diffs"]),
        "changed_directory_fields": changed_directory_fields,
        "fat_table_diff_entry_count": len(result["fat_table_diff_entries"]),
        "minifat_table_diff_entry_count": len(result["minifat_table_diff_entries"]),
        "stream_chain_diff_count": len(result["stream_chain_diffs"]),
        "fat_sector_ids_equal": result["fat_sector_ids_equal"],
        "difat_sector_ids_equal": result["difat_sector_ids_equal"],
        "directory_sector_ids_equal": result["directory_sector_ids_equal"],
        "minifat_sector_ids_equal": result["minifat_sector_ids_equal"],
        "root_chain_equal": result["root_chain_equal"],
    }


def build_discriminator(
    reader_root: Path,
    frontier_path: Path,
    out_root: Path,
    corpus_root: Path | None,
) -> dict[str, Any]:
    frontier = read_json(frontier_path)
    selected = frontier.get("selected")
    if not isinstance(selected, dict):
        payload = {
            "schema": SCHEMA,
            "status": "exhausted",
            "source_reader_run_id": frontier.get("source_reader_run_id"),
            "selected_sha256": None,
            "decision": "stop",
            "reason": "frontier has no selected unsupported witness",
        }
        out_root.mkdir(parents=True, exist_ok=True)
        (out_root / "decision.json").write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        return payload

    selected_sha = str(selected["source_sha256"]).lower()
    existing_owner = selected.get("existing_owner")
    if isinstance(existing_owner, dict):
        payload = {
            "schema": SCHEMA,
            "status": "existing_owner_handoff",
            "source_reader_run_id": frontier.get("source_reader_run_id"),
            "source_main_sha": frontier.get("source_main_sha"),
            "selected_sha256": selected_sha,
            "control_sha256": None,
            "control_relation": "existing_owner",
            "control_distance": None,
            "verdict": "known_format_gap_not_corruption_candidate",
            "closed_hypotheses": ["unowned_reader_failure", "generic_corruption_route"],
            "next_discriminator": {
                "kind": "handoff_existing_owner",
                "owner": existing_owner.get("owner"),
                "route": existing_owner.get("route"),
                "reason": (
                    "this exact witness already has a grounded format-research owner; "
                    "normal Reader failure and diagnostic forced salvage reachability do not "
                    "constitute independent corruption evidence"
                ),
                "target_carriers": [],
            },
            "physical_diff": None,
            "control_shortlist": [],
            "decision": "handoff_existing_owner",
            "evidence_boundary": (
                "source-free routing only; no new corruption classification is inferred "
                "from forced-trigger diagnostics"
            ),
        }
        out_root.mkdir(parents=True, exist_ok=True)
        (out_root / "decision.json").write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        (out_root / "decision.md").write_text(
            render_markdown(payload),
            encoding="utf-8",
        )
        return payload
    fingerprints = index_rows(
        read_json(find_unique(reader_root, "fingerprints.json")),
        "sha256",
    )
    reader_rows = index_rows(
        read_json(find_unique(reader_root, "reader-records.json")),
        "source_sha256",
    )
    acceptance_doc = read_json(find_unique(reader_root, "salvage-acceptance.json"))
    acceptance = index_rows(acceptance_doc.get("rows") or [], "source_sha256")

    if selected_sha not in fingerprints:
        raise ValueError(f"selected SHA missing from fingerprints: {selected_sha}")
    if selected_sha not in acceptance:
        raise ValueError(f"selected SHA missing from salvage acceptance: {selected_sha}")
    if acceptance[selected_sha].get("outcome") != "unsupported":
        raise ValueError("frontier selected row is no longer unsupported")

    selected_fp = fingerprints[selected_sha]
    selected_acceptance = acceptance[selected_sha]

    forced = forced_trigger_classification(selected_acceptance)
    if forced is not None:
        verdict, closed_hypotheses, next_discriminator = forced
        payload = {
            "schema": SCHEMA,
            "status": "discriminator_executed",
            "source_reader_run_id": frontier.get("source_reader_run_id"),
            "source_main_sha": frontier.get("source_main_sha"),
            "selected_sha256": selected_sha,
            "control_sha256": None,
            "control_relation": "same_witness_forced_trigger",
            "control_distance": None,
            "verdict": verdict,
            "closed_hypotheses": closed_hypotheses,
            "next_discriminator": next_discriminator,
            "physical_diff": None,
            "control_shortlist": [],
            "decision": "continue_offline",
            "evidence_boundary": (
                "source-free discriminator: the forced trigger is diagnostic only and does not alter "
                "Reader admission policy; outputs retain no filenames, document text, raw stream bytes, "
                "or PUB bytes"
            ),
        }
        out_root.mkdir(parents=True, exist_ok=True)
        (out_root / "decision.json").write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        (out_root / "decision.md").write_text(
            render_markdown(payload),
            encoding="utf-8",
        )
        return payload

    candidates = []
    for sha, control_reader in reader_rows.items():
        if sha == selected_sha or control_reader.get("opened") is not True:
            continue
        control_fp = fingerprints.get(sha)
        control_acceptance = acceptance.get(sha)
        if control_fp is None or control_acceptance is None:
            continue
        relation_rank, relation_name = relation(selected_fp, control_fp)
        candidates.append(
            {
                "source_sha256": sha,
                "relation_rank": relation_rank,
                "relation": relation_name,
                "distance": distance(
                    selected_fp,
                    control_fp,
                    selected_acceptance,
                    control_acceptance,
                ),
                "contents_family": control_fp.get("contents_family"),
                "contents_serialization_revision": control_fp.get(
                    "contents_serialization_revision"
                ),
                "pub_profile": control_acceptance.get("pub_profile"),
            }
        )

    if not candidates:
        raise ValueError("no successfully opened Reader-1050 controls are available")

    candidates.sort(
        key=lambda row: (
            -int(row["relation_rank"]),
            float(row["distance"]),
            row["source_sha256"],
        )
    )
    chosen = candidates[0]
    control_sha = str(chosen["source_sha256"])
    control_fp = fingerprints[control_sha]

    verdict, closed_hypotheses, next_discriminator = classify(
        selected_fp,
        control_fp,
    )

    physical = None
    if (
        next_discriminator["kind"] == "physical_cfb_diff"
        and corpus_root is not None
    ):
        physical = physical_diff_summary(corpus_root, selected_sha, control_sha)
        if physical.get("status") == "completed":
            next_discriminator = {
                "kind": "reader_physical_cfb_localization",
                "reason": (
                    "logical streams are identical and the physical CFB diff was completed; "
                    "instrument the Reader path that consumes the changed physical categories"
                ),
                "target_physical_categories": sorted(
                    (physical.get("broad_category_counts") or {}).keys()
                ),
                "target_carriers": [],
            }

    payload = {
        "schema": SCHEMA,
        "status": "discriminator_executed",
        "source_reader_run_id": frontier.get("source_reader_run_id"),
        "source_main_sha": frontier.get("source_main_sha"),
        "selected_sha256": selected_sha,
        "control_sha256": control_sha,
        "control_relation": chosen["relation"],
        "control_distance": chosen["distance"],
        "verdict": verdict,
        "closed_hypotheses": closed_hypotheses,
        "next_discriminator": next_discriminator,
        "physical_diff": physical,
        "control_shortlist": candidates[:5],
        "decision": "continue_offline",
        "evidence_boundary": (
            "source-free discriminator: outputs retain hashes/counts/classifications only; "
            "no filenames, document text, raw stream bytes, or PUB bytes"
        ),
    }

    out_root.mkdir(parents=True, exist_ok=True)
    (out_root / "decision.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (out_root / "decision.md").write_text(
        render_markdown(payload),
        encoding="utf-8",
    )
    return payload


def render_markdown(payload: dict[str, Any]) -> str:
    if payload.get("status") == "exhausted":
        return (
            "# Reader-1050 offline discriminator\n\n"
            "- Status: **exhausted**\n"
            "- Decision: **stop**\n"
        )

    next_step = payload["next_discriminator"]
    lines = [
        "# Reader-1050 offline discriminator",
        "",
        f"- Selected witness: `{payload['selected_sha256']}`",
        (
            f"- Opened control: `{payload['control_sha256']}`"
            if payload.get("control_sha256")
            else "- Opened control: **not needed; same-witness discriminator won**"
        ),
        f"- Control relation: `{payload['control_relation']}`",
        f"- Verdict: **{payload['verdict']}**",
        f"- Decision: **{payload['decision']}**",
        "",
        "## Closed hypotheses",
        "",
    ]
    if payload["closed_hypotheses"]:
        lines.extend(f"- `{item}`" for item in payload["closed_hypotheses"])
    else:
        lines.append("- none")

    lines.extend(
        [
            "",
            "## Next bounded discriminator",
            "",
            f"- Kind: `{next_step['kind']}`",
            f"- Reason: {next_step['reason']}",
        ]
    )
    for key in ("target_carriers", "target_physical_categories"):
        if next_step.get(key):
            lines.append(f"- {key}: " + ", ".join(f"`{x}`" for x in next_step[key]))

    physical = payload.get("physical_diff")
    if isinstance(physical, dict):
        lines.extend(
            [
                "",
                "## Physical pair discriminator",
                "",
                f"- Status: `{physical.get('status')}`",
            ]
        )
        if physical.get("status") == "completed":
            lines.extend(
                [
                    f"- Different bytes: **{physical.get('different_byte_count')}**",
                    f"- Unclassified changed bytes: **{physical.get('unclassified_byte_count')}**",
                    "- Categories: "
                    + ", ".join(
                        f"`{key}`={value}"
                        for key, value in sorted(
                            (physical.get("broad_category_counts") or {}).items()
                        )
                    ),
                ]
            )
        else:
            lines.append(f"- Reason: {physical.get('reason')}")

    lines.extend(
        [
            "",
            "## Boundary",
            "",
            payload["evidence_boundary"] + ".",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reader-root", type=Path, required=True)
    parser.add_argument("--frontier", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--corpus-root", type=Path)
    args = parser.parse_args()

    payload = build_discriminator(
        args.reader_root,
        args.frontier,
        args.out,
        args.corpus_root,
    )
    print(
        json.dumps(
            {
                "status": payload["status"],
                "selected_sha256": payload.get("selected_sha256"),
                "control_sha256": payload.get("control_sha256"),
                "verdict": payload.get("verdict"),
                "next_discriminator": (
                    payload.get("next_discriminator") or {}
                ).get("kind"),
                "decision": payload.get("decision"),
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
