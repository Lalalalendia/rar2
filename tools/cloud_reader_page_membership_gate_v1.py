#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

SUMMARY_SCHEMA = "chaptera.publisher-visual-fingerprint-compare.v1"
AUTHORITY_SCHEMA = "chaptera.publisher-page-membership-authority.v2"
GATE_SCHEMA = "chaptera.publisher-page-membership-gate.v2"
WARNING_STATE = "warning_membership_unknown"
LOGICAL_STAGE = "logical_page"
REFERENCE_SURFACE_STAGES = {
    "logical_page",
    "production_sheet",
    "viewport_spread",
    "unknown",
}
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
SOURCE_SHA_RE = re.compile(r"^[0-9a-f]{64}$")


def pair_map(summary: dict) -> dict[str, dict]:
    if summary.get("schema") != SUMMARY_SCHEMA:
        raise ValueError(f"unsupported summary schema: {summary.get('schema')!r}")
    rows = summary.get("pairs")
    if not isinstance(rows, list):
        raise ValueError("summary pairs must be a list")
    result = {}
    for row in rows:
        fixture = row.get("fixture")
        if not isinstance(fixture, str) or not fixture:
            raise ValueError("summary pair fixture must be non-empty")
        if fixture in result:
            raise ValueError(f"duplicate summary fixture: {fixture}")
        result[fixture] = row
    return result


def canonical_digest(value: object) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not SHA256_RE.fullmatch(value):
        raise ValueError(f"invalid PAGE identity order digest: {value!r}")
    return value


def canonical_stage(value: object) -> str:
    if value is None:
        return "unknown"
    if not isinstance(value, str) or value not in REFERENCE_SURFACE_STAGES:
        raise ValueError(f"invalid reference surface stage: {value!r}")
    return value


def membership_transitions(current: dict, baseline: dict) -> tuple[list[dict], list[dict]]:
    if current.get("batch_id") != baseline.get("batch_id"):
        raise ValueError("Batch01 identity mismatch")
    current_pairs = pair_map(current)
    baseline_pairs = pair_map(baseline)
    transitions = []
    bootstrap = []

    for fixture in sorted(set(current_pairs) & set(baseline_pairs)):
        now = current_pairs[fixture]
        before = baseline_pairs[fixture]
        if now.get("rendered") is not True or before.get("rendered") is not True:
            continue

        baseline_reference_state = before.get("reference_state")
        current_reference_state = now.get("reference_state")
        if baseline_reference_state != current_reference_state:
            raise ValueError(f"reference state drift across baseline for {fixture}")

        source_sha = now.get("source_sha256")
        if not isinstance(source_sha, str) or not SOURCE_SHA_RE.fullmatch(source_sha):
            raise ValueError(f"missing source SHA authority for {fixture}")
        baseline_source_sha = before.get("source_sha256")
        if baseline_source_sha is not None and baseline_source_sha != source_sha:
            raise ValueError(f"source SHA drift across baseline for {fixture}")

        baseline_stage = canonical_stage(before.get("reference_surface_stage"))
        current_stage = canonical_stage(now.get("reference_surface_stage"))
        before_count = before.get("candidate_pages")
        now_count = now.get("candidate_pages")
        count_changed = before_count != now_count
        before_digest = canonical_digest(before.get("candidate_page_identity_order_sha256"))
        now_digest = canonical_digest(now.get("candidate_page_identity_order_sha256"))

        if now_digest is None:
            raise ValueError(f"current summary lacks PAGE identity order digest for {fixture}")

        common = {
            "fixture": fixture,
            "source_sha256": source_sha,
            "reference_state": current_reference_state,
            "baseline_reference_surface_stage": baseline_stage,
            "current_reference_surface_stage": current_stage,
        }

        if before_digest is None:
            transitions.append({
                **common,
                "baseline_candidate_pages": before_count,
                "current_candidate_pages": now_count,
                "baseline_page_identity_order_sha256": None,
                "current_page_identity_order_sha256": now_digest,
                "page_count_changed": count_changed,
                "page_identity_or_order_changed": None,
                "identity_comparison_available": False,
            })
            continue

        identity_changed = before_digest != now_digest
        if not count_changed and not identity_changed:
            continue

        transitions.append({
            **common,
            "baseline_candidate_pages": before_count,
            "current_candidate_pages": now_count,
            "baseline_page_identity_order_sha256": before_digest,
            "current_page_identity_order_sha256": now_digest,
            "page_count_changed": count_changed,
            "page_identity_or_order_changed": identity_changed,
            "identity_comparison_available": True,
        })

    return transitions, bootstrap


