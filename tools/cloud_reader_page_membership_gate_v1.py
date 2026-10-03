#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

SUMMARY_SCHEMA = "chaptera.publisher-visual-fingerprint-compare.v1"
AUTHORITY_SCHEMA = "chaptera.publisher-page-membership-authority.v1"
GATE_SCHEMA = "chaptera.publisher-page-membership-gate.v1"
WARNING_STATE = "warning_membership_unknown"
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
SOURCE_SHA_RE = re.compile(r"^[0-9a-f]{64}$")


def page_ids_digest(page_ids: list[str] | None) -> str | None:
    if page_ids is None:
        return None
    if not isinstance(page_ids, list) or any(
        not isinstance(page_id, str) or not page_id for page_id in page_ids
    ):
        raise ValueError("source_page_ids must be a list of non-empty strings or null")
    canonical = "".join(page_id + "\n" for page_id in page_ids).encode("utf-8")
    return "sha256:" + hashlib.sha256(canonical).hexdigest()


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


def membership_changes(current: dict, baseline: dict) -> list[dict]:
    if current.get("batch_id") != baseline.get("batch_id"):
        raise ValueError("Batch01 identity mismatch")
    current_pairs = pair_map(current)
    baseline_pairs = pair_map(baseline)
    changes = []
    for fixture in sorted(set(current_pairs) & set(baseline_pairs)):
        now = current_pairs[fixture]
        before = baseline_pairs[fixture]
        if (
            now.get("reference_state") != WARNING_STATE
            or before.get("reference_state") != WARNING_STATE
            or now.get("rendered") is not True
            or before.get("rendered") is not True
        ):
            continue

        now_count = now.get("candidate_pages")
        before_count = before.get("candidate_pages")
        count_changed = now_count != before_count
        now_ids = now.get("source_page_ids")
        before_ids = before.get("source_page_ids")
        identity_available = isinstance(now_ids, list) and isinstance(before_ids, list)
        identity_changed = identity_available and now_ids != before_ids
        if not count_changed and not identity_changed:
            continue

        source_sha = now.get("source_sha256")
        if not isinstance(source_sha, str) or not SOURCE_SHA_RE.fullmatch(source_sha):
            raise ValueError(f"missing source SHA authority for {fixture}")
        changes.append({
            "fixture": fixture,
            "source_sha256": source_sha,
            "baseline_candidate_pages": before_count,
            "current_candidate_pages": now_count,
            "baseline_page_ids_sha256": page_ids_digest(before_ids) if identity_available else None,
            "current_page_ids_sha256": page_ids_digest(now_ids) if identity_available else None,
            "identity_comparison_available": identity_available,
            "page_count_changed": count_changed,
            "page_identity_or_order_changed": identity_changed,
        })
    return changes


def load_authorities(payload: dict) -> list[dict]:
    if payload.get("schema") != AUTHORITY_SCHEMA:
        raise ValueError(f"unsupported authority schema: {payload.get('schema')!r}")
    authorities = payload.get("authorities")
    if not isinstance(authorities, list):
        raise ValueError("authority entries must be a list")
    for entry in authorities:
        if not isinstance(entry, dict):
            raise ValueError("authority entry must be an object")
        if not isinstance(entry.get("fixture"), str) or not entry["fixture"]:
            raise ValueError("authority fixture must be non-empty")
        if not isinstance(entry.get("source_sha256"), str) or not SOURCE_SHA_RE.fullmatch(entry["source_sha256"]):
            raise ValueError("authority source_sha256 must be canonical")
        if not isinstance(entry.get("selector_profile_id"), str) or not entry["selector_profile_id"]:
            raise ValueError("authority selector_profile_id must be non-empty")
        if not isinstance(entry.get("evidence_issue"), int) or entry["evidence_issue"] <= 0:
            raise ValueError("authority evidence_issue must be a positive integer")
        if not isinstance(entry.get("evidence_digest"), str) or not SHA256_RE.fullmatch(entry["evidence_digest"]):
            raise ValueError("authority evidence_digest must be canonical sha256:<hex>")
        for key in ("baseline_page_ids_sha256", "current_page_ids_sha256"):
            if not isinstance(entry.get(key), str) or not SHA256_RE.fullmatch(entry[key]):
                raise ValueError(f"authority {key} must be canonical sha256:<hex>")
    return authorities


