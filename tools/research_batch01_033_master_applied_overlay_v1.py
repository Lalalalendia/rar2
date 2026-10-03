#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path

from PIL import Image

from cloud_reader_visual_fingerprint_v1 import (
    GRID_H,
    GRID_W,
    compare_grid,
    reference_grid,
    sha256,
)

SCHEMA = "chaptera.batch01-033-master-applied-overlay.v1"
MASK_THRESHOLDS = (12, 24, 48)
FOREGROUND_THRESHOLD = 24


def pil_grid(image: Image.Image) -> bytes:
    return image.convert("RGB").resize((GRID_W, GRID_H), Image.Resampling.BOX).tobytes()


def foreground_metrics(candidate: bytes, reference: bytes, threshold: int = FOREGROUND_THRESHOLD) -> dict:
    cells = len(candidate) // 3
    ref_count = 0
    candidate_count = 0
    intersection = 0
    for cell in range(cells):
        base = cell * 3
        ref_fg = max(255 - reference[base + c] for c in range(3)) >= threshold
        candidate_fg = max(255 - candidate[base + c] for c in range(3)) >= threshold
        ref_count += int(ref_fg)
        candidate_count += int(candidate_fg)
        intersection += int(ref_fg and candidate_fg)
    recall = intersection / ref_count if ref_count else 1.0
    precision = intersection / candidate_count if candidate_count else (1.0 if ref_count == 0 else 0.0)
    f1 = 2.0 * precision * recall / (precision + recall) if precision + recall else 0.0
    missing = 1.0 - recall if ref_count else 0.0
    extra = (candidate_count - intersection) / candidate_count if candidate_count else 0.0
    return {
        "foreground_recall": recall,
        "foreground_precision": precision,
        "foreground_f1": f1,
        "missing_reference_foreground_fraction": missing,
        "extra_candidate_foreground_fraction": extra,
        "foreground_loss": missing + extra,
    }