def load_authorities(payload: dict) -> list[dict]:
    if payload.get("schema") != AUTHORITY_SCHEMA:
        raise ValueError(f"unsupported authority schema: {payload.get('schema')!r}")
    rows = payload.get("authorities")
    if not isinstance(rows, list):
        raise ValueError("authority entries must be a list")

    seen = set()
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("authority entry must be an object")
        if not isinstance(row.get("fixture"), str) or not row["fixture"]:
            raise ValueError("authority fixture must be non-empty")
        if (
            not isinstance(row.get("source_sha256"), str)
            or not SOURCE_SHA_RE.fullmatch(row["source_sha256"])
        ):
            raise ValueError("authority source_sha256 must be canonical")
        if row.get("reference_surface_stage") != LOGICAL_STAGE:
            raise ValueError("membership authority is valid only for logical_page references")
        if not isinstance(row.get("evidence_issue"), int) or row["evidence_issue"] <= 0:
            raise ValueError("authority evidence_issue must be a positive integer")
        if (
            not isinstance(row.get("evidence_digest"), str)
            or not SHA256_RE.fullmatch(row["evidence_digest"])
        ):
            raise ValueError("authority evidence_digest must be canonical sha256:<hex>")
        for key in (
            "baseline_page_identity_order_sha256",
            "current_page_identity_order_sha256",
        ):
            if not isinstance(row.get(key), str) or not SHA256_RE.fullmatch(row[key]):
                raise ValueError(f"authority {key} must be canonical sha256:<hex>")

        identity = (
            row["fixture"],
            row["source_sha256"],
            row["reference_surface_stage"],
            row["baseline_candidate_pages"],
            row["current_candidate_pages"],
            row["baseline_page_identity_order_sha256"],
            row["current_page_identity_order_sha256"],
        )
        if identity in seen:
            raise ValueError("duplicate PAGE membership authority transition")
        seen.add(identity)

    return rows


def authority_matches(transition: dict, authority: dict) -> bool:
    if (
        transition.get("baseline_reference_surface_stage") != LOGICAL_STAGE
        or transition.get("current_reference_surface_stage") != LOGICAL_STAGE
        or authority.get("reference_surface_stage") != LOGICAL_STAGE
    ):
        return False

    keys = (
        "fixture",
        "source_sha256",
        "baseline_candidate_pages",
        "current_candidate_pages",
        "baseline_page_identity_order_sha256",
        "current_page_identity_order_sha256",
    )
    return all(transition.get(key) == authority.get(key) for key in keys)


def evaluate(current: dict, baseline: dict, authority_payload: dict) -> dict:
    transitions, bootstrap = membership_transitions(current, baseline)
    authorities = load_authorities(authority_payload)
    accepted = []
    blocked = []

    for transition in transitions:
        if not transition["identity_comparison_available"]:
            blocked.append({
                **transition,
                "reason": "baseline_page_identity_unavailable",
            })
            continue

        if (
            transition["baseline_reference_surface_stage"] != LOGICAL_STAGE
            or transition["current_reference_surface_stage"] != LOGICAL_STAGE
        ):
            blocked.append({
                **transition,
                "reason": "reference_surface_stage_not_logical_page",
            })
            continue

        matches = [row for row in authorities if authority_matches(transition, row)]
        if len(matches) != 1:
            blocked.append({
                **transition,
                "reason": (
                    "missing_exact_membership_authority"
                    if not matches
                    else "ambiguous_membership_authority"
                ),
            })
            continue

        authority = matches[0]
        accepted.append({
            **transition,
            "reference_surface_stage": LOGICAL_STAGE,
            "evidence_issue": authority["evidence_issue"],
            "evidence_digest": authority["evidence_digest"],
        })

    return {
        "schema": GATE_SCHEMA,
        "baseline_repository_commit_sha": baseline.get("repository_commit_sha"),
        "current_repository_commit_sha": current.get("repository_commit_sha"),
        "membership_sensitive_transition_count": len(transitions),
        "accepted_transition_count": len(accepted),
        "blocked_transition_count": len(blocked),
        "bootstrap_identity_count": len(bootstrap),
        "accepted_transitions": accepted,
        "blocked_transitions": blocked,
        "bootstrap_identity_rows": bootstrap,
        "status": "pass" if not blocked else "blocked",
        "claims": {
            "page_count_is_not_membership_authority": True,
            "visual_fingerprint_is_not_semantic_authority": True,
            "reference_surface_stage_is_independent_authority": True,
            "membership_authority_applies_only_to_logical_page_stage": True,
            "production_sheet_count_cannot_authorize_logical_page_transition": True,
            "viewport_spread_count_cannot_authorize_logical_page_transition": True,
            "unknown_stage_cannot_authorize_logical_page_transition": True,
            "raw_page_id_emitted": False,
            "ordered_page_identity_transition_is_exactly_bound": True,
            "unproven_membership_transition_fails_closed": True,
        },
    }


