#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

SCHEMA = "chaptera.publisher-visual-perceptual-oracle.v1"
RECEIPT_SCHEMA = "chaptera.publisher-visual-perceptual-comparison.v1"
CLOUD_PROTOCOL = "chaptera.cloud-reader-real-scene-browser.v1"
EMU_PER_POINT = 12_700.0
PHYSICAL_SIZE_TOL_PT = 0.1
PAGE_REGRESSION_TOL = 0.08
MEAN_REGRESSION_TOL = 0.03


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _bits_hex(bits: np.ndarray) -> str:
    flat = np.asarray(bits, dtype=np.uint8).flatten()
    pad = (-len(flat)) % 8
    if pad:
        flat = np.concatenate([flat, np.zeros(pad, dtype=np.uint8)])
    return np.packbits(flat).tobytes().hex()


def _norm_hist(vals: np.ndarray) -> list[int]:
    vals = np.asarray(vals, dtype=np.float64)
    total = float(vals.sum())
    if total <= 0:
        return [0] * len(vals)
    return np.rint(vals / total * 255.0).astype(int).tolist()


def perceptual_from_image(path: Path) -> dict:
    with Image.open(path) as im0:
        im = im0.convert("RGB")
        g = np.asarray(im.convert("L").resize((9, 8), Image.Resampling.LANCZOS), dtype=np.int16)
        dh = (g[:, 1:] > g[:, :-1]).astype(np.uint8)
        a = np.asarray(im.convert("L").resize((8, 8), Image.Resampling.LANCZOS), dtype=np.float32)
        ah = (a > a.mean()).astype(np.uint8)
        small = np.asarray(im.convert("L").resize((16, 16), Image.Resampling.LANCZOS), dtype=np.float32)
        threshold = min(245.0, float(np.median(small)))
        ih = (small < threshold).astype(np.uint8)
        rgb = np.asarray(im.resize((128, 128), Image.Resampling.BILINEAR), dtype=np.uint8)

    color: list[int] = []
    for channel in range(3):
        hist, _ = np.histogram(rgb[..., channel], bins=8, range=(0, 256))
        color.extend(_norm_hist(hist))
    gray = np.dot(rgb[..., :3], [0.299, 0.587, 0.114]).astype(np.float32)
    gx = np.zeros_like(gray)
    gy = np.zeros_like(gray)
    gx[:, 1:] = np.abs(gray[:, 1:] - gray[:, :-1])
    gy[1:, :] = np.abs(gray[1:, :] - gray[:-1, :])
    mag = np.clip((gx + gy) / 2, 0, 255)
    edge, _ = np.histogram(mag, bins=8, range=(0, 256))
    return {
        "dhash64": _bits_hex(dh),
        "ahash64": _bits_hex(ah),
        "inkhash256": _bits_hex(ih),
        "color_hist24": color,
        "edge_hist8": _norm_hist(edge),
    }


def _hamming(hex_a: str, hex_b: str, bits: int) -> float:
    a = int(hex_a, 16)
    b = int(hex_b, 16)
    return (a ^ b).bit_count() / bits