def authority_matches(change: dict, entry: dict) -> bool:
    keys = (
        "fixture",
        "source_sha256",
        "baseline_candidate_pages",
        "current_candidate_pages",
        "baseline_page_ids_sha256",
        "current_page_ids_sha256",
    )
    return all(change.get(key) == entry.get(key) for key in keys)


def evaluate(current: dict, baseline: dict, authority: dict) -> dict:
    changes = membership_changes(current, baseline)
    authorities = load_authorities(authority)
    accepted = []
    blocked = []
    for change in changes:
        if not change["identity_comparison_available"]:
            blocked.append({**change, "reason": "baseline_page_identity_unavailable"})
            continue
        matches = [entry for entry in authorities if authority_matches(change, entry)]
        if len(matches) != 1:
            blocked.append({
                **change,
                "reason": "missing_exact_membership_authority" if not matches else "ambiguous_membership_authority",
            })
            continue
        entry = matches[0]
        accepted.append({
            **change,
            "selector_profile_id": entry["selector_profile_id"],
            "evidence_issue": entry["evidence_issue"],
            "evidence_digest": entry["evidence_digest"],
        })

    return {
        "schema": GATE_SCHEMA,
        "baseline_repository_commit_sha": baseline.get("repository_commit_sha"),
        "current_repository_commit_sha": current.get("repository_commit_sha"),
        "membership_sensitive_change_count": len(changes),
        "accepted_change_count": len(accepted),
        "blocked_change_count": len(blocked),
        "accepted_changes": accepted,
        "blocked_changes": blocked,
        "status": "pass" if not blocked else "blocked",
        "claims": {
            "page_count_is_not_membership_authority": True,
            "visual_fingerprint_is_not_semantic_authority": True,
            "source_page_identity_transition_is_exactly_bound": True,
            "unproven_membership_change_fails_closed": True,
        },
    }


def self_test() -> None:
    def summary(commit: str, ids: list[str] | None, count: int) -> dict:
        return {
            "schema": SUMMARY_SCHEMA,
            "batch_id": "batch01",
            "repository_commit_sha": commit,
            "pairs": [{
                "fixture": "029_deadbeef",
                "source_sha256": "a" * 64,
                "source_page_ids": ids,
                "reference_state": WARNING_STATE,
                "rendered": True,
                "candidate_pages": count,
                "reference_pages": 2,
            }],
        }

    baseline = summary("base", ["root-a", "root-b", "page-a"], 3)
    current = summary("head", ["page-a", "page-b"], 2)
    empty = {"schema": AUTHORITY_SCHEMA, "authorities": []}
    blocked = evaluate(current, baseline, empty)
    assert blocked["status"] == "blocked"
    assert blocked["blocked_changes"][0]["reason"] == "missing_exact_membership_authority"

    change = membership_changes(current, baseline)[0]
    authority = {
        "schema": AUTHORITY_SCHEMA,
        "authorities": [{
            **{key: change[key] for key in (
                "fixture",
                "source_sha256",
                "baseline_candidate_pages",
                "current_candidate_pages",
                "baseline_page_ids_sha256",
                "current_page_ids_sha256",
            )},
            "selector_profile_id": "test/profile/v1",
            "evidence_issue": 1025,
            "evidence_digest": "sha256:" + "b" * 64,
        }],
    }
    passed = evaluate(current, baseline, authority)
    assert passed["status"] == "pass"
    assert passed["accepted_change_count"] == 1

    old_baseline = summary("old", None, 3)
    unavailable = evaluate(current, old_baseline, authority)
    assert unavailable["status"] == "blocked"
    assert unavailable["blocked_changes"][0]["reason"] == "baseline_page_identity_unavailable"

    unchanged = evaluate(current, current, empty)
    assert unchanged["status"] == "pass"
    assert unchanged["membership_sensitive_change_count"] == 0
    print("page membership gate self-test: ok")


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

    current = json.loads(args.current.read_text(encoding="utf-8"))
    baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
    authority = json.loads(args.authority.read_text(encoding="utf-8"))
    result = evaluate(current, baseline, authority)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "status": result["status"],
        "membership_sensitive_changes": result["membership_sensitive_change_count"],
        "accepted_changes": result["accepted_change_count"],
        "blocked_changes": result["blocked_change_count"],
    }, indent=2, sort_keys=True))
    if result["status"] != "pass":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
