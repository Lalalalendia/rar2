#!/usr/bin/env python3
"""Aggregate source-safe causes behind fixed-PDF missing image resources."""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess
import tempfile

import fitz

from pub_pdf_cli_publisher_oracle_v1 import (
    PRIVATE_FAMILY,
    build_source_index,
    convert,
    load_references,
    sha256,
)

SCHEMA = "chaptera.pub-pdf-image-missing-corpus.v1"


def merge(target: Counter, values: dict) -> None:
    for key, value in values.items():
        target[str(key)] += int(value)


def classify(helper: Path, source: Path, loss: Path, output: Path, timeout: int) -> dict:
    try:
        proc = subprocess.run(
            [str(helper), str(source), str(loss), str(output)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError("image-missing helper timed out") from exc
    if proc.returncode != 0 or not output.is_file():
        raise RuntimeError("image-missing helper failed")
    return json.loads(output.read_text(encoding="utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch01", type=Path, required=True)
    parser.add_argument("--supplemental", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--classifier", type=Path, required=True)
    parser.add_argument("--fallback-font", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")

    pairs = load_references(args.batch01, args.supplemental)
    sources = build_source_index(args.source_root)
    statuses = Counter()
    cause_counts = Counter()
    equal_page_causes = Counter()
    parent_topologies = Counter()
    node_kinds = Counter()
    features = Counter()

    rows = []
    hosted_found = 0
    candidate_pdf_count = 0
    image_pair_count = 0
    image_node_count = 0
    equal_page_pair_count = 0
    equal_page_node_count = 0
    graph_lookup_missing_count = 0

    for pair in pairs:
        fixture = pair["basename"]
        identity = pair["pub_sha256"]
        if pair.get("family") == PRIVATE_FAMILY:
            statuses["private_source_not_hosted"] += 1
            continue
        found = sources.get(identity, [])
        if len(found) != 1:
            statuses["source_missing_or_ambiguous"] += 1
            continue
        source = found[0]
        if source.stat().st_size != int(pair["pub_bytes"]) or sha256(source) != identity:
            statuses["source_identity_rejected"] += 1
            continue
        hosted_found += 1

        with tempfile.TemporaryDirectory(prefix="pub-pdf-image-cause-") as td:
            temp = Path(td)
            pdf = temp / "candidate.pdf"
            outcome, summary = convert(
                args.cli.resolve(),
                source,
                args.fallback_font.resolve(),
                pdf,
                args.timeout,
            )
            if outcome != "ok":
                statuses[outcome] += 1
                continue
            candidate_pdf_count += 1
            statuses["converted"] += 1

            missing = int(summary.get("node_code_counts", {}).get("pdf.node.resource_missing", 0))
            if missing == 0:
                continue

            helper_out = temp / "image-cause.json"
            try:
                receipt = classify(
                    args.classifier.resolve(),
                    source,
                    Path(str(pdf) + ".loss.json"),
                    helper_out,
                    args.timeout,
                )
            except (OSError, ValueError, RuntimeError, json.JSONDecodeError):
                statuses["classifier_failed"] += 1
                continue
            if receipt.get("source_sha256") != identity:
                raise SystemExit(f"classifier source identity mismatch for {fixture}")

            graph_lookup_missing_count += int(
                receipt.get("resolved_graph_lookup_missing_count", 0)
            )
            count = int(receipt.get("image_missing_node_count", 0))
            if count == 0:
                continue

            with fitz.open(pdf) as document:
                candidate_pages = document.page_count
            equal_pages = candidate_pages == int(pair["reference_pages"])

            image_pair_count += 1
            image_node_count += count
            merge(cause_counts, receipt.get("cause_counts", {}))
            merge(parent_topologies, receipt.get("parent_topology_counts", {}))
            merge(node_kinds, receipt.get("node_kind_counts", {}))
            merge(features, receipt.get("feature_counts", {}))
            if equal_pages:
                equal_page_pair_count += 1
                equal_page_node_count += count
                merge(equal_page_causes, receipt.get("cause_counts", {}))

            rows.append({
                "fixture": fixture,
                "family": pair.get("family", "batch01"),
                "reference_pages": pair["reference_pages"],
                "candidate_pages": candidate_pages,
                "publisher_page_count_equal": equal_pages,
                "image_missing_node_count": count,
                "unique_image_slot_count": receipt.get("unique_image_slot_count"),
                "cause_counts": receipt.get("cause_counts", {}),
                "parent_topology_counts": receipt.get("parent_topology_counts", {}),
                "node_kind_counts": receipt.get("node_kind_counts", {}),
                "feature_counts": receipt.get("feature_counts", {}),
                "asset_manifest": receipt.get("asset_manifest", {}),
            })

    rows.sort(key=lambda row: (-row["image_missing_node_count"], row["fixture"]))
    report = {
        "schema": SCHEMA,
        "registered_pair_count": len(pairs),
        "hosted_expected_pair_count": 79,
        "private_expected_pair_count": 7,
        "hosted_source_found_count": hosted_found,
        "candidate_pdf_count": candidate_pdf_count,
        "image_missing_pair_count": image_pair_count,
        "image_missing_node_count": image_node_count,
        "publisher_page_count_equal_image_pair_count": equal_page_pair_count,
        "publisher_page_count_equal_image_node_count": equal_page_node_count,
        "cause_counts": dict(sorted(cause_counts.items())),
        "publisher_page_count_equal_cause_counts": dict(sorted(equal_page_causes.items())),
        "parent_topology_counts": dict(sorted(parent_topologies.items())),
        "node_kind_counts": dict(sorted(node_kinds.items())),
        "feature_counts": dict(sorted(features.items())),
        "resolved_graph_lookup_missing_count": graph_lookup_missing_count,
        "status_counts": dict(sorted(statuses.items())),
        "pairs": rows,
        "claims": {
            "publisher_raster_used": False,
            "raw_node_ids_emitted": False,
            "raw_slot_ids_emitted": False,
            "source_offsets_emitted": False,
            "source_text_emitted": False,
            "image_bytes_emitted": False,
            "derived_preview_promoted_to_exact": False,
        },
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "hosted_source_found_count": hosted_found,
        "candidate_pdf_count": candidate_pdf_count,
        "image_missing_pair_count": image_pair_count,
        "image_missing_node_count": image_node_count,
        "publisher_page_count_equal_image_pair_count": equal_page_pair_count,
        "publisher_page_count_equal_image_node_count": equal_page_node_count,
        "cause_counts": report["cause_counts"],
        "publisher_page_count_equal_cause_counts": report["publisher_page_count_equal_cause_counts"],
        "top_pairs": rows[:12],
        "status_counts": report["status_counts"],
    }, indent=2, sort_keys=True))

    if hosted_found != 79:
        raise SystemExit("hosted source census incomplete")
    if graph_lookup_missing_count != 0:
        raise SystemExit("missing image node failed resolved-graph join")
    if image_node_count < 60:
        raise SystemExit("image-missing census unexpectedly lost its known cohort")


if __name__ == "__main__":
    main()
