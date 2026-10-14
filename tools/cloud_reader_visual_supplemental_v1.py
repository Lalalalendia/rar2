#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from pathlib import Path

from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid

SCHEMA = "chaptera.publisher-visual-supplemental-hosted.v1"
REFERENCE_SCHEMA = "chaptera.publisher-visual-fingerprint.v1"
EXTERNAL_FAMILY = "manual-reduction-family"
EMU_PER_POINT = 12_700.0


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def checked_sha256_digest(value: object, source: str) -> str:
    """Admit only a lowercase digest; never publish its source payload."""
    if not isinstance(value, str) or len(value) != 64 or any(
        char not in "0123456789abcdef" for char in value
    ):
        raise ValueError(f"invalid source-safe SHA-256 digest for {source}")
    return value


def checked_capture_number(value: object, source: str, *, positive: bool = False) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"invalid numeric screenshot capture provenance for {source}")
    number = float(value)
    if not (-1.0e15 < number < 1.0e15):
        raise ValueError(f"out-of-range screenshot capture provenance for {source}")
    if positive and number <= 0:
        raise ValueError(f"non-positive screenshot capture provenance for {source}")
    return number


def checked_capture_state(value: object) -> dict:
    if not isinstance(value, dict):
        raise ValueError("missing screenshot capture provenance")
    expected = {
        "device_pixel_ratio", "scroll_x", "scroll_y", "inner_width", "inner_height",
        "visual_viewport", "svg_rect", "view_box", "screen_ctm",
    }
    if set(value) != expected:
        raise ValueError("unexpected screenshot capture provenance fields")

    checked_capture_number(value["device_pixel_ratio"], "device_pixel_ratio", positive=True)
    checked_capture_number(value["scroll_x"], "scroll_x")
    checked_capture_number(value["scroll_y"], "scroll_y")
    checked_capture_number(value["inner_width"], "inner_width", positive=True)
    checked_capture_number(value["inner_height"], "inner_height", positive=True)

    visual = value["visual_viewport"]
    if visual is not None:
        if not isinstance(visual, dict) or set(visual) != {
            "width", "height", "scale", "offset_left", "offset_top", "page_left", "page_top"
        }:
            raise ValueError("invalid visual viewport provenance")
        checked_capture_number(visual["width"], "visual_viewport.width", positive=True)
        checked_capture_number(visual["height"], "visual_viewport.height", positive=True)
        checked_capture_number(visual["scale"], "visual_viewport.scale", positive=True)
        for key in ("offset_left", "offset_top", "page_left", "page_top"):
            checked_capture_number(visual[key], f"visual_viewport.{key}")

    rect = value["svg_rect"]
    if not isinstance(rect, dict) or set(rect) != {
        "x", "y", "width", "height", "top", "right", "bottom", "left"
    }:
        raise ValueError("invalid SVG client rect provenance")
    for key in ("x", "y", "top", "right", "bottom", "left"):
        checked_capture_number(rect[key], f"svg_rect.{key}")
    checked_capture_number(rect["width"], "svg_rect.width", positive=True)
    checked_capture_number(rect["height"], "svg_rect.height", positive=True)

    view_box = value["view_box"]
    if view_box is None or not isinstance(view_box, dict) or set(view_box) != {
        "x", "y", "width", "height"
    }:
        raise ValueError("missing SVG viewBox provenance")
    checked_capture_number(view_box["x"], "view_box.x")
    checked_capture_number(view_box["y"], "view_box.y")
    checked_capture_number(view_box["width"], "view_box.width", positive=True)
    checked_capture_number(view_box["height"], "view_box.height", positive=True)

    ctm = value["screen_ctm"]
    if ctm is None or not isinstance(ctm, dict) or set(ctm) != {"a", "b", "c", "d", "e", "f"}:
        raise ValueError("missing SVG screen CTM provenance")
    for key in ("a", "b", "c", "d", "e", "f"):
        checked_capture_number(ctm[key], f"screen_ctm.{key}")
    return value


