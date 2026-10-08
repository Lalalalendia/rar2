#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
from collections import Counter
from pathlib import Path

GRID_W = 64
GRID_H = 64


def transformed_bbox(box: dict, transform: dict | None) -> tuple[float, float, float, float]:
    x0 = float(box["x"])
    y0 = float(box["y"])
    x1 = x0 + float(box["width"])
    y1 = y0 + float(box["height"])
    if not transform:
        return (x0, y0, x1, y1)
    a = float(transform.get("a", 1))
    b = float(transform.get("b", 0))
    c = float(transform.get("c", 0))
    d = float(transform.get("d", 1))
    tx = float(transform.get("tx", 0))
    ty = float(transform.get("ty", 0))
    pts = [
        (a*x0+c*y0+tx, b*x0+d*y0+ty),
        (a*x1+c*y0+tx, b*x1+d*y0+ty),
        (a*x0+c*y1+tx, b*x0+d*y1+ty),
        (a*x1+c*y1+tx, b*x1+d*y1+ty),
    ]
    xs=[p[0] for p in pts]
    ys=[p[1] for p in pts]
    return (min(xs),min(ys),max(xs),max(ys))


def gap_cells(box: tuple[float,float,float,float], band: tuple[float,float,float,float], cw: float, ch: float) -> float:
    gx=max(band[0]-box[2], box[0]-band[2], 0.0)/cw
    gy=max(band[1]-box[3], box[1]-band[3], 0.0)/ch
    return max(gx,gy)


def grid_bbox(box: tuple[float,float,float,float], width: float, height: float) -> list[float]:
    return [
        round(box[0] / width * GRID_W, 3),
        round(box[1] / height * GRID_H, 3),
        round(box[2] / width * GRID_W, 3),
        round(box[3] / height * GRID_H, 3),
    ]


def main() -> None:
    ap=argparse.ArgumentParser()
    ap.add_argument("result_json", type=Path)
    ap.add_argument("--fixture", required=True)
    ap.add_argument("--page", type=int, default=1)
    ap.add_argument("--min-col", type=int, required=True)
    ap.add_argument("--max-col", type=int, required=True)
    ap.add_argument("--min-row", type=int, required=True)
    ap.add_argument("--max-row", type=int, required=True)
    ap.add_argument("--max-gap", type=float, default=2.0)
    args=ap.parse_args()

    receipt=json.loads(args.result_json.read_text())
    scene=receipt["scene"]
    pages=sorted(scene["pages"], key=lambda x:x["order"])
    page=pages[args.page-1]
    width=float(page["width_emu"])
    height=float(page["height_emu"])
    cw=width/GRID_W
    ch=height/GRID_H
    band=(args.min_col*cw,args.min_row*ch,(args.max_col+1)*cw,(args.max_row+1)*ch)

    rows=[]
    sig=Counter()
    for node in scene.get("nodes",[]):
        if node.get("page_id") != page.get("page_id"):
            continue
        bounds=node.get("bounds")
        if not bounds:
            continue
        t=node.get("transform")
        semantic=transformed_bbox(bounds,t)
        gap=gap_cells(semantic,band,cw,ch)
        text_bounds=node.get("text_bounds")
        text_box=transformed_bbox(text_bounds,t) if text_bounds else None
        text_gap=gap_cells(text_box,band,cw,ch) if text_box else None
        nearest=min([x for x in [gap,text_gap] if x is not None])
        if nearest > args.max_gap:
            continue

        paint=node.get("paint") or {}
        line=paint.get("line") or {}
        border=node.get("decorative_border") or {}
        placements=border.get("placements") or []
        transform_identity=(not t) or (
            float(t.get("a",1))==1 and float(t.get("b",0))==0 and float(t.get("c",0))==0
            and float(t.get("d",1))==1 and float(t.get("tx",0))==0 and float(t.get("ty",0))==0
        )
        row={
            "kind":node.get("kind"),
            "semantic_grid_bbox":grid_bbox(semantic,width,height),
            "text_grid_bbox":grid_bbox(text_box,width,height) if text_box else None,
            "semantic_gap_cells":round(gap,3),
            "text_gap_cells":round(text_gap,3) if text_gap is not None else None,
            "text_present":bool(node.get("text")),
            "text_layout":(node.get("text_layout") or {}).get("disposition"),
            "resource_present":bool(node.get("resource_id")),
            "preset_shape":paint.get("preset_shape"),
            "fill_present":isinstance(paint.get("fill_rgb"),list),
            "line_present":isinstance(line.get("rgb"),list) and float(line.get("width_emu") or 0)>0,
            "line_width_emu":int(line.get("width_emu") or 0),
            "decorative_border_placements":len(placements),
            "transform_identity":transform_identity,
        }
        rows.append(row)
        sig[json.dumps({
            "kind":row["kind"],
            "text_present":row["text_present"],
            "text_layout":row["text_layout"],
            "resource_present":row["resource_present"],
            "preset_shape":row["preset_shape"],
            "fill_present":row["fill_present"],
            "line_present":row["line_present"],
            "decorative_border_placements":row["decorative_border_placements"],
            "transform_identity":row["transform_identity"],
        },sort_keys=True)] += 1

    rows.sort(key=lambda r:(min(r["semantic_gap_cells"], r["text_gap_cells"] if r["text_gap_cells"] is not None else 999), r["kind"] or ""))
    payload={
        "schema":"chaptera.scene-band-paint-census.v1",
        "fixture":args.fixture,
        "page":args.page,
        "band_grid_bbox":[args.min_col,args.min_row,args.max_col,args.max_row],
        "max_gap_cells":args.max_gap,
        "nearby_node_count":len(rows),
        "signature_counts":[{"signature":json.loads(k),"count":v} for k,v in sorted(sig.items(), key=lambda kv:(-kv[1],kv[0]))],
        "nearby_nodes":rows,
        "claims":{
            "raw_story_text_emitted":False,
            "raw_node_ids_emitted":False,
            "raw_source_coordinates_emitted":False,
            "source_neutral_scene_only":True,
        },
    }
    print("SCENE_BAND_PAINT_CENSUS "+json.dumps(payload,sort_keys=True))


if __name__=="__main__":
    main()