def self_test() -> None:
    source_sha = "a" * 64
    old_digest = "sha256:" + "1" * 64
    new_digest = "sha256:" + "2" * 64

    def summary(
        commit: str,
        count: int,
        digest: str | None,
        stage: str = LOGICAL_STAGE,
    ) -> dict:
        return {
            "schema": SUMMARY_SCHEMA,
            "batch_id": "batch01",
            "repository_commit_sha": commit,
            "pairs": [{
                "fixture": "029_test",
                "source_sha256": source_sha,
                "candidate_page_identity_order_sha256": digest,
                "reference_state": WARNING_STATE,
                "reference_surface_stage": stage,
                "rendered": True,
                "candidate_pages": count,
                "reference_pages": 2,
            }],
        }

    empty = {"schema": AUTHORITY_SCHEMA, "authorities": []}
    baseline = summary("base", 9, old_digest)
    current = summary("head", 2, new_digest)

    blocked = evaluate(current, baseline, empty)
    assert blocked["status"] == "blocked"
    assert blocked["blocked_transitions"][0]["reason"] == "missing_exact_membership_authority"

    authority = {
        "schema": AUTHORITY_SCHEMA,
        "authorities": [{
            "fixture": "029_test",
            "source_sha256": source_sha,
            "reference_surface_stage": LOGICAL_STAGE,
            "baseline_candidate_pages": 9,
            "current_candidate_pages": 2,
            "baseline_page_identity_order_sha256": old_digest,
            "current_page_identity_order_sha256": new_digest,
            "evidence_issue": 1025,
            "evidence_digest": "sha256:" + "3" * 64,
        }],
    }
    passed = evaluate(current, baseline, authority)
    assert passed["status"] == "pass"
    assert passed["accepted_transition_count"] == 1

    for stage in ("unknown", "production_sheet", "viewport_spread"):
        wrong_stage = evaluate(
            summary("head", 2, new_digest, stage),
            summary("base", 9, old_digest, stage),
            authority,
        )
        assert wrong_stage["status"] == "blocked"
        assert (
            wrong_stage["blocked_transitions"][0]["reason"]
            == "reference_surface_stage_not_logical_page"
        )

    stage_cross = evaluate(
        summary("head", 2, new_digest, LOGICAL_STAGE),
        summary("base", 9, old_digest, "unknown"),
        authority,
    )
    assert stage_cross["status"] == "blocked"
    assert (
        stage_cross["blocked_transitions"][0]["reason"]
        == "reference_surface_stage_not_logical_page"
    )

    old_same_count = evaluate(
        summary("head", 9, new_digest, LOGICAL_STAGE),
        summary("old", 9, None, LOGICAL_STAGE),
        empty,
    )
    assert old_same_count["status"] == "blocked"
    assert old_same_count["bootstrap_identity_count"] == 0
    assert (
        old_same_count["blocked_transitions"][0]["reason"]
        == "baseline_page_identity_unavailable"
    )

    old_count_change = evaluate(
        summary("head", 2, new_digest, LOGICAL_STAGE),
        summary("old", 9, None, LOGICAL_STAGE),
        empty,
    )
    assert old_count_change["status"] == "blocked"
    assert (
        old_count_change["blocked_transitions"][0]["reason"]
        == "baseline_page_identity_unavailable"
    )

    non_membership_warning = summary("head", 2, new_digest, LOGICAL_STAGE)
    non_membership_warning["pairs"][0]["reference_state"] = "empty_visual_output"
    non_membership_baseline = summary("base", 9, old_digest, LOGICAL_STAGE)
    non_membership_baseline["pairs"][0]["reference_state"] = "empty_visual_output"
    non_membership_blocked = evaluate(non_membership_warning, non_membership_baseline, empty)
    assert non_membership_blocked["status"] == "blocked"
    assert (
        non_membership_blocked["blocked_transitions"][0]["reason"]
        == "missing_exact_membership_authority"
    )

    unchanged = evaluate(current, current, empty)
    assert unchanged["status"] == "pass"
    assert unchanged["membership_sensitive_transition_count"] == 0

    print("page membership stage-aware hard gate self-test: ok")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--current", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--authority", type=Path)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if None in (args.current, args.baseline, args.authority, args.out):
        parser.error("--current, --baseline, --authority and --out are required unless --self-test")

    result = evaluate(
        json.loads(args.current.read_text(encoding="utf-8")),
        json.loads(args.baseline.read_text(encoding="utf-8")),
        json.loads(args.authority.read_text(encoding="utf-8")),
    )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({
        "status": result["status"],
        "membership_sensitive_transitions": result["membership_sensitive_transition_count"],
        "accepted_transitions": result["accepted_transition_count"],
        "blocked_transitions": result["blocked_transition_count"],
        "bootstrap_identity_rows": result["bootstrap_identity_count"],
    }, indent=2, sort_keys=True))

    if result["status"] != "pass":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