def masked_overlay(bottom: Image.Image, top: Image.Image, threshold: int) -> Image.Image:
    if bottom.size != top.size:
        raise ValueError("overlay source sizes differ")
    b = bottom.convert("RGB")
    t = top.convert("RGB")
    out = Image.new("RGB", b.size, (255, 255, 255))
    bp = b.load()
    tp = t.load()
    op = out.load()
    for y in range(b.height):
        for x in range(b.width):
            tr, tg, tb = tp[x, y]
            top_fg = max(255 - tr, 255 - tg, 255 - tb) >= threshold
            op[x, y] = (tr, tg, tb) if top_fg else bp[x, y]
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("browser_receipt", type=Path)
    ap.add_argument("page_role_receipt", type=Path)
    ap.add_argument("viewer_projection_receipt", type=Path)
    ap.add_argument("reference", type=Path)
    ap.add_argument("fixture")
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    browser = json.loads(args.browser_receipt.read_text(encoding="utf-8"))
    roles = json.loads(args.page_role_receipt.read_text(encoding="utf-8"))
    projection = json.loads(args.viewer_projection_receipt.read_text(encoding="utf-8"))
    reference = json.loads(args.reference.read_text(encoding="utf-8"))

    fixtures = [row for row in browser.get("results", []) if row.get("fixture") == args.fixture]
    pairs = [row for row in reference.get("pairs", []) if row.get("basename") == args.fixture]
    if len(fixtures) != 1 or len(pairs) != 1:
        raise ValueError("fixture/reference row is not unique")
    fixture = fixtures[0]
    pair = pairs[0]
    if fixture.get("rendered") is not True or int(fixture.get("pages", -1)) != 12:
        raise ValueError("033 fail-open render/cardinality drift")
    if int(pair.get("reference_pages", -1)) != 4:
        raise ValueError("033 reference cardinality drift")

    role_rows = sorted(roles.get("pages", []), key=lambda row: row["document_ordinal"])
    if len(role_rows) != 12:
        raise ValueError("033 source PAGE count drift")
    seq_to_ordinal = {int(row["contents_seq_num"]): int(row["document_ordinal"]) for row in role_rows}

    source_by_fingerprint = {}
    for row in role_rows:
        fp = row.get("page_identity_fingerprint_sha256")
        if not isinstance(fp, str) or len(fp) != 64:
            raise ValueError("source fingerprint missing")
        master = row.get("applied_master_seq_num")
        if master is None:
            relation = "none"
        elif master in seq_to_ordinal:
            relation = f"page_ordinal:{seq_to_ordinal[master]}"
        else:
            relation = "external"
        source_by_fingerprint[fp] = {
            "ordinal": int(row["document_ordinal"]),
            "relation": relation,
        }

    projection_rows = sorted(projection.get("per_page", []), key=lambda row: row["viewer_page_index"])
    screenshots = sorted(fixture.get("screenshots", []), key=lambda row: row["page"])
    geometry = sorted(fixture.get("page_geometry", []), key=lambda row: row["order"])
    if len(projection_rows) != 12 or len(screenshots) != 12 or len(geometry) != 12:
        raise ValueError("033 output receipt cardinality drift")

    by_ordinal = {}
    for idx, (projected, shot, geo) in enumerate(zip(projection_rows, screenshots, geometry), start=1):
        if projected.get("viewer_page_index") != idx or shot.get("page") != idx:
            raise ValueError("033 output order drift")
        fp = projected.get("page_identity_fingerprint_sha256")
        source = source_by_fingerprint.get(fp)
        if source is None:
            raise ValueError("Viewer PAGE missing from source identity receipt")
        png = args.browser_receipt.parent / shot["filename"]
        if sha256(png) != shot["sha256"]:
            raise ValueError("033 screenshot SHA drift")
        by_ordinal[source["ordinal"]] = {
            "png": png,
            "relation": source["relation"],
            "width_emu": int(geo["width_emu"]),
            "height_emu": int(geo["height_emu"]),
        }

    if sorted(by_ordinal) != list(range(12)):
        raise ValueError(f"033 ordinal coverage drift: {sorted(by_ordinal)}")

    expected_pairs = [(0, 4), (1, 5), (2, 6), (3, 7)]
    for root, applied in expected_pairs:
        if by_ordinal[root]["relation"] != "none":
            raise ValueError(f"033 root {root} unexpectedly applied")
        if by_ordinal[applied]["relation"] != f"page_ordinal:{root}":
            raise ValueError(f"033 applied relation drift for {root}->{applied}")

    refs = [reference_grid(row) for row in pair.get("pages", [])]
    if len(refs) != 4:
        raise ValueError("033 reference fingerprint rows missing")

    def load(ordinal: int) -> Image.Image:
        with Image.open(by_ordinal[ordinal]["png"]) as image:
            return image.convert("RGB").copy()

    def compare_image(image: Image.Image, reference_index: int, members: list[int], order: str, mask_threshold: int | None) -> dict:
        candidate = pil_grid(image)
        ref = refs[reference_index - 1]
        metrics = compare_grid(candidate, ref)
        foreground = foreground_metrics(candidate, ref)
        return {
            "reference_page": reference_index,
            "source_document_ordinals": members,
            "order": order,
            "mask_threshold": mask_threshold,
            **metrics,
            **foreground,
        }

    controls = []
    for i, (root, applied) in enumerate(expected_pairs, start=1):
        controls.append({
            "reference_page": i,
            "root": compare_image(load(root), i, [root], "root_only", None),
            "applied": compare_image(load(applied), i, [applied], "applied_only", None),
        })

    hypotheses = []
    for threshold in MASK_THRESHOLDS:
        for order in ("root_under_applied", "applied_under_root"):
            rows = []
            for i, (root, applied) in enumerate(expected_pairs, start=1):
                root_img = load(root)
                applied_img = load(applied)
                if order == "root_under_applied":
                    composed = masked_overlay(root_img, applied_img, threshold)
                    members = [root, applied]
                else:
                    composed = masked_overlay(applied_img, root_img, threshold)
                    members = [applied, root]
                rows.append(compare_image(composed, i, members, order, threshold))
            hypotheses.append({
                "order": order,
                "mask_threshold": threshold,
                "mean_changed_cell_fraction": sum(r["changed_cell_fraction"] for r in rows) / 4.0,
                "mean_abs_channel_delta": sum(r["mean_abs_channel_delta"] for r in rows) / 4.0,
                "mean_foreground_loss": sum(r["foreground_loss"] for r in rows) / 4.0,
                "mean_foreground_f1": sum(r["foreground_f1"] for r in rows) / 4.0,
                "pages": rows,
            })

    hypotheses.sort(key=lambda row: (
        row["mean_foreground_loss"],
        row["mean_changed_cell_fraction"],
        row["mean_abs_channel_delta"],
        row["order"],
        row["mask_threshold"],
    ))

    out = {
        "schema": SCHEMA,
        "fixture": args.fixture,
        "source_sha256": fixture.get("source_sha256"),
        "viewer_page_count": 12,
        "reference_page_count": 4,
        "pairing": [[a, b] for a, b in expected_pairs],
        "controls": controls,
        "hypotheses": hypotheses,
        "best_hypothesis": hypotheses[0],
        "claims": {
            "measurement_only": True,
            "source_graph_mutated": False,
            "viewer_semantics_changed": False,
            "raw_page_id_emitted": False,
            "raw_contents_seq_num_emitted": False,
            "screenshots_emitted": False,
            "story_text_emitted": False,
            "publisher_fingerprint_used_as_product_authority": False,
            "overlay_is_raster_counterfactual_only": True,
            "page_identity_join_uses_sha256_of_canonical_page_id": True,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(json.dumps({
        "best_order": hypotheses[0]["order"],
        "best_mask_threshold": hypotheses[0]["mask_threshold"],
        "best_mean_changed_cell_fraction": hypotheses[0]["mean_changed_cell_fraction"],
        "best_mean_foreground_loss": hypotheses[0]["mean_foreground_loss"],
        "best_mean_foreground_f1": hypotheses[0]["mean_foreground_f1"],
        "best_page_f1": [row["foreground_f1"] for row in hypotheses[0]["pages"]],
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
