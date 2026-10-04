#!/usr/bin/env python3
"""Build a source-safe census for current Viewer nodes omitted from fixed-PDF resources."""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
from typing import Any

SCHEMA = "chaptera.current-viewer-fixed-pdf-residual-census.v1"
PACKET_VERSION = "chaptera.current-viewer-fixed-pdf-input.v1"


def color_resolution_for_range(
    runs: list[dict[str, Any]], scalar_start: int, scalar_end: int
) -> str:
    if scalar_start >= scalar_end:
        return "invalid_range"
    cursor = scalar_start
    resolved: tuple[int, int, int] | None = None
    for run in runs:
        start = max(int(run["scalar_start"]), scalar_start)
        end = min(int(run["scalar_end"]), scalar_end)
        if start >= end:
            continue
        if start != cursor:
            return "coverage_gap"
        color = run.get("color_rgb")
        if color is None:
            return "missing_rgb"
        current = tuple(int(value) for value in color)
        if resolved is None:
            resolved = current
        elif resolved != current:
            return "mixed_rgb"
        cursor = end
    if cursor != scalar_end:
        return "coverage_gap"
    return "uniform_rgb" if resolved is not None else "coverage_gap"


def uniform_color_for_range(
    runs: list[dict[str, Any]], scalar_start: int, scalar_end: int
) -> bool:
    return color_resolution_for_range(runs, scalar_start, scalar_end) == "uniform_rgb"


def classify_node(node: dict[str, Any]) -> tuple[list[str], dict[str, int]]:
    reasons: list[str] = []
    line_counts: collections.Counter[str] = collections.Counter()

    if node.get("projected_scene_instance") is None:
        reasons.append("scene_projection:base")
    else:
        reasons.append("scene_projection:projected")

    if node.get("solid_fill_rgb") is not None or node.get("solid_line") is not None:
        reasons.append("mapped_paint_present")

    image = node.get("image")
    if image is not None:
        if image.get("source_window") is not None:
            reasons.append("cropped_image")
        else:
            reasons.append("mapped_full_image_present")

    if node.get("table") is not None:
        reasons.append("table_present")
    if node.get("decorative_border") is not None:
        reasons.append("decorative_border_present")

    text = node.get("text")
    if text is not None:
        layout = text.get("layout")
        if layout is None:
            reasons.append("text_layout_missing")
        else:
            disposition = layout.get("disposition") or {}
            kind = disposition.get("kind")
            if kind == "backend_fallback":
                reason = disposition.get("reason")
                reasons.append(f"text_backend_fallback:{reason}")
                if reason == "shared_layout_incomplete":
                    sizes = {
                        int(run["text_size_emu"])
                        for run in (text.get("typography") or [])
                        if int(run.get("text_size_emu", 0)) > 0
                    }
                    if not sizes:
                        reasons.append("shared_layout_incomplete:size_profile_unknown")
                    elif len(sizes) == 1:
                        reasons.append("shared_layout_incomplete:uniform_size")
                    else:
                        reasons.append("shared_layout_incomplete:mixed_size")
            elif kind == "shared_resolved":
                admitted_lines = 0
                typography = text.get("typography") or []
                for line in layout.get("lines") or []:
                    if not line.get("text"):
                        line_counts["empty_line"] += 1
                        continue
                    if line.get("spans"):
                        line_counts["shaped_span_line"] += 1
                        continue
                    if line.get("shaping") is None:
                        line_counts["missing_shaping_line"] += 1
                        continue
                    color_status = color_resolution_for_range(
                        typography,
                        int(line["scalar_start"]),
                        int(line["scalar_end"]),
                    )
                    if color_status != "uniform_rgb":
                        line_counts["unresolved_text_color_line"] += 1
                        line_counts[f"unresolved_text_color:{color_status}"] += 1
                        reasons.append(
                            f"shared_resolved_unresolved_color:{color_status}"
                        )
                        continue
                    admitted_lines += 1
                if admitted_lines:
                    reasons.append("mapped_text_present")
                    line_counts["admitted_text_line"] += admitted_lines
                else:
                    reasons.append("shared_resolved_no_admitted_text")
                    if line_counts["shaped_span_line"]:
                        reasons.append("shared_resolved_shaped_spans_only_or_partial")
                    if line_counts["missing_shaping_line"]:
                        reasons.append("shared_resolved_missing_shaping")
                    if line_counts["unresolved_text_color_line"]:
                        reasons.append("shared_resolved_unresolved_color")
            else:
                reasons.append(f"text_layout_unknown_disposition:{kind}")

    if not reasons:
        reasons.append("unclassified_resource_missing")

    return sorted(set(reasons)), dict(sorted(line_counts.items()))


