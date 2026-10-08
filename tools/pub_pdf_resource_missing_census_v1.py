#!/usr/bin/env python3
"""Classify fixed-PDF resource_missing nodes using source/Viewer semantics only."""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess
import tempfile

from pub_pdf_cli_publisher_oracle_v1 import (
    PRIVATE_FAMILY,
    build_source_index,
    compare_pdf,
    convert,
    load_references,
    sha256,
)

SCHEMA = "chaptera.pub-pdf-resource-missing-corpus.v1"


def merge_counts(target: Counter, values: dict) -> None:
    for key, value in values.items():
        target[str(key)] += int(value)


def run_classifier(classifier: Path, source: Path, loss: Path, out: Path, timeout: int) -> dict:
    try:
        proc = subprocess.run(
            [str(classifier), str(source), str(loss), str(out)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError("resource-missing classifier timeout") from exc
    if proc.returncode != 0 or not out.is_file():
        raise RuntimeError("resource-missing classifier failed")
    return json.loads(out.read_text(encoding="utf-8"))


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
    all_classes = Counter()
    all_kinds = Counter()
    all_features = Counter()
    compared_classes = Counter()
    compared_kinds = Counter()
    compared_features = Counter()

    affected_rows = []
    noncompared_rows = []
    hosted_source_found_count = 0
    candidate_pdf_count = 0
    fully_compared_pair_count = 0
    compared_page_count = 0
    all_missing_pair_count = 0
    all_missing_node_count = 0
    compared_missing_pair_count = 0
    compared_missing_node_count = 0
    graph_lookup_missing_count = 0

    for pair in pairs:
        identity = pair["pub_sha256"]
        fixture = pair["basename"]
        if pair.get("family") == PRIVATE_FAMILY:
            statuses["private_source_not_hosted"] += 1
            noncompared_rows.append({"fixture": fixture, "status": "private_source_not_hosted"})
            continue

        found = sources.get(identity, [])
        if len(found) != 1:
            statuses["source_missing_or_ambiguous"] += 1
            noncompared_rows.append({"fixture": fixture, "status": "source_missing_or_ambiguous"})
            continue
        source = found[0]
        if source.stat().st_size != int(pair["pub_bytes"]) or sha256(source) != identity:
            statuses["source_identity_rejected"] += 1
            noncompared_rows.append({"fixture": fixture, "status": "source_identity_rejected"})
            continue

        hosted_source_found_count += 1
        with tempfile.TemporaryDirectory(prefix="pub-pdf-resource-missing-") as td:
            temp = Path(td)
            pdf = temp / "candidate.pdf"
            outcome, loss_summary = convert(
                args.cli.resolve(),
                source,
                args.fallback_font.resolve(),
                pdf,
                args.timeout,
            )
            if outcome != "ok":
                statuses[outcome] += 1
                noncompared_rows.append({"fixture": fixture, "status": outcome})
                continue

            candidate_pdf_count += 1
            loss = Path(str(pdf) + ".loss.json")
            classifier_out = temp / "classification.json"
            try:
                classification = run_classifier(
                    args.classifier.resolve(), source, loss, classifier_out, args.timeout
                )
            except (OSError, ValueError, RuntimeError, json.JSONDecodeError):
                statuses["classifier_failed"] += 1
                noncompared_rows.append({"fixture": fixture, "status": "classifier_failed"})
                continue
            if classification.get("source_sha256") != identity:
                raise SystemExit(f"classifier source identity mismatch for {fixture}")
            graph_lookup_missing_count += int(
                classification.get("resolved_graph_lookup_missing_count", 0)
            )

            try:
                comparison = compare_pdf(pdf, pair)
            except Exception:
                statuses["pdf_decode_or_raster_failed"] += 1
                noncompared_rows.append({"fixture": fixture, "status": "pdf_decode_or_raster_failed"})
                continue

            status = comparison["status"]
            statuses[status] += 1
            pages_compared = sum(
                page.get("status") == "compared" for page in comparison.get("pages", [])
            )
            compared_page_count += pages_compared
            fully_compared = status in ("raster_compared", "raster_compared_stage_unknown")
            if fully_compared:
                fully_compared_pair_count += 1

            missing = int(classification.get("resource_missing_count", 0))
            if missing:
                all_missing_pair_count += 1
                all_missing_node_count += missing
                merge_counts(all_classes, classification.get("semantic_class_counts", {}))
                merge_counts(all_kinds, classification.get("node_kind_counts", {}))
                merge_counts(all_features, classification.get("feature_counts", {}))

                row = {
                    "fixture": fixture,
                    "family": pair.get("family", "batch01"),
                    "status": status,
                    "reference_pages": pair["reference_pages"],
                    "candidate_pages": comparison.get("candidate_pages"),
                    "resource_missing_count": missing,
                    "semantic_class_counts": classification.get("semantic_class_counts", {}),
                    "node_kind_counts": classification.get("node_kind_counts", {}),
                    "feature_counts": classification.get("feature_counts", {}),
                    "pdf_node_code_counts": loss_summary.get("node_code_counts", {}),
                }
                if fully_compared:
                    row["mean_changed_cell_fraction"] = comparison.get(
                        "mean_changed_cell_fraction"
                    )
                    compared_missing_pair_count += 1
                    compared_missing_node_count += missing
                    merge_counts(compared_classes, classification.get("semantic_class_counts", {}))
                    merge_counts(compared_kinds, classification.get("node_kind_counts", {}))
                    merge_counts(compared_features, classification.get("feature_counts", {}))
                affected_rows.append(row)
            elif not fully_compared:
                noncompared_rows.append(
                    {
                        "fixture": fixture,
                        "status": status,
                        "reference_pages": pair["reference_pages"],
                        "candidate_pages": comparison.get("candidate_pages"),
                    }
                )

    affected_rows.sort(
        key=lambda row: (
            -(row.get("mean_changed_cell_fraction") or -1.0),
            -row["resource_missing_count"],
            row["fixture"],
        )
    )
    noncompared_rows.sort(key=lambda row: row["fixture"])

    report = {
        "schema": SCHEMA,
        "registered_pair_count": len(pairs),
        "registered_publisher_page_count": sum(p["reference_pages"] for p in pairs),
        "hosted_expected_pair_count": 79,
        "private_expected_pair_count": 7,
        "hosted_source_found_count": hosted_source_found_count,
        "candidate_pdf_count": candidate_pdf_count,
        "fully_compared_pair_count": fully_compared_pair_count,
        "compared_page_count": compared_page_count,
        "status_counts": dict(sorted(statuses.items())),
        "all_candidate_resource_missing": {
            "pair_count": all_missing_pair_count,
            "node_count": all_missing_node_count,
            "semantic_class_counts": dict(sorted(all_classes.items())),
            "node_kind_counts": dict(sorted(all_kinds.items())),
            "feature_counts": dict(sorted(all_features.items())),
        },
        "fully_compared_resource_missing": {
            "pair_count": compared_missing_pair_count,
            "node_count": compared_missing_node_count,
            "semantic_class_counts": dict(sorted(compared_classes.items())),
            "node_kind_counts": dict(sorted(compared_kinds.items())),
            "feature_counts": dict(sorted(compared_features.items())),
        },
        "resolved_graph_lookup_missing_count": graph_lookup_missing_count,
        "affected_pairs": affected_rows,
        "noncompared_pairs": noncompared_rows,
        "claims": {
            "raw_node_ids_emitted": False,
            "source_text_emitted": False,
            "classification_is_source_viewer_semantic_not_raster_inferred": True,
            "classification_changes_pdf_output": False,
            "publisher_raster_used_only_for_residual_correlation": True,
        },
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "hosted_source_found_count": hosted_source_found_count,
        "candidate_pdf_count": candidate_pdf_count,
        "fully_compared_pair_count": fully_compared_pair_count,
        "compared_page_count": compared_page_count,
        "fully_compared_resource_missing": report["fully_compared_resource_missing"],
        "top_affected_pairs": affected_rows[:15],
        "status_counts": report["status_counts"],
    }, indent=2, sort_keys=True))

    if hosted_source_found_count != 79:
        raise SystemExit("hosted Publisher corpus source set is incomplete")
    if graph_lookup_missing_count != 0:
        raise SystemExit("resource-missing node could not be joined to resolved source graph")
    if fully_compared_pair_count < 50:
        raise SystemExit("Publisher-comparable coverage unexpectedly collapsed")


if __name__ == "__main__":
    main()
