#!/usr/bin/env python3
"""Source-bound PUB->PDF CLI comparison against registered Publisher raster oracles.

Outputs only fixture identities, bounded metrics, and safe status codes. Source PUB,
generated PDF, font bytes, and document content remain in the CI runner.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

import fitz
from PIL import Image

from cloud_reader_visual_fingerprint_v1 import (
    CELL_DELTA, GRID_H, GRID_W, compare_grid, reference_grid, reference_surface_stage,
)

SCHEMA = "chaptera.pub-pdf-cli-publisher-fingerprint.v1"
REFERENCE_SCHEMA = "chaptera.publisher-visual-fingerprint.v1"
RASTER_DPI = 144
MAX_RASTER_PIXELS = 60_000_000
REGISTERED_BATCHES = ((55, 162), (31, 59))
PRIVATE_FAMILY = "manual-reduction-family"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as src:
        for buf in iter(lambda: src.read(1024 * 1024), b""):
            h.update(buf)
    return h.hexdigest()


def load_references(batch01: Path, supplemental: Path) -> list[dict]:
    pairs = []
    for path, (pair_count, page_count) in zip((batch01, supplemental), REGISTERED_BATCHES):
        data = json.loads(path.read_text(encoding="utf-8"))
        if (
            data.get("schema") != REFERENCE_SCHEMA
            or data.get("pair_count") != pair_count
            or data.get("reference_page_count") != page_count
            or len(data.get("pairs", [])) != pair_count
        ):
            raise ValueError("registered Publisher fingerprint census drift")
        for pair in data["pairs"]:
            if len(pair.get("pages", [])) != pair["reference_pages"]:
                raise ValueError("missing Publisher reference pages")
            if not re.fullmatch("[0-9a-f]{64}", pair["pub_sha256"]):
                raise ValueError("invalid PUB SHA-256")
            if not re.fullmatch("[0-9a-f]{64}", pair["pdf_sha256"]):
                raise ValueError("invalid Publisher PDF SHA-256")
            for page in pair["pages"]:
                reference_grid(page)  # Verify digest and compressed RGB bytes before running anything.
            pairs.append(pair)
    if len({p["pub_sha256"] for p in pairs}) != 86:
        raise ValueError("duplicate source identities or missing registered PUBs")
    if len({p["basename"] for p in pairs}) != 86:
        raise ValueError("duplicate registered fixture names")
    if sum(p["reference_pages"] for p in pairs) != 221:
        raise ValueError("registered Publisher page count drift")
    if sum(p.get("family") == PRIVATE_FAMILY for p in pairs) != 7:
        raise ValueError("registered private corpus partition drift")
    return pairs


def build_source_index(root: Path) -> dict[str, list[Path]]:
    index: dict[str, list[Path]] = {}
    for path in root.rglob("*.pub"):
        if re.fullmatch("[0-9a-fA-F]{64}", path.stem):
            index.setdefault(path.stem.lower(), []).append(path)
    return index


def page_grid(page: fitz.Page) -> bytes:
    pix = page.get_pixmap(dpi=RASTER_DPI, colorspace=fitz.csRGB, alpha=False)
    image = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
    return image.resize((GRID_W, GRID_H), Image.Resampling.BOX).tobytes()


def compare_pdf(path: Path, pair: dict) -> dict:
    out = {"candidate_pdf_sha256": sha256(path), "pages": []}
    with fitz.open(path) as pdf:
        count = pdf.page_count
        out["candidate_pages"] = count
        if count != pair["reference_pages"]:
            out["status"] = "page_count_mismatch"
            return out
        stage = reference_surface_stage(pair.get("reference_surface_stage"))
        if stage in ("production_sheet", "viewport_spread"):
            out["status"] = "surface_mapping_required"
            return out
        for index, reference_page in enumerate(pair["pages"]):
            candidate_page = pdf[index]
            delta = [
                round(float(candidate_page.rect.width) - float(reference_page["media_width_pt"]), 6),
                round(float(candidate_page.rect.height) - float(reference_page["media_height_pt"]), 6),
            ]
            page_row = {"page": index + 1, "media_extent_delta_pt": delta}
            px_w = candidate_page.rect.width * RASTER_DPI / 72
            px_h = candidate_page.rect.height * RASTER_DPI / 72
            if abs(delta[0]) > 0.05 or abs(delta[1]) > 0.05:
                page_row["status"] = "media_extent_mismatch"
            elif px_w <= 0 or px_h <= 0 or px_w * px_h > MAX_RASTER_PIXELS:
                page_row["status"] = "raster_budget_exceeded"
            else:
                page_row.update(compare_grid(page_grid(candidate_page), reference_grid(reference_page)))
                page_row["status"] = "compared"
                page_row["reference_grid_sha256"] = reference_page["grid_sha256"]
            out["pages"].append(page_row)
        compared = [p for p in out["pages"] if p["status"] == "compared"]
        if len(compared) == count:
            out["status"] = "raster_compared_stage_unknown" if stage == "unknown" else "raster_compared"
            out["mean_changed_cell_fraction"] = sum(p["changed_cell_fraction"] for p in compared) / len(compared)
        else:
            out["status"] = "partial_page_comparison"
    return out



def classify_cli_failure(stderr: str) -> str:
    """Map CLI stderr to a bounded source-safe status; never emit raw stderr."""
    if (
        "bounded PDF conversion currently requires mature 0x2C PUB input" in stderr
        or "bounded PDF conversion currently requires mature 0x2C or legacy 0x22 low-text PUB input" in stderr
        or "bounded PDF conversion currently requires mature 0x2C, legacy 0x22 low-text, or legacy 0x22 Quill PUB input" in stderr
    ):
        return "unsupported_pub_route"
    if "fallback font cannot be embedded under fixed PDF policy" in stderr:
        return "fallback_font_embedding_blocked"
    if "open mature-0x2C PUB for bounded PDF conversion" in stderr:
        return "pub_open_failed"
    if (
        "project effective mature pages for bounded PDF conversion" in stderr
        or "resolve bounded shaped text flow for fixed PDF" in stderr
    ):
        return "layout_projection_failed"
    pdf_render_rules = (
        ("has non-positive size", "pdf_nonpositive_page_size"),
        ("duplicate explicit paint for node", "pdf_duplicate_paint"),
        ("explicit paint references missing resolved node", "pdf_missing_paint_node"),
        ("duplicate fixed image resource", "pdf_duplicate_image_resource"),
        ("more than one fixed image resource references node", "pdf_duplicate_image_use"),
        ("fixed image resource", "pdf_image_resource_failed"),
        ("resolved text preparation failed", "pdf_text_prepare_failed"),
    )
    if "render bounded deterministic PDF" in stderr:
        for marker, code in pdf_render_rules:
            if marker in stderr:
                return code
        return "pdf_render_failed"
    return "cli_failed_other"

def safe_loss_summary(receipt: dict) -> dict:
    """Keep only bounded counts/codes from the loss receipt; never source text or ids."""
    typography = receipt.get("typography", {})
    pdf_report = receipt.get("pdf", {})
    node_dispositions = Counter(str(node.get("disposition", "unknown")) for node in pdf_report.get("nodes", []))
    node_codes = Counter(str(node.get("code", "unknown")) for node in pdf_report.get("nodes", []))
    pdf_diagnostics = Counter(str(item.get("code", "unknown")) for item in pdf_report.get("diagnostics", []))
    shaped_diagnostics = Counter(
        str(item.get("code", "unknown"))
        for item in typography.get("shaped_flow", {}).get("diagnostics", [])
        if isinstance(item, dict)
    )
    skipped_codes = Counter(
        str(item.get("code", "unknown"))
        for item in typography.get("skipped", [])
        if isinstance(item, dict)
    )
    table_projection = receipt.get("table_projection", {})
    safe_table_projection = {
        str(key): value
        for key, value in table_projection.items()
        if key in {
            "source_table_count",
            "table_paint_resource_count",
            "table_fill_primitive_count",
            "table_border_primitive_count",
            "table_paint_incomplete_count",
            "table_no_visible_paint_count",
            "table_row_total",
            "table_column_total",
            "table_max_rows",
            "table_max_columns",
            "table_spanning_cell_count",
            "table_missing_exact_geometry_cell_count",
            "table_direct_literal_fill_cell_count",
            "table_native_autoformat_fill_cell_count",
            "table_other_fill_authority_cell_count",
            "table_nonempty_cell_count",
            "table_story_range_cell_count",
            "table_hidden_fill_cell_count",
            "table_uniform_text_inset_count",
            "table_uniform_vertical_alignment_count",
            "table_unique_fill_rgb_count",
            "table_unique_border_rgb_count",
            "table_unique_border_width_count",
            "table_text_disposition",
        }
        and isinstance(value, (int, str))
    }
    return {
        "table_projection": dict(sorted(safe_table_projection.items())),
        "materialized_text_run_count": len(typography.get("materialized_runs", [])),
        "skipped_text_run_count": len(typography.get("skipped", [])),
        "visible_line_count": typography.get("shaped_flow", {}).get("visible_line_count"),
        "node_disposition_counts": dict(sorted(node_dispositions.items())),
        "node_code_counts": dict(sorted(node_codes.items())),
        "pdf_diagnostic_code_counts": dict(sorted(pdf_diagnostics.items())),
        "shaped_flow_diagnostic_code_counts": dict(sorted(shaped_diagnostics.items())),
        "skipped_text_code_counts": dict(sorted(skipped_codes.items())),
    }


def convert(cli: Path, pub: Path, font: Path, pdf: Path, timeout: int) -> tuple[str, dict]:
    try:
        proc = subprocess.run(
            [str(cli), "convert", str(pub), "--to", "pdf", "--output", str(pdf),
             "--fallback-font", str(font)],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return "cli_timeout", {}
    if proc.returncode != 0:
        return classify_cli_failure(proc.stderr or ""), {}
    loss = Path(str(pdf) + ".loss.json")
    if not pdf.is_file() or not loss.is_file():
        return "output_missing", {}
    try:
        receipt = json.loads(loss.read_text(encoding="utf-8"))
        if receipt["conversion_profile"]["source"]["source_sha256"] != sha256(pub):
            return "output_source_identity_mismatch", {}
        if receipt["target"]["format"] != "pdf":
            return "output_format_mismatch", {}
        if receipt["typography"]["disposition"] != "explicit_user_fallback_not_source_font":
            return "output_font_policy_mismatch", {}
    except (OSError, ValueError, KeyError, TypeError):
        return "output_receipt_invalid", {}
    return "ok", safe_loss_summary(receipt)


def run(pairs: list[dict], source_root: Path, cli: Path, font: Path, timeout: int) -> dict:
    if not cli.is_file() or not font.is_file():
        raise ValueError("CLI binary or explicit fallback font missing")
    sources = build_source_index(source_root)
    rows = []
    for pair in pairs:
        identity = pair["pub_sha256"]
        stage = reference_surface_stage(pair.get("reference_surface_stage"))
        row = {
            "fixture": pair["basename"],
            "oracle_id": pair["oracle_id"],
            "pub_sha256": identity,
            "publisher_pdf_sha256": pair["pdf_sha256"],
            "family": pair.get("family", "batch01"),
            "warning_state": pair.get("warning_state"),
            "reference_state": pair.get("reference_state"),
            "reference_surface_stage": stage,
            "reference_pages": pair["reference_pages"],
        }
        found = sources.get(identity, [])
        if pair.get("family") == PRIVATE_FAMILY:
            row["status"] = "private_source_not_hosted"
        elif len(found) != 1:
            row["status"] = "source_missing_or_ambiguous"
        elif found[0].stat().st_size != pair["pub_bytes"] or sha256(found[0]) != identity:
            row["status"] = "source_identity_rejected"
        else:
            with tempfile.TemporaryDirectory(prefix="pub-pdf-oracle-") as td:
                pdf = Path(td) / "output.pdf"
                outcome, loss_summary = convert(cli, found[0], font, pdf, timeout)
                if outcome != "ok":
                    row["status"] = outcome
                else:
                    row["loss_summary"] = loss_summary
                    try:
                        row.update(compare_pdf(pdf, pair))
                    except (fitz.FileDataError, fitz.EmptyFileError, OSError, ValueError, RuntimeError):
                        row["status"] = "pdf_decode_or_raster_failed"
        rows.append(row)
    counts = Counter(r["status"] for r in rows)
    pages = [
        {"fixture": r["fixture"], "family": r["family"], "warning_state": r["warning_state"], **p}
        for r in rows for p in r.get("pages", []) if p["status"] == "compared"
    ]
    pages.sort(key=lambda p: (-p["changed_cell_fraction"], p["fixture"], p["page"]))
    mean = sum(p["changed_cell_fraction"] for p in pages) / len(pages) if pages else None
    return {
        "schema": SCHEMA,
        "registered_pair_count": len(pairs),
        "registered_publisher_page_count": sum(p["reference_pages"] for p in pairs),
        "hosted_expected_pair_count": 79,
        "private_expected_pair_count": 7,
        "hosted_source_found_count": sum(r["status"] not in (
            "private_source_not_hosted", "source_missing_or_ambiguous", "source_identity_rejected"
        ) for r in rows),
        "candidate_pdf_count": sum("candidate_pdf_sha256" in r for r in rows),
        "fully_compared_pair_count": counts["raster_compared"] + counts["raster_compared_stage_unknown"],
        "known_surface_compared_pair_count": counts["raster_compared"],
        "compared_page_count": len(pages),
        "status_counts": dict(sorted(counts.items())),
        "mean_changed_cell_fraction": mean,
        "worst_pages": pages[:30],
        "pairs": rows,
        "raster": {"renderer": "MuPDF", "dpi": RASTER_DPI, "grid": [GRID_W, GRID_H],
                   "resampling": "Pillow.BOX", "changed_cell_channel_delta": CELL_DELTA},
        "claims": {
            "publisher_visual_parity_proven": False,
            "publisher_source_font_identity_proven": False,
            "source_or_candidate_pdf_bytes_uploaded": False,
            "source_text_uploaded": False,
            "private_inputs_not_silently_counted": True,
            "reference_media_box_not_source_page_authority": True,
        },
        "limitations": [
            "64x64 RGB raster comparison is screening evidence, not full-resolution visual parity.",
            "Unknown PDF output-surface stage is compared only provisionally.",
            "Fallback font is explicit and may differ from Publisher's original source fonts.",
            "Publisher PDF page count can encode print sheets rather than logical pages.",
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch01", type=Path, required=True)
    parser.add_argument("--supplemental", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--fallback-font", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    report = run(
        load_references(args.batch01, args.supplemental),
        args.source_root,
        args.cli.resolve(),
        args.fallback_font.resolve(),
        args.timeout,
    )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: report[key] for key in (
        "registered_pair_count", "registered_publisher_page_count", "hosted_source_found_count",
        "candidate_pdf_count", "fully_compared_pair_count", "known_surface_compared_pair_count",
        "compared_page_count", "status_counts", "mean_changed_cell_fraction", "worst_pages",
    )}, indent=2, sort_keys=True))
    if report["hosted_source_found_count"] < 79:
        raise SystemExit("incomplete hosted source census: see source-safe receipt")
    if report["candidate_pdf_count"] < 1:
        raise SystemExit("no PDF output was produced: check CLI acceptance")


if __name__ == "__main__":
    main()