def checked_png_dimensions(value: object) -> dict:
    if not isinstance(value, dict) or set(value) != {"width", "height"}:
        raise ValueError("invalid screenshot PNG dimensions")
    if type(value["width"]) is not int or type(value["height"]) is not int:
        raise ValueError("screenshot PNG dimensions must be integers")
    if value["width"] <= 0 or value["height"] <= 0:
        raise ValueError("screenshot PNG dimensions must be positive")
    return value


def hosted_rows(pairs_csv: Path) -> list[dict[str, str]]:
    rows = list(csv.DictReader(pairs_csv.open(newline="", encoding="utf-8-sig")))
    hosted = [row for row in rows if row["family"] != EXTERNAL_FAMILY]
    if len(rows) != 31:
        raise ValueError(f"expected 31 registered supplemental rows, got {len(rows)}")
    if len(hosted) != 24:
        raise ValueError(f"expected 24 hosted-materializable rows, got {len(hosted)}")
    if sum(int(row["pdf_pages"]) for row in hosted) != 52:
        raise ValueError("hosted supplemental reference-page count drift")
    return hosted


def load_reference(reference_path: Path) -> dict:
    reference = json.loads(reference_path.read_text(encoding="utf-8"))
    if reference.get("schema") != REFERENCE_SCHEMA:
        raise ValueError(f"unsupported supplemental reference schema: {reference.get('schema')!r}")
    if reference.get("pair_count") != 31 or reference.get("reference_page_count") != 59:
        raise ValueError("complete supplemental reference identity drift")
    claims = reference.get("claims", {})
    if claims.get("partial_supplemental_reference_set") is not False:
        raise ValueError("supplemental reference must declare complete reference coverage")
    if claims.get("available_reference_pair_count") != 31:
        raise ValueError("supplemental available-reference pair count drift")
    if claims.get("missing_registered_reference_pair_count") != 0:
        raise ValueError("supplemental reference unexpectedly reports missing registered pairs")
    return reference


