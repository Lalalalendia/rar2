#!/usr/bin/env python3
"""Exact-source, one-head Reader/CLI-PDF/Publisher 64x64 RGB diagnostic.

The native Publisher reference is *screening-only* when surface stage is unknown.
No PUB, font, full PDF, screenshot or source text leaves the CI runner.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

import fitz
from PIL import Image

from cloud_reader_visual_fingerprint_v1 import (
    GRID_H, GRID_W, compare_grid, reference_grid, image_grid, sha256,
    reference_surface_stage,
)

SCHEMA = "chaptera.reader-pdf-022-screen-census.v1"
EXPECTED = {
    "001_0ca858ed4806e81d": "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade",
    "022_7079ad60fd810979": "7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c",
    "023_6bbfbf7b4c9b2400": "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8",
}
CODE = re.compile(r"^[a-zA-Z][a-zA-Z0-9_.-]{0,119}$")
DPI = 144
CHANNEL_THRESHOLD = 12


def require_reference(path: Path) -> dict[str, dict]:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if raw.get("schema") != "chaptera.publisher-visual-fingerprint.v1" or raw.get("pair_count") != 55:
        raise ValueError("unexpected pinned Batch01 reference census")
    pairs = {x["basename"]: x for x in raw.get("pairs", [])}
    if len(pairs) != 55:
        raise ValueError("duplicate or incomplete registered Batch01 references")
    for fixture, digest in EXPECTED.items():
        p = pairs.get(fixture)
        if p is None or p.get("pub_sha256") != digest or p.get("reference_pages") != 1 or len(p["pages"]) != 1:
            raise ValueError("exact source or one-page reference identity drift")
        reference_grid(p["pages"][0])
        reference_surface_stage(p.get("reference_surface_stage"))
    return pairs


def prepare(source_root: Path, reference: Path, output: Path) -> dict:
    require_reference(reference)
    fixtures = []
    for name, digest in EXPECTED.items():
        path = source_root / f"{digest}.pub"
        if not path.is_file() or sha256(path) != digest:
            raise ValueError("pinned PUB file absent or source SHA mismatch")
        size = path.stat().st_size
        if size <= 0 or size > 100 * 1024 * 1024:
            raise ValueError("PUB byte size outside bounded research admission")
        fixtures.append({"name": name, "sha256": digest, "bytes": size,
                         "require_render": False, "require_shared_text": False,
                         "source_path": str(path.resolve())})
    manifest = {"schema": "chaptera.cloud-reader-real-fixtures.v1", "fixtures": fixtures}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return manifest


def source_safe_codes(values: object) -> list[str]:
    if not isinstance(values, list) or len(values) > 2048:
        raise ValueError("unbounded source diagnostic-code list")
    out = set()
    for entry in values:
        if not isinstance(entry, str) or not CODE.fullmatch(entry):
            raise ValueError("raw or unsafe diagnostic code rejected")
        out.add(entry)
    return sorted(out)


def source_safe_counts(values: object) -> dict[str, int]:
    if not isinstance(values, dict) or len(values) > 2048:
        raise ValueError("unbounded source diagnostic group")
    out = {}
    for name, count in values.items():
        if not isinstance(name, str) or not CODE.fullmatch(name):
            raise ValueError("raw or unsafe count code rejected")
        if not isinstance(count, int) or isinstance(count, bool) or not 0 <= count <= 100000:
            raise ValueError("invalid source diagnostic count")
        out[name] = count
    return dict(sorted(out.items()))


def compare_cells(a: bytes, b: bytes) -> tuple[int, list[int]]:
    if len(a) != GRID_W * GRID_H * 3 or len(b) != len(a):
        raise ValueError("invalid RGB grid shape")
    changed = 0
    tile_counts = [0] * 16  # 4x4 grid of 16x16 pixel cells
    for cell in range(GRID_W * GRID_H):
        pos = cell * 3
        flagged = max(abs(a[pos + k] - b[pos + k]) for k in range(3)) >= CHANNEL_THRESHOLD
        if flagged:
            changed += 1
            tile_counts[(cell // GRID_W // 16) * 4 + (cell % GRID_W // 16)] += 1
    assert changed == sum(tile_counts)
    return changed, tile_counts


def occupancy(rgb: bytes) -> dict:
    if len(rgb) != GRID_W * GRID_H * 3:
        raise ValueError("invalid occupancy source grid")
    n = GRID_W * GRID_H
    sums = [0, 0, 0]
    whites = blacks = chroma = 0
    for i in range(0, len(rgb), 3):
        pixel = rgb[i:i+3]
        for k in range(3): sums[k] += pixel[k]
        whites += all(channel >= 245 for channel in pixel)
        blacks += all(channel <= 20 for channel in pixel)
        chroma += max(pixel) - min(pixel) >= 20
    return {"mean_rgb": [round(x / n, 3) for x in sums],
            "near_white_cell_count": whites,
            "near_black_cell_count": blacks,
            "chroma_cell_count": chroma}


def pdf_page_rgb(path: Path) -> tuple[bytes, int, tuple[float, float]]:
    with fitz.open(path) as doc:
        if doc.page_count != 1:
            raise ValueError("PDF one-page cardinality mismatch")
        page = doc[0]
        if page.rect.width <= 0 or page.rect.height <= 0 or page.rect.width * page.rect.height * 4 > 60_000_000:
            raise ValueError("invalid or unbounded PDF page extent")
        pix = page.get_pixmap(dpi=DPI, colorspace=fitz.csRGB, alpha=False)
        img = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
        return img.resize((GRID_W, GRID_H), Image.Resampling.BOX).tobytes(), doc.page_count, (float(page.rect.width), float(page.rect.height))


def collect(browser_dir: Path, pdf_dir: Path, reference: Path, commit: str) -> dict:
    if not re.fullmatch(r"[a-f0-9]{40}", commit):
        raise ValueError("exact code head SHA required")
    refs = require_reference(reference)
    receipt = json.loads((browser_dir / "receipt.json").read_text(encoding="utf-8"))
    if receipt.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1" or receipt.get("repository_commit_sha") != commit:
        raise ValueError("Reader source/head identity mismatch")
    rows = receipt.get("results", [])
    if not isinstance(rows, list) or len(rows) != len(EXPECTED):
        raise ValueError("Reader witness/controls count mismatch")
    indexed = {r.get("fixture"): r for r in rows}
    if set(indexed) != set(EXPECTED):
        raise ValueError("Reader controls/source set drift")
    output = []
    for name, sha in EXPECTED.items():
        item, oracle = indexed[name], refs[name]
        if item.get("source_sha256") != sha or item.get("rendered") is not True or item.get("pages") != 1:
            raise ValueError("Reader source, coverage or render failure")
        shots = item.get("screenshots", [])
        if len(shots) != 1 or shots[0].get("page") != 1:
            raise ValueError("Reader screenshot cardinality drift")
        filename = f"{name}-page-1.png"
        if shots[0].get("filename") != filename or not re.fullmatch(r"[a-f0-9]{64}", str(shots[0].get("sha256", ""))):
            raise ValueError("Reader screenshot file identity unsupported")
        screenshot = browser_dir / filename
        if sha256(screenshot) != shots[0]["sha256"]:
            raise ValueError("Reader screenshot digest mismatch")
        reader_grid = image_grid(screenshot)
        pdf_file = pdf_dir / f"{name}.pdf"
        if not pdf_file.is_file():
            raise ValueError("exact converted PDF missing")
        pdf_grid, _, media = pdf_page_rgb(pdf_file)
        ref = oracle["pages"][0]
        if abs(media[0] - float(ref["media_width_pt"])) > .05 or abs(media[1] - float(ref["media_height_pt"])) > .05:
            raise ValueError("PDF Publisher reference media extent mismatch")
        if len(item.get("page_geometry", [])) != 1:
            raise ValueError("Reader page geometry missing")
        geometry = item["page_geometry"][0]
        if abs(geometry["width_emu"] / 12700. - float(ref["media_width_pt"])) > .05 or abs(geometry["height_emu"] / 12700. - float(ref["media_height_pt"])) > .05:
            raise ValueError("Reader Publisher reference media extent mismatch")
        ref_rgb = reference_grid(ref)
        rscore = compare_grid(reader_grid, ref_rgb)
        pscore = compare_grid(pdf_grid, ref_rgb)
        dscore = compare_grid(reader_grid, pdf_grid)
        rc, rtiles = compare_cells(reader_grid, ref_rgb)
        pc, ptiles = compare_cells(pdf_grid, ref_rgb)
        dc, dtiles = compare_cells(reader_grid, pdf_grid)
        assert rc == rscore["changed_cell_count"] and pc == pscore["changed_cell_count"] and dc == dscore["changed_cell_count"]
        sidecar = json.loads((pdf_dir / f"{name}.pdf.loss.json").read_text(encoding="utf-8"))
        if sidecar.get("conversion_profile", {}).get("source", {}).get("source_sha256") != sha:
            raise ValueError("fixed PDF source SHA mismatch")
        pdf_data = sidecar.get("pdf", {})
        codes = source_safe_codes([str(node.get("code", "")) for node in pdf_data.get("nodes", [])])
        counts = Counter(str(node.get("code", "")) for node in pdf_data.get("nodes", []))
        dispositions = Counter(str(node.get("disposition", "")) for node in pdf_data.get("nodes", []))
        result = {"fixture":name, "source_sha256":sha, "reference_surface_stage":reference_surface_stage(oracle.get("reference_surface_stage")),
                  "reader":rscore, "pdf":pscore, "reader_vs_pdf_direct":dscore,
                  "reader_reference_tiles_4x4":rtiles, "pdf_reference_tiles_4x4":ptiles,
                  "reader_vs_pdf_tiles_4x4":dtiles,
                  "occupancy":{"reader":occupancy(reader_grid),"pdf":occupancy(pdf_grid),"publisher_reference":occupancy(ref_rgb)},
                  "scene":{"node_count":item.get("nodes"),"story_count":item.get("stories"),"readable_shared_line_count":item.get("nonempty_shared_lines"),
                           "descriptor_only_resource_count":item.get("descriptor_only_resource_count"),
                           "fidelity_reasons":source_safe_codes(item.get("fidelity_reasons",[])),
                           "diagnostic_codes":source_safe_codes(item.get("diagnostic_codes",[])),
                           "stacking_fidelity":item.get("stacking_fidelity"),
                           "text_layout_fallback_counts":source_safe_counts(item.get("text_layout_fallback_counts",{}))},
                  "pdf_resource_codes":source_safe_counts(dict(counts)),
                  "pdf_node_disposition_counts":source_safe_counts(dict(dispositions)),
                  "pdf_diagnostic_codes":source_safe_codes([str(x.get("code", "")) for x in pdf_data.get("diagnostics", [])])}
        output.append(result)
    if output[1]["fixture"] != "022_7079ad60fd810979":
        raise ValueError("022 experimental witness not at pinned identity")
    return {"schema":SCHEMA,"repository_commit_sha":commit,"source_count":len(output),"matched_page_count":len(output),
            "comparison":{"grid":[64,64],"resampling":"Pillow.BOX","threshold":12,"pdf_raster_dpi":DPI},
            "interpretation_fences":{"same_product_head":True,"same_source_pub_sha_per_backend":True,"reference_surface_stage_is_not_inferred":True,
              "raster_is_screening_not_semantic_authority":True,"no_source_or_rendered_bytes_emitted":True,
              "scene_code_counts_are_not_auto_causal_attribution":True},
            "rows":output}


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    pre = sub.add_parser("prepare")
    pre.add_argument("--source-root",type=Path,required=True)
    pre.add_argument("--reference",type=Path,required=True)
    pre.add_argument("--manifest",type=Path,required=True)
    rep = sub.add_parser("collect")
    rep.add_argument("--browser-dir",type=Path,required=True)
    rep.add_argument("--pdf-dir",type=Path,required=True)
    rep.add_argument("--reference",type=Path,required=True)
    rep.add_argument("--commit",type=str,required=True)
    rep.add_argument("--out",type=Path,required=True)
    args=ap.parse_args()
    if args.cmd=="prepare":
        result=prepare(args.source_root,args.reference,args.manifest)
        print(json.dumps({"manifest_fixture_count":len(result["fixtures"]),"source_identity_verified":True},sort_keys=True))
    else:
        result=collect(args.browser_dir,args.pdf_dir,args.reference,args.commit)
        args.out.parent.mkdir(parents=True,exist_ok=True)
        args.out.write_text(json.dumps(result,indent=2,sort_keys=True)+"\n",encoding="utf-8")
        print(json.dumps({"schema":result["schema"],"witness_count":result["source_count"],"reader_022":result["rows"][1]["reader"]["changed_cell_fraction"],"pdf_022":result["rows"][1]["pdf"]["changed_cell_fraction"],"direct_022":result["rows"][1]["reader_vs_pdf_direct"]["changed_cell_fraction"]},sort_keys=True))


if __name__ == "__main__":
    main()
