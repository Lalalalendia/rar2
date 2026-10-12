#!/usr/bin/env python3
"""Fail-closed Product-only 49/111 scoreboard; no direct-worker fallback."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
from collections import defaultdict
from reader_publisher_lo_49_111_scoreboard_v1 import (
    load_frozen, read_publisher_registry, strict_load_json,
    win_summary, document_summary, winner, HEX_SHA,
)
ROUTE = "oidc-https-source-ingress-isolated-worker-host-preview-chromium"
SCHEMA = "chaptera.product-publisher-49-111-source-neutral.v1"
PROOFS = ("oidc_pkce", "https_product_http", "source_ingress",
          "tenant_current_revision", "isolated_worker",
          "independent_host_image_admission", "real_chromium",
          "privacy_scan_passed")
METHOD = {"channel_significance": 12, "grid": [64, 64],
          "colorspace": "RGB", "resampling": "Pillow.BOX",
          "raster_dpi": 144, "physical_page_tolerance_pt": 0.05}
def score_product(physical, frozen, refs, pairs, head):
    if not isinstance(head,str) or not HEX_SHA.fullmatch(head):
        raise ValueError("missing exact checkout SHA")
    if (physical.get("schema") != SCHEMA
        or physical.get("repository_commit_sha") != head
        or physical.get("route") != ROUTE
        or physical.get("method") != METHOD
        or physical.get("reference_surface_stage") != "unknown"
        or physical.get("source_free_artifact") is not True
        or physical.get("publisher_pixel_exact_parity") is not False):
        raise ValueError("not a valid exact-head Product receipt")
    fixtures = physical.get("fixtures")
    if not isinstance(fixtures,list) or len(fixtures) != 49:
        raise ValueError("49 fixed PUB source records required")
    keyed = {}
    for row in fixtures:
        if not isinstance(row,dict) or row.get("fixture") in keyed:
            raise ValueError("invalid or duplicate Product fixture")
        keyed[row["fixture"]] = row
    if set(keyed) != {r[0] for r in frozen["rows"]}:
        raise ValueError("frozen source membership changed")
    scored, excluded = [], []
    for fixture, pub_sha, pdf_sha, pdf_pages, _, _, lo_pages in frozen["rows"]:
        p, ref, pair = keyed[fixture], refs[fixture], pairs[fixture]
        if (p.get("source_sha256") != pub_sha
            or p.get("publisher_pdf_sha256") != pdf_sha
            or p.get("repository_commit_sha") != head
            or p.get("route") != ROUTE
            or pair["pub_sha256"] != pub_sha or pair["pdf_sha256"] != pdf_sha
            or ref["pub_sha256"] != pub_sha or ref["pdf_sha256"] != pdf_sha):
            raise ValueError("Product / Publisher source identity drift: " + fixture)
        if (p.get("status") not in ("rendered","rendered_partial")
            or p.get("scene_fidelity") not in ("supported","partial")
            or not isinstance(p.get("physical_proofs"),dict)
            or any(p["physical_proofs"].get(k) is not True for k in PROOFS)
            or p.get("external_fetch_count") != 0
            or p.get("page_error_count") != 0):
            raise ValueError("Product physical / security gate failed: " + fixture)
        rows = p.get("pages")
        if not isinstance(rows,list) or type(p.get("candidate_page_count")) is not int:
            raise ValueError("Product page evidence incomplete: " + fixture)
        if not lo_pages:
            if rows: raise ValueError("predeclared excluded source scored: " + fixture)
            excluded.append(fixture)
            continue
        if p["candidate_page_count"] != pdf_pages or len(rows) != len(lo_pages):
            raise ValueError("eligible Product page count changed: " + fixture)
        current = {}
        page_ids = set()
        for row in rows:
            if not isinstance(row,dict): raise ValueError("invalid page record")
            i = row.get("page")
            if type(i) is not int or i<1 or i>pdf_pages or i in current:
                raise ValueError("Product page identity duplicate / invalid")
            page_id = row.get("source_page_id_sha256")
            png_sha = row.get("chromium_screenshot_sha256")
            if any(not isinstance(v,str) or len(v)!=64
                   or any(c not in "0123456789abcdef" for c in v)
                   for v in (page_id,png_sha)) or page_id in page_ids:
                raise ValueError("canonical Product PAGE / screenshot hash missing")
            page_ids.add(page_id)
            bounds = row.get("reference_media_extent_delta_pt")
            if (not isinstance(bounds,dict) or set(bounds)!={"width","height"}
                or any(type(v) not in (float,int) or abs(v)>.05
                       for v in bounds.values())
                or row.get("reference_surface_stage")!="unknown"
                or row.get("reference_grid_sha256")!=ref["pages"][i-1]["grid_sha256"]
                or type(row.get("changed_cell_count")) is not int
                or not 0<=row["changed_cell_count"]<=4096):
                raise ValueError("Product page reference / visual method drift")
            current[i] = row["changed_cell_count"]
        if set(current)!={r[0] for r in lo_pages}:
            raise ValueError("Product page ordinal membership changed")
        for ordinal, lo_cells, _ in lo_pages:
            cells=current[ordinal]
            scored.append({"fixture":fixture, "page":ordinal,
                           "reader_cells":cells, "libreoffice_cells":lo_cells,
                           "winner":winner((cells-lo_cells)/4096)})
    if len(scored)!=111 or len(excluded)!=10:
        raise ValueError("fixed Product 39/111 denominator changed")
    pages, docs = win_summary(scored),document_summary(scored)
    if pages["pages"]!=111 or docs["documents"]!=39:
        raise ValueError("fixed Product document denominator changed")
    debts=defaultdict(int)
    for row in scored:
        debts[row["fixture"]]+=max(0,row["reader_cells"]-row["libreoffice_cells"])
    return {"schema":"chaptera.product-publisher-lo-49-111-scoreboard.v1",
            "repository_commit_sha":head,"measurement_route":ROUTE,
            "ranked_pages":111,"ranked_documents":39,
            "predeclared_ineligible":sorted(excluded),
            "page_scores":pages,"document_scores":docs,
            "largest_lo_advantages":[{"fixture":k,"gap_cells_sum":v}
                 for k,v in sorted(debts.items(),key=lambda x:(-x[1],x[0]))[:10]],
            "native_publisher_pixel_exact_parity":False,
            "production_deployment_verified":False}
def main():
    a=argparse.ArgumentParser(description=__doc__)
    for arg in ("physical-product-receipt","frozen-lo","publisher-fingerprint",
                "publisher-pairs","expected-product-sha","out"):
        a.add_argument("--"+arg,required=True)
    v=vars(a.parse_args())
    frozen=load_frozen(Path(v["frozen_lo"]))
    refs,pairs=read_publisher_registry(Path(v["publisher_fingerprint"]),Path(v["publisher_pairs"]))
    physical=strict_load_json(Path(v["physical_product_receipt"]))
    result=score_product(physical,frozen,refs,pairs,v["expected_product_sha"])
    output=Path(v["out"]);output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(result,indent=2,sort_keys=True)+"\n")
    print("PRODUCT_49_111_PHYSICAL_ONLY",result["page_scores"],result["document_scores"])
if __name__=="__main__":
    main()