def build_census(packet: dict[str, Any], renderer_wrap: dict[str, Any]) -> dict[str, Any]:
    if packet.get("protocol_version") != PACKET_VERSION:
        raise ValueError("unexpected current Viewer packet protocol")

    renderer = renderer_wrap.get("renderer_result", renderer_wrap)
    node_reports = renderer.get("node_reports")
    if not isinstance(node_reports, list):
        raise ValueError("renderer node_reports missing")

    residual_ids = {
        item["origin_node_id"]
        for item in node_reports
        if item.get("code") == "pdf.node.resource_missing"
    }

    packet_nodes: dict[str, tuple[int, int, dict[str, Any]]] = {}
    for page_index, page in enumerate(packet.get("pages") or []):
        for node_index, node in enumerate(page.get("nodes") or []):
            node_id = node["node_id"]
            if node_id in packet_nodes:
                raise ValueError(f"duplicate packet NodeId: {node_id}")
            packet_nodes[node_id] = (page_index, node_index, node)

    unknown = sorted(residual_ids - packet_nodes.keys())
    if unknown:
        raise ValueError(f"renderer residual NodeIds missing from packet: {unknown!r}")

    reason_nodes: collections.Counter[str] = collections.Counter()
    line_reasons: collections.Counter[str] = collections.Counter()
    combinations: collections.Counter[str] = collections.Counter()
    per_page: collections.Counter[str] = collections.Counter()
    entries: list[dict[str, Any]] = []

    for node_id, (page_index, node_index, node) in packet_nodes.items():
        if node_id not in residual_ids:
            continue
        reasons, line_counts = classify_node(node)
        for reason in reasons:
            reason_nodes[reason] += 1
        for reason, count in line_counts.items():
            line_reasons[reason] += count
        combination = " + ".join(reasons)
        combinations[combination] += 1
        per_page[str(page_index + 1)] += 1
        entries.append(
            {
                "node_id": node_id,
                "page_index": page_index,
                "node_index": node_index,
                "reasons": reasons,
                "line_reason_counts": line_counts,
            }
        )

    if len(entries) != len(residual_ids):
        raise ValueError("residual census cardinality mismatch")

    summary = renderer.get("summary") or {}
    if int(summary.get("node_unsupported", len(entries))) < len(entries):
        raise ValueError("renderer unsupported summary smaller than resource-missing census")

    return {
        "schema": SCHEMA,
        "source_hash": (packet.get("binding") or {}).get("source_hash"),
        "input_page_count": len(packet.get("pages") or []),
        "input_node_count": sum(
            len(page.get("nodes") or []) for page in packet.get("pages") or []
        ),
        "resource_missing_node_count": len(entries),
        "reason_node_counts": dict(sorted(reason_nodes.items())),
        "line_reason_counts": dict(sorted(line_reasons.items())),
        "reason_combination_counts": dict(sorted(combinations.items())),
        "per_page_resource_missing_counts": dict(sorted(per_page.items())),
        "nodes": entries,
        "source_safe": {
            "story_text_retained": False,
            "image_bytes_retained": False,
            "font_bytes_retained": False,
            "resource_ids_retained": False,
            "geometry_retained": False,
            "colors_retained": False,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--packet", required=True, type=pathlib.Path)
    parser.add_argument("--renderer", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    packet = json.loads(args.packet.read_text(encoding="utf-8"))
    renderer = json.loads(args.renderer.read_text(encoding="utf-8"))
    census = build_census(packet, renderer)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(census, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
