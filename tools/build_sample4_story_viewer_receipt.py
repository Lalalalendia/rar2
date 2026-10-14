#!/usr/bin/env python3
"""Build an explicit Partial legacy Sample4 Story scene from current Reader output.

Never drop table relationships silently, invent a page, or authorize a
general PUB editor. This only supplies a safe original Viewer scene to a
single exact-SHA browser acceptance case.
"""
import argparse
import hashlib
import json
from pathlib import Path

SOURCE_SHA = "42195f7ad23d911219fea3ec88e66e867e9b9a6821a16dd1b535e2aa9d57a11b"
CANDIDATE_SHA = "2f7795a3c4307716565c7accf4636a80b7947f8921c60a87b51e1a48ee529509"

def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def main() -> None:
    parser = argparse.ArgumentParser()
    for name in ("source", "graph", "viewer_raw", "manifest", "output", "status"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    source = args.source.read_bytes()
    if len(source) != 72192 or digest(source) != SOURCE_SHA:
        raise RuntimeError("not the exact pinned Sample4 source")
    graph = json.loads(args.graph.read_text(encoding="utf-8"))
    viewer = json.loads(args.viewer_raw.read_text(encoding="utf-8"))
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    if (
        graph["document"]["source_hash"] != SOURCE_SHA
        or viewer["document"]["source"]["source_hash"] != SOURCE_SHA
        or manifest["schema"] != "chaptera.pub-native-story-handoff.v1"
        or manifest["source_sha256"] != SOURCE_SHA
        or manifest["candidate_sha256"] != CANDIDATE_SHA
        or manifest["story_syid"] != 1
        or manifest["before_utf16_len"] != 86
        or manifest["after_utf16_len"] != 85
    ):
        raise RuntimeError("Reader or native handoff source/Story identity mismatch")

    stories = viewer["document"].get("stories")
    graph_stories = graph.get("stories")
    if not isinstance(stories, list) or not isinstance(graph_stories, dict):
        raise RuntimeError("missing source-backed Story catalog")
    target = [
        item["id"]
        for item in stories
        if isinstance(item, dict) and isinstance(item.get("text"), str)
        and digest(item["text"].encode("utf-8")) == manifest["before_text_sha256"]
        and len(item["text"].encode("utf-16-le")) // 2 == 86
    ]
    if len(target) != 1:
        raise RuntimeError("Sample4 target Story is not uniquely source-backed")
    story_id = target[0]
    if graph_stories.get(story_id, {}).get("text") != next(
        item["text"] for item in stories if item["id"] == story_id
    ):
        raise RuntimeError("graph and legacy Viewer disagree on the target Story")

    tables = viewer.pop("tables", [])
    if not isinstance(tables, list):
        raise RuntimeError("Reader table carrier is not an array")
    all_stories = {item["id"] for item in stories}
    nodes = {item["origin"] for item in viewer["scene"]["nodes"]}
    rows = []
    for table in tables:
        if (
            not isinstance(table, dict)
            or not isinstance(table.get("story_id"), str)
            or table["story_id"] not in all_stories
            or not isinstance(table.get("node_id"), str)
            or table["node_id"] not in nodes
            or not isinstance(table.get("cells"), list)
            or not isinstance(table.get("rows"), int)
            or not isinstance(table.get("columns"), int)
        ):
            raise RuntimeError("unverified Reader table identity or topology")
        if table["story_id"] == story_id:
            raise RuntimeError("Sample4 target is table-owned; generic Story UI forbidden")
        rows.append({
            "node_id": table["node_id"],
            "story_id": table["story_id"],
            "rows": table["rows"],
            "columns": table["columns"],
            "cell_count": len(table["cells"]),
        })
    if rows:
        viewer.setdefault("document", {}).setdefault("diagnostics", []).append({
            "code": "viewer.table.unprojected_in_sample4_story_ui",
            "severity": "fidelity_warning",
            "message": "Source table cells are not rendered by this bounded Story editing view.",
        })
    args.output.write_text(
        json.dumps(viewer, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    # Only this small source-safe status is uploaded; Viewer and source stay private.
    args.status.write_text(
        json.dumps({
            "source_sha256": SOURCE_SHA,
            "candidate_sha256": CANDIDATE_SHA,
            "story_syid": 1,
            "target_story_id": story_id,
            "target_is_table_owned": False,
            "source_table_count": len(rows),
            "browser_fidelity": "partial" if rows else "original_scene",
            "native_publisher_authority": "exact_pair_only",
        }, indent=2) + "\n",
        encoding="utf-8",
    )
    print({
        "viewer_source_identity": "exact_pinned_Sample4",
        "target_table_owned": False,
        "extra_table_count": len(rows),
    })

if __name__ == "__main__":
    main()