def prepare(pairs_csv: Path, source_root: Path, out_dir: Path, manifest: Path) -> dict:
    rows = hosted_rows(pairs_csv)
    candidates: dict[str, list[Path]] = {}
    for path in source_root.rglob("*.pub"):
        candidates.setdefault(sha256(path), []).append(path)

    out_dir.mkdir(parents=True, exist_ok=True)
    fixtures = []
    for row in rows:
        digest = row["pub_sha256"].strip().lower()
        matches = candidates.get(digest, [])
        if len(matches) != 1:
            raise ValueError(f"expected one source for {row['basename']} {digest}, got {len(matches)}")
        source = matches[0]
        expected_bytes = int(row["pub_bytes"])
        if source.stat().st_size != expected_bytes:
            raise ValueError(f"source byte length mismatch for {row['basename']}")
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

    payload = {
        "schema": "chaptera.cloud-reader-real-fixtures.v1",
        "fixtures": fixtures,
    }
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def visual_comparison(browser: dict, browser_receipt: Path, reference: dict) -> dict:
    by_name = {}
    for row in browser.get("results", []):
        name = row.get("fixture")
        if name in by_name:
            raise ValueError(f"duplicate browser fixture {name!r}")
        by_name[name] = row

    hosted_reference_pairs = [
        pair for pair in reference["pairs"]
        if pair["basename"] in by_name
    ]
    external_reference_pairs = [
        pair for pair in reference["pairs"]
        if pair["basename"] not in by_name
    ]
    if len(hosted_reference_pairs) != 24:
        raise ValueError(f"expected 24 hosted reference pairs, got {len(hosted_reference_pairs)}")
    if sum(int(pair["reference_pages"]) for pair in hosted_reference_pairs) != 52:
        raise ValueError("hosted complete-reference page count drift")
    if len(external_reference_pairs) != 7:
        raise ValueError(f"expected 7 external reference pairs, got {len(external_reference_pairs)}")

    page_rows = []
    pair_rows = []
    unavailable = []
    for pair in hosted_reference_pairs:
        fixture = by_name[pair["basename"]]
        if fixture.get("source_sha256") != pair["pub_sha256"]:
            raise ValueError(f"source identity drift for {pair['basename']}")
        if fixture.get("rendered") is not True:
            unavailable.append({
                "fixture": pair["basename"],
                "reason": "reader_not_rendered",
            })
            continue
        candidate_pages = int(fixture.get("pages", 0))
        expected_pages = int(pair["reference_pages"])
        if candidate_pages != expected_pages:
            unavailable.append({
                "fixture": pair["basename"],
                "reason": "page_count_mismatch",
                "candidate_pages": candidate_pages,
                "reference_pages": expected_pages,
            })
            continue

        screenshots = fixture.get("screenshots", [])
        geometry = fixture.get("page_geometry", [])
        if len(screenshots) != candidate_pages or len(geometry) != candidate_pages:
            raise ValueError(f"incomplete browser receipt for {pair['basename']}")

        fractions = []
        for index in range(candidate_pages):
            shot = screenshots[index]
            png = browser_receipt.parent / shot["filename"]
            candidate_raster_sha256 = sha256(png)
            if candidate_raster_sha256 != checked_sha256_digest(shot["sha256"], "candidate PNG"):
                raise ValueError(f"candidate PNG identity drift: {shot['filename']}")
            capture_state_before = checked_capture_state(shot.get("capture_state_before"))
            capture_state_after = checked_capture_state(shot.get("capture_state_after"))
            png_dimensions = checked_png_dimensions(shot.get("png_dimensions"))
            candidate = image_grid(png)
            reference_bytes = reference_grid(pair["pages"][index])
            metrics = compare_grid(candidate, reference_bytes)
            fractions.append(metrics["changed_cell_fraction"])

            page_geometry = geometry[index]
            ref_page = pair["pages"][index]
            width_pt = page_geometry["width_emu"] / EMU_PER_POINT
            height_pt = page_geometry["height_emu"] / EMU_PER_POINT
            page_rows.append({
                "fixture": pair["basename"],
                "oracle_id": pair["oracle_id"],
                "family": pair["family"],
                "warning_state": pair.get("warning_state"),
                "page": index + 1,
                "candidate_raster_sha256": candidate_raster_sha256,
                "candidate_png_dimensions": png_dimensions,
                "capture_state_before": capture_state_before,
                "capture_state_after": capture_state_after,
                "capture_state_changed": capture_state_before != capture_state_after,
                **metrics,
                "reference_media_extent_delta": {
                    "width_pt": round(width_pt - ref_page["media_width_pt"], 6),
                    "height_pt": round(height_pt - ref_page["media_height_pt"], 6),
                },
            })

        pair_rows.append({
            "fixture": pair["basename"],
            "oracle_id": pair["oracle_id"],
            "family": pair["family"],
            "warning_state": pair.get("warning_state"),
            "reference_pages": expected_pages,
            "mean_changed_cell_fraction": sum(fractions) / len(fractions),
            "max_changed_cell_fraction": max(fractions),
        })

    page_rows.sort(key=lambda row: (-row["changed_cell_fraction"], row["fixture"], row["page"]))
    pair_rows.sort(key=lambda row: (-row["mean_changed_cell_fraction"], row["fixture"]))
    fractions = [row["changed_cell_fraction"] for row in page_rows]

    return {
        "available_reference_pair_count": int(reference["pair_count"]),
        "available_reference_page_count": int(reference["reference_page_count"]),
        "hosted_reference_pair_count": len(hosted_reference_pairs),
        "hosted_reference_page_count": sum(int(pair["reference_pages"]) for pair in hosted_reference_pairs),
        "external_reference_pair_count": len(external_reference_pairs),
        "external_reference_page_count": sum(int(pair["reference_pages"]) for pair in external_reference_pairs),
        "compared_pair_count": len(pair_rows),
        "compared_page_count": len(page_rows),
        "capture_state_changed_page_count": sum(
            row["capture_state_changed"] for row in page_rows
        ),
        "capture_state_changed_pages": [
            {
                "fixture": row["fixture"],
                "page": row["page"],
                "candidate_raster_sha256": row["candidate_raster_sha256"],
                "candidate_png_dimensions": row["candidate_png_dimensions"],
                "capture_state_before": row["capture_state_before"],
                "capture_state_after": row["capture_state_after"],
            }
            for row in page_rows
            if row["capture_state_changed"]
        ],
        "unavailable_pair_count": len(unavailable),
        "corpus_mean_changed_cell_fraction": (
            sum(fractions) / len(fractions) if fractions else None
        ),
        "worst_pages": page_rows[:25],
        "pairs": pair_rows,
        "pages": page_rows,
        "unavailable_pairs": unavailable,
        "comparator": {
            "grid_width": 64,
            "grid_height": 64,
            "colorspace": "RGB",
            "resampling": "Pillow.BOX",
            "changed_cell_channel_delta": 12,
        },
    }


