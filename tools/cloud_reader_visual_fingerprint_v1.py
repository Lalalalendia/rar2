#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import json
import zlib
from pathlib import Path

from PIL import Image

SCHEMA = "chaptera.publisher-visual-fingerprint.v1"
SUMMARY_SCHEMA = "chaptera.publisher-visual-fingerprint-compare.v1"
GRID_W = 64
GRID_H = 64
CELL_DELTA = 12
EMU_PER_POINT = 12700.0
REFERENCE_SURFACE_STAGES = (
    "logical_page",
    "production_sheet",
    "viewport_spread",
    "unknown",
)


def reference_surface_stage(value: object) -> str:
    if value is None:
        return "unknown"
    if not isinstance(value, str) or value not in REFERENCE_SURFACE_STAGES:
        raise ValueError(f"unsupported reference_surface_stage: {value!r}")
    return value


def reference_surface_stage_counts(rows: list[dict]) -> dict[str, int]:
    counts = {stage: 0 for stage in REFERENCE_SURFACE_STAGES}
    for row in rows:
        counts[reference_surface_stage(row.get("reference_surface_stage"))] += 1
    return counts



def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def image_grid(path: Path) -> bytes:
    with Image.open(path) as image:
        rgb = image.convert("RGB").resize((GRID_W, GRID_H), Image.Resampling.BOX)
        return rgb.tobytes()


def reference_grid(page: dict) -> bytes:
    raw = zlib.decompress(base64.b64decode(page["rgb_zlib_base64"]))
    expected = GRID_W * GRID_H * 3
    if len(raw) != expected:
        raise ValueError(f"reference grid byte length mismatch: {len(raw)} != {expected}")
    if hashlib.sha256(raw).hexdigest() != page["grid_sha256"]:
        raise ValueError("reference grid SHA-256 mismatch")
    return raw


def compare_grid(candidate: bytes, reference: bytes) -> dict:
    if len(candidate) != len(reference):
        raise ValueError("grid length mismatch")
    total_abs = 0
    max_delta = 0
    changed_cells = 0
    cell_count = GRID_W * GRID_H
    for cell in range(cell_count):
        base = cell * 3
        deltas = [
            abs(candidate[base + channel] - reference[base + channel])
            for channel in range(3)
        ]
        total_abs += sum(deltas)
        local_max = max(deltas)
        max_delta = max(max_delta, local_max)
        if local_max >= CELL_DELTA:
            changed_cells += 1
    return {
        "changed_cell_count": changed_cells,
        "changed_cell_fraction": changed_cells / cell_count,
        "mean_abs_channel_delta": total_abs / (cell_count * 3),
        "max_channel_delta": max_delta,
    }


def find_fixture(receipt: dict, name: str) -> dict:
    rows = [row for row in receipt.get("results", []) if row.get("fixture") == name]
    if len(rows) != 1:
        raise ValueError(f"fixture {name!r} is not unique in browser receipt")
    return rows[0]