def _hist_l1(a: list[int], b: list[int]) -> float:
    if len(a) != len(b):
        raise ValueError("histogram length mismatch")
    # each normalized histogram group sums roughly to 255; divide by the
    # theoretical two-sided maximum for a stable 0..1-ish distance.
    return sum(abs(int(x) - int(y)) for x, y in zip(a, b)) / (2.0 * 255.0 * max(1, len(a) // 8))


def compare_signature(candidate: dict, reference: dict) -> dict:
    dh = _hamming(candidate["dhash64"], reference["dhash64"], 64)
    ah = _hamming(candidate["ahash64"], reference["ahash64"], 64)
    ink = _hamming(candidate["inkhash256"], reference["inkhash256"], 256)
    color = _hist_l1(candidate["color_hist24"], reference["color_hist24"])
    edge = _hist_l1(candidate["edge_hist8"], reference["edge_hist8"])
    score = 0.20 * dh + 0.15 * ah + 0.35 * ink + 0.20 * color + 0.10 * edge
    return {
        "score": round(score, 9),
        "dhash_distance": round(dh, 9),
        "ahash_distance": round(ah, 9),
        "inkhash_distance": round(ink, 9),
        "color_hist_distance": round(color, 9),
        "edge_hist_distance": round(edge, 9),
    }


def _fixture_map(cloud: dict) -> dict[str, dict]:
    out: dict[str, dict] = {}
    for row in cloud.get("results", []):
        name = row.get("fixture")
        if not isinstance(name, str) or not name or name in out:
            raise ValueError("Cloud receipt fixture names must be unique")
        out[name] = row
    return out


def _baseline_maps(baseline: dict | None):
    page = {}
    pair = {}
    if baseline:
        for p in baseline.get("pairs", []):
            pair[p["fixture"]] = p
            for row in p.get("pages", []):
                if row.get("score") is not None:
                    page[(p["fixture"], int(row["page"]))] = float(row["score"])
    return page, pair


def compare(
    cloud_path: Path,
    reference_path: Path,
    output_path: Path,
    baseline_path: Path | None = None,
    admission_path: Path | None = None,
) -> dict:
    cloud = json.loads(cloud_path.read_text(encoding="utf-8"))
    reference = json.loads(reference_path.read_text(encoding="utf-8"))
    baseline = json.loads(baseline_path.read_text(encoding="utf-8")) if baseline_path else None
    admission = json.loads(admission_path.read_text(encoding="utf-8")) if admission_path else None
    if cloud.get("protocol") != CLOUD_PROTOCOL:
        raise ValueError(f"unsupported Cloud receipt protocol: {cloud.get('protocol')!r}")
    if reference.get("schema") != SCHEMA:
        raise ValueError(f"unsupported reference schema: {reference.get('schema')!r}")
    fixtures = _fixture_map(cloud)
    baseline_pages, baseline_pairs = _baseline_maps(baseline)
    excluded_by_id = {}
    if admission is not None:
        if admission.get("schema") != "chaptera.publisher-visual-golden-ci-admission.v1":
            raise ValueError(f"unsupported admission schema: {admission.get('schema')!r}")
        if admission.get("batch_id") != reference.get("batch_id"):
            raise ValueError("admission batch_id does not match reference batch")
        for item in admission.get("excluded", []):
            oid = item.get("oracle_id")
            if not isinstance(oid, str) or not oid or oid in excluded_by_id:
                raise ValueError("admission exclusions must have unique oracle_id values")
            excluded_by_id[oid] = item

    pairs = []
    ranking = []
    regressions = []
    unsupported = 0
    excluded = 0
    admitted_reference_pages = 0
    page_count_mismatch = 0
    physical_mismatch = 0

    for ref_pair in reference["pairs"]:
        name = ref_pair["fixture"]
        exclusion = excluded_by_id.get(ref_pair["oracle_id"])
        if exclusion is not None:
            excluded += 1
            pairs.append({
                "oracle_id": ref_pair["oracle_id"],
                "fixture": name,
                "admitted": False,
                "exclusion_reason": exclusion.get("reason"),
                "reference_pages": ref_pair["pdf_pages"],
                "pages": [],
            })
            continue
        admitted_reference_pages += int(ref_pair["pdf_pages"])
        fixture = fixtures.get(name)
        if fixture is None:
            raise ValueError(f"missing admitted Cloud fixture {name}")
        if fixture.get("source_sha256") != ref_pair["pub_sha256"]:
            raise ValueError(f"source SHA drift for {name}")
        row = {
            "oracle_id": ref_pair["oracle_id"],
            "fixture": name,
            "admitted": True,
            "source_pub_sha256": ref_pair["pub_sha256"],
            "rendered": bool(fixture.get("rendered")),
            "classification": fixture.get("classification"),
            "terminal_code": fixture.get("terminal_code"),
            "candidate_pages": fixture.get("pages") if fixture.get("rendered") else None,
            "reference_pages": ref_pair["pdf_pages"],
            "page_count_match": None,
            "pages": [],
        }
        if not fixture.get("rendered"):
            unsupported += 1
            if baseline_pairs and baseline_pairs.get(name, {}).get("rendered") is True:
                regressions.append({"fixture": name, "kind": "newly_unrendered"})
            pairs.append(row)
            continue

        row["page_count_match"] = fixture.get("pages") == ref_pair["pdf_pages"]
        if not row["page_count_match"]:
            page_count_mismatch += 1
            if baseline_pairs and baseline_pairs.get(name, {}).get("page_count_match") is True:
                regressions.append({"fixture": name, "kind": "page_count_regression"})
        shots = fixture.get("screenshots", [])
        geometry = fixture.get("page_geometry", [])
        if len(shots) != fixture.get("pages") or len(geometry) != fixture.get("pages"):
            raise ValueError(f"incomplete page receipt for {name}")

        for idx in range(min(fixture["pages"], len(ref_pair["pages"]))):
            shot = shots[idx]
            candidate_path = cloud_path.parent / shot["filename"]
            if file_sha256(candidate_path) != shot["sha256"]:
                raise ValueError(f"candidate PNG identity drift: {shot['filename']}")
            signature = perceptual_from_image(candidate_path)
            diff = compare_signature(signature, ref_pair["pages"][idx])
            g = geometry[idx]
            width_pt = float(g["width_emu"]) / EMU_PER_POINT
            height_pt = float(g["height_emu"]) / EMU_PER_POINT
            ref_page = ref_pair["pages"][idx]
            width_delta = width_pt - float(ref_page["width_pt"])
            height_delta = height_pt - float(ref_page["height_pt"])
            size_match = abs(width_delta) <= PHYSICAL_SIZE_TOL_PT and abs(height_delta) <= PHYSICAL_SIZE_TOL_PT
            if not size_match:
                physical_mismatch += 1
            page_row = {
                "page": idx + 1,
                **diff,
                "physical_size_match": size_match,
                "physical_size_delta_pt": {"width": round(width_delta, 6), "height": round(height_delta, 6)},
                "candidate_png_sha256": shot["sha256"],
            }
            base_score = baseline_pages.get((name, idx + 1))
            if base_score is not None:
                delta = diff["score"] - base_score
                page_row["baseline_score"] = round(base_score, 9)
                page_row["baseline_delta"] = round(delta, 9)
                if delta > PAGE_REGRESSION_TOL:
                    regressions.append({"fixture": name, "page": idx + 1, "kind": "perceptual_regression", "baseline_score": round(base_score, 9), "score": diff["score"], "delta": round(delta, 9)})
            row["pages"].append(page_row)
            ranking.append({"fixture": name, "page": idx + 1, **diff})
        pairs.append(row)

    ranking.sort(key=lambda x: x["score"], reverse=True)
    scores = [x["score"] for x in ranking]
    mean = sum(scores) / len(scores) if scores else None
    maximum = max(scores) if scores else None
    if baseline is not None and scores:
        old = [float(p["score"]) for pair in baseline.get("pairs", []) for p in pair.get("pages", []) if p.get("score") is not None]
        if old:
            old_mean = sum(old) / len(old)
            if mean - old_mean > MEAN_REGRESSION_TOL:
                regressions.append({"kind":"mean_perceptual_regression","baseline_mean":round(old_mean,9),"mean":round(mean,9),"delta":round(mean-old_mean,9)})

    receipt = {
        "schema": RECEIPT_SCHEMA,
        "repository_commit_sha": cloud.get("repository_commit_sha"),
        "reference_batch_id": reference.get("batch_id"),
        "reference_sha256": file_sha256(reference_path),
        "pair_count": len(pairs),
        "admitted_pair_count": len(pairs) - excluded,
        "excluded_pair_count": excluded,
        "reference_page_count": reference.get("reference_page_count"),
        "admitted_reference_page_count": admitted_reference_pages,
        "compared_page_count": len(ranking),
        "unsupported_pair_count": unsupported,
        "page_count_mismatch_pair_count": page_count_mismatch,
        "physical_size_mismatch_page_count": physical_mismatch,
        "mean_score": round(mean,9) if mean is not None else None,
        "max_score": round(maximum,9) if maximum is not None else None,
        "ranking": ranking[:50],
        "regressions": regressions,
        "pairs": pairs,
        "comparator": {
            "weights": {"dhash":0.20,"ahash":0.15,"inkhash":0.35,"color_hist":0.20,"edge_hist":0.10},
            "page_regression_tolerance": PAGE_REGRESSION_TOL,
            "mean_regression_tolerance": MEAN_REGRESSION_TOL,
            "physical_size_tolerance_pt": PHYSICAL_SIZE_TOL_PT,
        },
        "claims": {"source_free_reference":True,"raw_reference_pdf_bytes_used_at_runtime":False,"raw_story_text_emitted":False,"perceptual_smoke_not_pixel_parity":True,"pdf_visual_authority_only":True,"not_semantic_authority":True},
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True)+"\n",encoding="utf-8")
    return receipt


def main(argv: list[str]) -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("cloud_receipt", type=Path)
    parser.add_argument("reference", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--admission", type=Path)
    args = parser.parse_args(argv[1:])
    receipt=compare(args.cloud_receipt,args.reference,args.output,args.baseline,args.admission)
    print(json.dumps({"pair_count":receipt["pair_count"],"compared_page_count":receipt["compared_page_count"],"unsupported_pair_count":receipt["unsupported_pair_count"],"page_count_mismatch_pair_count":receipt["page_count_mismatch_pair_count"],"physical_size_mismatch_page_count":receipt["physical_size_mismatch_page_count"],"mean_score":receipt["mean_score"],"max_score":receipt["max_score"],"regression_count":len(receipt["regressions"]),"top10":receipt["ranking"][:10]},indent=2,sort_keys=True))
    if args.baseline is not None and receipt["regressions"]:
        raise SystemExit(2)

if __name__ == '__main__':
    main(sys.argv)