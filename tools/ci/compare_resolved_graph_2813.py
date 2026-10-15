#!/usr/bin/env python3
"""Source-neutral, strict comparison of two physical canonical CDM graphs.

Accepts only the pinned public SampleNewsletter fixture. Never emit node IDs,
Story text, source names, geometry values, user strings, or raw JSON. Reports
bounded counts and named invariant failures. A new graph pin may be approved
only after a human inspects this physical, source-safe result.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

REGISTRIES = (
    "pages", "nodes", "stories", "paragraphs", "text_runs",
    "resources", "styles", "extensions",
)
UNCHANGED = (
    "cdm_version", "resolver_version", "source", "document",
    "stories", "paragraphs", "text_runs", "resources", "styles",
    "extensions",
)
MAX_BYTES = 2_000_000
KINDS = {"shape", "text_frame", "image_frame", "vector_path",
         "group", "connector", "table", "placed_artifact", "unsupported"}


def load(path: Path) -> tuple[dict, str, int]:
    raw = path.read_bytes()
    if len(raw) > MAX_BYTES:
        raise ValueError("graph exceeds pinned size ceiling")
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError("CDM graph must be a JSON object")
    return value, hashlib.sha256(raw).hexdigest(), len(raw)


def _subsequence(before: list, after: list) -> bool:
    it = iter(after)
    return all(any(a == b for a in it) for b in before)


def compare(before: dict, after: dict) -> tuple[dict, list[str]]:
    """Compare all CDM registries. Only a single source-backed node insertion
    into one existing Page is tentatively admissible for issue #2813.
    """
    errors: list[str] = []
    report: dict = {"registries": {}, "changed_existing_nodes": 0}
    if set(before) != set(after):
        errors.append("top_level_schema_drift")
    for field in UNCHANGED:
        if before.get(field) != after.get(field):
            errors.append("modified_" + field)

    for registry in REGISTRIES:
        left, right = before.get(registry), after.get(registry)
        if not isinstance(left, dict) or not isinstance(right, dict):
            errors.append("invalid_registry_" + registry)
            continue
        a, b = set(left), set(right)
        modified = sum(left[k] != right[k] for k in (a & b))
        report["registries"][registry] = {
            "before": len(a), "after": len(b),
            "added": len(b - a), "removed": len(a - b),
            "modified_existing": modified,
        }
        if registry not in ("pages", "nodes") and (a != b or modified):
            errors.append("unexpected_registry_delta_" + registry)
        if registry == "nodes":
            report["changed_existing_nodes"] = modified
            if not b.issuperset(a) or modified:
                errors.append("old_nodes_changed_or_removed")
        if registry == "pages" and a != b:
            errors.append("page_identity_set_changed")

    old_nodes = before.get("nodes", {})
    new_nodes = after.get("nodes", {})
    old_pages = before.get("pages", {})
    new_pages = after.get("pages", {})
    if not all(isinstance(m, dict) for m in
               (old_nodes, new_nodes, old_pages, new_pages)):
        return report, sorted(set(errors))

    additions = set(new_nodes) - set(old_nodes)
    if len(additions) != 1:
        errors.append("expected_exactly_one_new_source_node")
    page_insertions = 0
    changed_pages = 0
    newly_referenced: set = set()
    for page_id in old_pages.keys() & new_pages.keys():
        old_page, new_page = old_pages[page_id], new_pages[page_id]
        if not isinstance(old_page, dict) or not isinstance(new_page, dict):
            errors.append("invalid_page_shape")
            continue
        if old_page == new_page:
            continue
        changed_pages += 1
        old_children, new_children = (
            old_page.get("children"), new_page.get("children"))
        if not isinstance(old_children, list) or not isinstance(new_children, list):
            errors.append("invalid_page_children")
            continue
        if {k: v for k, v in old_page.items() if k != "children"} != {
                k: v for k, v in new_page.items() if k != "children"}:
            errors.append("page_nonchild_fields_changed")
        if not _subsequence(old_children, new_children):
            errors.append("existing_page_paint_order_changed")
        if len(new_children) != len(old_children) + 1:
            errors.append("unexpected_page_child_count")
        if set(old_children) - set(new_children):
            errors.append("old_page_child_removed")
        newly_referenced.update(set(new_children) - set(old_children))
        page_insertions += len(new_children) - len(old_children)
    report["changed_pages"] = changed_pages
    report["new_page_children"] = page_insertions
    if changed_pages != 1 or page_insertions != 1 or newly_referenced != additions:
        errors.append("new_node_not_exactly_one_page_child")

    if len(additions) == 1:
        node = new_nodes[next(iter(additions))]
        kind = node.get("kind") if isinstance(node, dict) else None
        # Untrusted payload strings cannot be copied into the receipt.
        report["new_node_kind"] = kind if kind in KINDS else "unrecognized"
        header = node.get("header") if isinstance(node, dict) else None
        refs = header.get("source_refs") if isinstance(header, dict) else None
        report["new_node_has_source_refs"] = isinstance(refs, list) and len(refs) > 0
        if kind != "shape":
            errors.append("new_node_not_shape")
        if not isinstance(refs, list) or not refs:
            errors.append("new_node_missing_physical_source_refs")

    return report, sorted(set(errors))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--expected-baseline", required=True)
    parser.add_argument("--expected-candidate", required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()

    before, old_digest, old_bytes = load(args.baseline)
    after, new_digest, new_bytes = load(args.candidate)
    semantic, errors = compare(before, after)
    if old_digest != args.expected_baseline:
        errors.append("baseline_byte_identity_mismatch")
    if new_digest != args.expected_candidate:
        errors.append("candidate_byte_identity_mismatch")
    output = {
        "protocol": "chaptera.resolved-graph-source-neutral-diff.v1",
        "baseline_sha256": old_digest,
        "candidate_sha256": new_digest,
        "baseline_bytes": old_bytes,
        "candidate_bytes": new_bytes,
        "semantic": semantic,
        "pass": not errors,
        "failures": sorted(set(errors)),
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(output, sort_keys=True, indent=2) + "\n")
    print(json.dumps(output, sort_keys=True))
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