def prepare_manifest(pairs_csv: Path, natural_root: Path, historical_root: Path, out_dir: Path, manifest: Path) -> dict:
    rows = list(csv.DictReader(pairs_csv.open(newline="", encoding="utf-8-sig")))
    if len(rows) != 55:
        raise ValueError(f"expected 55 pair rows, got {len(rows)}")

    candidates = {}
    for root in [natural_root, historical_root]:
        if not root.exists():
            continue
        for path in root.rglob("*.pub"):
            digest = sha256(path)
            candidates.setdefault(digest, []).append(path)

    out_dir.mkdir(parents=True, exist_ok=True)
    fixtures = []
    for row in rows:
        digest = row["pub_sha256"].strip().lower()
        matches = candidates.get(digest, [])
        if len(matches) != 1:
            raise ValueError(f"expected one source for {digest}, got {len(matches)}")
        source = matches[0]
        expected_bytes = int(row["pub_bytes"])
        if source.stat().st_size != expected_bytes:
            raise ValueError(f"source byte length mismatch for {digest}")
        target = out_dir / row["pub_filename"]
        target.write_bytes(source.read_bytes())
        if sha256(target) != digest:
            raise ValueError(f"copied source SHA mismatch for {digest}")
        fixtures.append({
            "name": row["basename"],
            "sha256": digest,
            "bytes": expected_bytes,
            "require_render": False,
            "require_shared_text": False,
            "source_path": str(target.resolve()),
        })

    payload = {"schema": "chaptera.cloud-reader-real-fixtures.v1", "fixtures": fixtures}
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def compare(browser_receipt: Path, reference_path: Path, out_path: Path) -> dict:
    browser = json.loads(browser_receipt.read_text(encoding="utf-8"))
    if browser.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1":
        raise ValueError(f"unsupported browser receipt: {browser.get('protocol')!r}")

    reference = json.loads(reference_path.read_text(encoding="utf-8"))
    if reference.get("schema") != SCHEMA:
        raise ValueError(f"unsupported reference schema: {reference.get('schema')!r}")
    if reference.get("pair_count") != 55 or reference.get("reference_page_count") != 162:
        raise ValueError("Batch 01 reference identity drift")

    pair_rows = []
    page_rows = []
    unsupported = []
    page_count_mismatch = 0

    for pair in reference["pairs"]:
        surface_stage = reference_surface_stage(pair.get("reference_surface_stage"))
        fixture = find_fixture(browser, pair["basename"])
        if fixture.get("source_sha256") != pair["pub_sha256"]:
            raise ValueError(f"source identity drift for {pair['basename']}")

        rendered = fixture.get("rendered") is True
        candidate_pages = fixture.get("pages") if rendered else None
        expected_pages = pair["reference_pages"]
        if rendered and candidate_pages != expected_pages:
            page_count_mismatch += 1

        pair_metrics = []
        if not rendered:
            unsupported.append({
                "fixture": pair["basename"],
                "classification": fixture.get("classification"),
                "terminal_code": fixture.get("terminal_code"),
                "reference_state": pair["reference_state"],
                "reference_surface_stage": surface_stage,
            })
        else:
            shots = fixture.get("screenshots", [])
            geometry = fixture.get("page_geometry", [])
            if len(shots) != candidate_pages or len(geometry) != candidate_pages:
                raise ValueError(f"incomplete browser receipt for {pair['basename']}")

            for index in range(min(candidate_pages, expected_pages)):
                shot = shots[index]
                png = browser_receipt.parent / shot["filename"]
                if sha256(png) != shot["sha256"]:
                    raise ValueError(f"candidate PNG identity drift: {shot['filename']}")
                candidate = image_grid(png)
                reference_bytes = reference_grid(pair["pages"][index])
                metrics = compare_grid(candidate, reference_bytes)
                page_geometry = geometry[index]
                width_pt = page_geometry["width_emu"] / EMU_PER_POINT
                height_pt = page_geometry["height_emu"] / EMU_PER_POINT
                ref_page = pair["pages"][index]
                media_delta = {
                    "width_pt": round(width_pt - ref_page["media_width_pt"], 6),
                    "height_pt": round(height_pt - ref_page["media_height_pt"], 6),
                }
                row = {
                    "fixture": pair["basename"],
                    "page": index + 1,
                    "reference_state": pair["reference_state"],
                    "reference_surface_stage": surface_stage,
                    "candidate_page_identity_sha256": page_geometry.get("page_identity_sha256"),
                    **metrics,
                    "reference_media_extent_delta": media_delta,
                }
                page_rows.append(row)
                pair_metrics.append(metrics["changed_cell_fraction"])

        pair_rows.append({
            "fixture": pair["basename"],
            "reference_state": pair["reference_state"],
            "reference_surface_stage": surface_stage,
            "rendered": rendered,
            "candidate_pages": candidate_pages,
            "reference_pages": expected_pages,
            "page_count_match": candidate_pages == expected_pages if rendered else None,
            "mean_changed_cell_fraction": (
                sum(pair_metrics) / len(pair_metrics) if pair_metrics else None
            ),
            "max_changed_cell_fraction": max(pair_metrics) if pair_metrics else None,
        })

    page_rows.sort(key=lambda row: (-row["changed_cell_fraction"], row["fixture"], row["page"]))
    fractions = [row["changed_cell_fraction"] for row in page_rows]
    summary = {
        "schema": SUMMARY_SCHEMA,
        "batch_id": reference["batch_id"],
        "repository_commit_sha": browser.get("repository_commit_sha"),
        "pair_count": len(reference["pairs"]),
        "reference_page_count": reference["reference_page_count"],
        "compared_page_count": len(page_rows),
        "unsupported_pair_count": len(unsupported),
        "page_count_mismatch_pair_count": page_count_mismatch,
        "reference_surface_stage_counts": reference_surface_stage_counts(pair_rows),
        "corpus_mean_changed_cell_fraction": sum(fractions) / len(fractions) if fractions else None,
        "worst_pages": page_rows[:25],
        "pages": page_rows,
        "pairs": pair_rows,
        "unsupported_pairs": unsupported,
        "comparator": {
            "grid_width": GRID_W,
            "grid_height": GRID_H,
            "colorspace": "RGB",
            "resampling": "Pillow.BOX",
            "changed_cell_channel_delta": CELL_DELTA,
        },
        "claims": {
            "source_free_coarse_visual_regression": True,
            "pixel_exact_visual_parity": False,
            "pdf_used_as_visual_authority_only": True,
            "fingerprint_used_as_semantic_authority": False,
            "missing_reference_surface_stage_defaults_to_unknown": True,
            "reference_surface_stage_inference_from_raster": False,
            "raw_pub_bytes_emitted": False,
            "raw_pdf_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return summary



def compare_baseline(current_path: Path, baseline_path: Path, out_path: Path) -> dict:
    current = json.loads(current_path.read_text(encoding="utf-8"))
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    if current.get("schema") != SUMMARY_SCHEMA or baseline.get("schema") != SUMMARY_SCHEMA:
        raise ValueError("unsupported visual summary schema for baseline comparison")
    if current.get("batch_id") != baseline.get("batch_id"):
        raise ValueError("visual baseline batch identity mismatch")

    def pages_by_key(payload: dict) -> dict[tuple[str, int], dict]:
        return {
            (row["fixture"], int(row["page"])): row
            for row in payload.get("pages", [])
        }

    current_pages = pages_by_key(current)
    baseline_pages = pages_by_key(baseline)
    keys = sorted(set(current_pages) & set(baseline_pages))
    epsilon = 1e-9
    improved = []
    regressed = []
    unchanged = []
    remapped = []
    identity_unknown = []
    stable_identity_keys = []
    for key in keys:
        current_identity = current_pages[key].get("candidate_page_identity_sha256")
        baseline_identity = baseline_pages[key].get("candidate_page_identity_sha256")
        if current_identity is None or baseline_identity is None:
            identity_unknown.append({
                "fixture": key[0],
                "page": key[1],
                "baseline_page_identity_sha256": baseline_identity,
                "current_page_identity_sha256": current_identity,
            })
            continue
        if current_identity != baseline_identity:
            remapped.append({
                "fixture": key[0],
                "page": key[1],
                "baseline_page_identity_sha256": baseline_identity,
                "current_page_identity_sha256": current_identity,
            })
            continue
        stable_identity_keys.append(key)

    for key in stable_identity_keys:
        now = current_pages[key]["changed_cell_fraction"]
        before = baseline_pages[key]["changed_cell_fraction"]
        delta = now - before
        row = {
            "fixture": key[0],
            "page": key[1],
            "page_identity_sha256": current_pages[key].get("candidate_page_identity_sha256"),
            "baseline_changed_cell_fraction": before,
            "current_changed_cell_fraction": now,
            "delta": delta,
        }
        if delta < -epsilon:
            improved.append(row)
        elif delta > epsilon:
            regressed.append(row)
        else:
            unchanged.append(row)

    improved.sort(key=lambda row: row["delta"])
    regressed.sort(key=lambda row: -row["delta"])
    mean_before = baseline.get("corpus_mean_changed_cell_fraction")
    mean_now = current.get("corpus_mean_changed_cell_fraction")
    payload = {
        "schema": "chaptera.publisher-visual-fingerprint-delta.v1",
        "batch_id": current["batch_id"],
        "baseline_repository_commit_sha": baseline.get("repository_commit_sha"),
        "current_repository_commit_sha": current.get("repository_commit_sha"),
        "matched_page_count": len(keys),
        "stable_identity_page_count": len(stable_identity_keys),
        "remapped_page_count": len(remapped),
        "identity_unknown_page_count": len(identity_unknown),
        "improved_page_count": len(improved),
        "regressed_page_count": len(regressed),
        "unchanged_page_count": len(unchanged),
        "corpus_mean_changed_cell_fraction": {
            "baseline": mean_before,
            "current": mean_now,
            "delta": (
                mean_now - mean_before
                if mean_now is not None and mean_before is not None
                else None
            ),
        },
        "unsupported_pair_count": {
            "baseline": baseline.get("unsupported_pair_count"),
            "current": current.get("unsupported_pair_count"),
        },
        "page_count_mismatch_pair_count": {
            "baseline": baseline.get("page_count_mismatch_pair_count"),
            "current": current.get("page_count_mismatch_pair_count"),
        },
        "reference_surface_stage_counts": {
            "baseline": reference_surface_stage_counts(baseline.get("pairs", [])),
            "current": reference_surface_stage_counts(current.get("pairs", [])),
        },
        "largest_improvements": improved[:25],
        "largest_regressions": regressed[:25],
        "page_identity_remaps": remapped[:100],
        "page_identity_unknown": identity_unknown[:100],
        "claims": {
            "measurement_only": True,
            "hard_regression_threshold_applied": False,
            "visual_delta_only_compares_stable_page_identity": True,
            "page_identity_remap_is_not_visual_regression_evidence": True,
        },
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload

def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)

    prep = sub.add_parser("prepare")
    prep.add_argument("pairs_csv", type=Path)
    prep.add_argument("natural_root", type=Path)
    prep.add_argument("historical_root", type=Path)
    prep.add_argument("out_dir", type=Path)
    prep.add_argument("manifest", type=Path)

    comp = sub.add_parser("compare")
    comp.add_argument("browser_receipt", type=Path)
    comp.add_argument("reference", type=Path)
    comp.add_argument("out", type=Path)

    delta = sub.add_parser("delta")
    delta.add_argument("current", type=Path)
    delta.add_argument("baseline", type=Path)
    delta.add_argument("out", type=Path)

    args = parser.parse_args()
    if args.command == "prepare":
        payload = prepare_manifest(args.pairs_csv, args.natural_root, args.historical_root, args.out_dir, args.manifest)
        print(f"BATCH01 VISUAL MANIFEST fixtures={len(payload['fixtures'])}")
    elif args.command == "compare":
        summary = compare(args.browser_receipt, args.reference, args.out)
        worst = summary["worst_pages"][0] if summary["worst_pages"] else None
        print(json.dumps({
            "compared_pages": summary["compared_page_count"],
            "unsupported_pairs": summary["unsupported_pair_count"],
            "page_count_mismatch_pairs": summary["page_count_mismatch_pair_count"],
            "mean_changed_cell_fraction": summary["corpus_mean_changed_cell_fraction"],
            "worst_page": worst,
        }, indent=2, sort_keys=True))
    else:
        delta = compare_baseline(args.current, args.baseline, args.out)
        print(json.dumps({
            "matched_pages": delta["matched_page_count"],
            "stable_identity_pages": delta["stable_identity_page_count"],
            "remapped_pages": delta["remapped_page_count"],
            "identity_unknown_pages": delta["identity_unknown_page_count"],
            "improved_pages": delta["improved_page_count"],
            "regressed_pages": delta["regressed_page_count"],
            "unchanged_pages": delta["unchanged_page_count"],
            "mean_delta": delta["corpus_mean_changed_cell_fraction"]["delta"],
        }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