def summarize(pairs_csv: Path, browser_receipt: Path, reference_path: Path, out: Path) -> dict:
    rows = hosted_rows(pairs_csv)
    browser = json.loads(browser_receipt.read_text(encoding="utf-8"))
    if browser.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1":
        raise ValueError(f"unsupported browser receipt: {browser.get('protocol')!r}")
    reference = load_reference(reference_path)

    by_name = {}
    for row in browser.get("results", []):
        name = row.get("fixture")
        if name in by_name:
            raise ValueError(f"duplicate browser fixture {name!r}")
        by_name[name] = row

    results = []
    unsupported = []
    page_mismatches = []
    visual_degeneracies = []
    for row in rows:
        name = row["basename"]
        actual = by_name.get(name)
        if actual is None:
            raise ValueError(f"missing browser result for {name}")
        if actual.get("source_sha256") != row["pub_sha256"]:
            raise ValueError(f"source SHA drift for {name}")
        rendered = actual.get("rendered") is True
        worker_receipt_sha256 = (
            checked_sha256_digest(actual.get("worker_receipt_sha256"), "worker receipt")
            if rendered else None
        )
        scene_sha256 = (
            checked_sha256_digest(actual.get("scene_sha256"), "reader Scene")
            if rendered else None
        )
        candidate_pages = actual.get("pages") if rendered else None
        expected_pages = int(row["pdf_pages"])
        page_match = rendered and candidate_pages == expected_pages
        result = {
            "oracle_id": row["oracle_id"],
            "basename": name,
            "family": row["family"],
            "warning_state": row["warning_state"],
            "pub_sha256": row["pub_sha256"],
            "reference_pdf_sha256": row["pdf_sha256"],
            "worker_receipt_sha256": worker_receipt_sha256,
            "scene_sha256": scene_sha256,
            "reference_pages": expected_pages,
            "rendered": rendered,
            "classification": actual.get("classification"),
            "terminal_code": actual.get("terminal_code"),
            "candidate_pages": candidate_pages,
            "page_count_match": page_match,
            "fidelity": actual.get("fidelity"),
            "fidelity_reasons": actual.get("fidelity_reasons", []),
            "diagnostic_codes": actual.get("diagnostic_codes", []),
            "text_layout_fallback_counts": actual.get("text_layout_fallback_counts", {}),
            "browser_preview_census": actual.get("browser_preview_census", {}),
            "visual_degeneracies": actual.get("visual_degeneracies", []),
            "visual_degeneracy_count": int(actual.get("visual_degeneracy_count", 0) or 0),
        }
        results.append(result)
        if not rendered:
            unsupported.append(result)
        elif not page_match:
            page_mismatches.append(result)
        if result["visual_degeneracy_count"] > 0:
            visual_degeneracies.append(result)

    visual = visual_comparison(browser, browser_receipt, reference)
    reference_names = {pair["basename"] for pair in reference["pairs"]}
    missing_hosted_reference_rows = [
        row for row in rows if row["basename"] not in reference_names
    ]

    payload = {
        "schema": SCHEMA,
        "repository_commit_sha": browser.get("repository_commit_sha"),
        "registered_supplemental_pair_count": 31,
        "registered_supplemental_reference_page_count": 59,
        "hosted_pair_count": len(results),
        "hosted_reference_page_count": sum(x["reference_pages"] for x in results),
        "rendered_pair_count": sum(x["rendered"] for x in results),
        "unsupported_pair_count": len(unsupported),
        "page_count_mismatch_pair_count": len(page_mismatches),
        "visual_degeneracy_pair_count": len(visual_degeneracies),
        "visual_degeneracy_count": sum(x["visual_degeneracy_count"] for x in results),
        "font_warning_pair_count": sum(bool(x["warning_state"]) for x in results),
        "external_manual_reduction_pair_count": 7,
        "external_manual_reduction_reference_page_count": 7,
        "visual_reference_fingerprint_state": "COMPLETE_FOR_24_HOSTED_PAIRS_SURFACE_BLOCKED_3",
        "missing_hosted_reference_pair_count": len(missing_hosted_reference_rows),
        "missing_hosted_reference_pairs": [
            {
                "oracle_id": row["oracle_id"],
                "basename": row["basename"],
                "reference_pdf_sha256": row["pdf_sha256"],
                "reference_pages": int(row["pdf_pages"]),
            }
            for row in missing_hosted_reference_rows
        ],
        "visual": visual,
        "claims": {
            "partial_visual_measurement_only": True,
            "complete_registered_reference_material": True,
            "surface_stage_blocks_three_hosted_pairs": True,
            "publisher_pdf_sha_is_identity_only_when_reference_grid_is_missing": True,
            "open_render_and_page_count_are_current_reader_execution_evidence": True,
            "publisher_pdf_page_count_is_not_logical_page_membership_authority": True,
            "surface_stage_must_be_classified_before_count_mismatch_is_semantic": True,
            "pdf_used_as_visual_authority_only": True,
            "fingerprint_used_as_semantic_authority": False,
            "raw_pub_bytes_emitted": False,
            "raw_pdf_bytes_emitted": False,
            "raw_story_text_emitted": False,
            "worker_and_raster_sha256_are_observation_identities_only": True,
            "worker_receipt_sha256_includes_volatile_timings": True,
            "scene_sha256_excludes_worker_timings": True,
            "screenshot_capture_provenance_is_numeric_source_safe": True,
            "screenshot_capture_provenance_does_not_change_rendering": True,
        },
        "pairs": results,
        "unsupported_pairs": unsupported,
        "page_count_mismatches": page_mismatches,
        "visual_degeneracy_pairs": visual_degeneracies,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("prepare")
    p.add_argument("pairs_csv", type=Path)
    p.add_argument("source_root", type=Path)
    p.add_argument("out_dir", type=Path)
    p.add_argument("manifest", type=Path)

    s = sub.add_parser("summarize")
    s.add_argument("pairs_csv", type=Path)
    s.add_argument("browser_receipt", type=Path)
    s.add_argument("reference_fingerprint", type=Path)
    s.add_argument("out", type=Path)

    args = parser.parse_args()
    if args.cmd == "prepare":
        payload = prepare(args.pairs_csv, args.source_root, args.out_dir, args.manifest)
        print(json.dumps({"fixtures": len(payload["fixtures"])}, sort_keys=True))
    else:
        payload = summarize(args.pairs_csv, args.browser_receipt, args.reference_fingerprint, args.out)
        print(json.dumps({
            "hosted_pairs": payload["hosted_pair_count"],
            "hosted_reference_pages": payload["hosted_reference_page_count"],
            "rendered_pairs": payload["rendered_pair_count"],
            "unsupported_pairs": payload["unsupported_pair_count"],
            "page_count_mismatches": payload["page_count_mismatch_pair_count"],
            "visual_degeneracy_pairs": payload["visual_degeneracy_pair_count"],
            "visual_degeneracies": payload["visual_degeneracy_count"],
            "visual_reference_fingerprint_state": payload["visual_reference_fingerprint_state"],
            "visual_compared_pairs": payload["visual"]["compared_pair_count"],
            "visual_compared_pages": payload["visual"]["compared_page_count"],
            "visual_mean_changed_cell_fraction": payload["visual"]["corpus_mean_changed_cell_fraction"],
            "visual_worst_page": payload["visual"]["worst_pages"][0] if payload["visual"]["worst_pages"] else None,
            "missing_hosted_reference_pairs": payload["missing_hosted_reference_pair_count"],
        }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
