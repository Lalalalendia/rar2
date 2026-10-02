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
                    **metrics,
                    "reference_media_extent_delta": media_delta,
                }
                page_rows.append(row)
                pair_metrics.append(metrics["changed_cell_fraction"])

        pair_rows.append({
            "fixture": pair["basename"],
            "reference_state": pair["reference_state"],
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
        "corpus_mean_changed_cell_fraction": sum(fractions) / len(fractions) if fractions else None,
        "worst_pages": page_rows[:25],
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
            "raw_pub_bytes_emitted": False,
            "raw_pdf_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return summary


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

    args = parser.parse_args()
    if args.command == "prepare":
        payload = prepare_manifest(args.pairs_csv, args.natural_root, args.historical_root, args.out_dir, args.manifest)
        print(f"BATCH01 VISUAL MANIFEST fixtures={len(payload['fixtures'])}")
    else:
        summary = compare(args.browser_receipt, args.reference, args.out)
        worst = summary["worst_pages"][0] if summary["worst_pages"] else None
        print(json.dumps({
            "compared_pages": summary["compared_page_count"],
            "unsupported_pairs": summary["unsupported_pair_count"],
            "page_count_mismatch_pairs": summary["page_count_mismatch_pair_count"],
            "mean_changed_cell_fraction": summary["corpus_mean_changed_cell_fraction"],
            "worst_page": worst,
        }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
