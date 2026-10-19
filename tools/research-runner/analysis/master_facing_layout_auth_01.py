#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import defaultdict
import hashlib
import json
import sys
import zlib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from operation_blast_radius_v1 import BlastRadiusError, CFB, build_receipt, stream_bytes  # noqa: E402

SCHEMA = "chaptera.master-facing-layout-auth-01.analysis.v1"
EXPERIMENT_ID = "MASTER-FACING-LAYOUT-AUTH-01"


class AnalysisError(RuntimeError):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict):
        raise AnalysisError(f"{path}: expected object")
    return value


def resolve_artifact(root: Path, summary: dict[str, Any]) -> Path:
    rel = summary.get("relative_path")
    expected = summary.get("sha256")
    if not isinstance(rel, str) or not rel:
        raise AnalysisError("artifact summary missing relative_path")
    if not isinstance(expected, str) or len(expected) != 64:
        raise AnalysisError(f"{rel}: artifact summary missing sha256")
    path = root / Path(rel)
    if not path.is_file():
        raise AnalysisError(f"artifact missing: {path}")
    actual = sha256_bytes(path.read_bytes())
    if actual != expected:
        raise AnalysisError(f"{rel}: SHA mismatch expected={expected} actual={actual}")
    return path


def root_contents(data: bytes) -> bytes:
    cfb = CFB(data)
    matches = [e for e in cfb.dirs if e["type"] == 2 and e["name"] == "Contents"]
    if len(matches) != 1:
        raise AnalysisError(f"expected one root Contents stream, found {len(matches)}")
    return stream_bytes(cfb, matches[0])


def logical_ranges(left: bytes, right: bytes) -> list[dict[str, Any]]:
    limit = min(len(left), len(right))
    offsets = [i for i in range(limit) if left[i] != right[i]]
    if len(left) != len(right):
        offsets.extend(range(limit, max(len(left), len(right))))
    if not offsets:
        return []
    out: list[dict[str, Any]] = []
    start = prev = offsets[0]
    for off in offsets[1:]:
        if off == prev + 1:
            prev = off
            continue
        out.append(range_row(left, right, start, prev + 1))
        start = prev = off
    out.append(range_row(left, right, start, prev + 1))
    return out


def range_row(left: bytes, right: bytes, start: int, end: int) -> dict[str, Any]:
    return {
        "offset": start,
        "length": end - start,
        "before_hex": left[start:min(end, len(left))].hex(),
        "after_hex": right[start:min(end, len(right))].hex(),
    }


def png_visual_fingerprint(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise AnalysisError(f"{path}: expected PNG")
    pos = 8
    ihdr = None
    idat: list[bytes] = []
    while pos + 12 <= len(data):
        length = int.from_bytes(data[pos:pos + 4], "big")
        kind = data[pos + 4:pos + 8]
        p0 = pos + 8
        p1 = p0 + length
        if p1 + 4 > len(data):
            raise AnalysisError(f"{path}: truncated PNG")
        payload = data[p0:p1]
        if kind == b"IHDR":
            ihdr = payload
        elif kind == b"IDAT":
            idat.append(payload)
        elif kind == b"IEND":
            break
        pos = p1 + 4
    if ihdr is None or not idat:
        raise AnalysisError(f"{path}: missing IHDR/IDAT")
    raw = zlib.decompress(b"".join(idat))
    return {
        "file_sha256": sha256_bytes(data),
        "visual_payload_sha256": sha256_bytes(ihdr + raw),
        "ihdr_hex": ihdr.hex(),
        "decompressed_scanline_bytes": len(raw),
    }


def value_field(obj: dict[str, Any], key: str) -> Any:
    item = obj.get(key)
    if not isinstance(item, dict) or item.get("state") != "value":
        return None
    return item.get("value")


def page_by_id(snapshot: dict[str, Any]) -> dict[int, dict[str, Any]]:
    return {int(row["page_id"]): row for row in snapshot.get("pages", [])}


def master_by_id(snapshot: dict[str, Any]) -> dict[int, dict[str, Any]]:
    return {int(row["page_id"]): row for row in snapshot.get("masters", [])}


def tagged_role_owners(snapshot: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    out: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for master in snapshot.get("masters", []):
        for shape in master.get("tagged_shapes", []):
            out[str(shape["role"])].append(
                {
                    "master_page_id": int(master["page_id"]),
                    "master_index": int(master["index"]),
                    "shape_id": int(shape["shape_id"]),
                    "left": shape["left"],
                    "top": shape["top"],
                    "width": shape["width"],
                    "height": shape["height"],
                    "rotation": shape["rotation"],
                    "z_order_position": shape["z_order_position"],
                }
            )
    return dict(out)


def render_map(root: Path, arm: dict[str, Any]) -> dict[int, dict[str, Any]]:
    renders = arm["fresh_reopen"]["renders"]
    out: dict[int, dict[str, Any]] = {}
    for page_id, summary in renders.items():
        path = resolve_artifact(root, summary)
        out[int(page_id)] = png_visual_fingerprint(path)
    return out


def summarize_blast(receipt: dict[str, Any]) -> dict[str, Any]:
    streams = receipt["cfb"]["control_mutation_stream_delta"]
    return {
        "changed_stream_count": len(streams),
        "changed_streams": [
            {
                "stream_id": row["stream_id"],
                "before_size": row["before_size"],
                "after_size": row["after_size"],
                "classification": row["classification"],
            }
            for row in streams
        ],
        "topology_delta_count": len(receipt["cfb"]["control_mutation_topology_delta"]),
        "physical_byte_range_count": len(receipt["cfb"]["control_mutation_byte_ranges"]),
        "classification_counts": receipt["classification_counts"],
    }


def analyze(output_root: Path) -> dict[str, Any]:
    native_path = output_root / "analysis" / "master-facing-layout-auth-01.json"
    native = load_json(native_path)
    if native.get("experiment_id") != EXPERIMENT_ID:
        raise AnalysisError(f"unexpected experiment id: {native.get('experiment_id')!r}")

    baseline = native["baseline"]
    control = native["arms"]["control"]
    converted = native["arms"]["convert_two_page"]
    parity = native["arms"]["parity_insert"]
    back = native["arms"]["convert_back"]

    baseline_path = resolve_artifact(output_root, baseline["output"])
    control_path = resolve_artifact(output_root, control["output"])
    converted_path = resolve_artifact(output_root, converted["output"])
    parity_path = resolve_artifact(output_root, parity["output"])
    back_path = resolve_artifact(output_root, back["output"])

    baseline_bytes = baseline_path.read_bytes()
    control_bytes = control_path.read_bytes()
    converted_bytes = converted_path.read_bytes()

    receipt = build_receipt(
        baseline_bytes,
        control_bytes,
        converted_bytes,
        evidence={
            "operation": {
                "kind": "publisher-master-facing-layout",
                "experiment_id": EXPERIMENT_ID,
                "arm": "is-two-page-master-true",
            },
            "producer": {
                "publisher_environment": "publisher-2019",
                "experiment_id": EXPERIMENT_ID,
            },
            "requested_streams": [
                f"dir:{e['i']}:{e['name']}"
                for e in CFB(control_bytes).dirs
                if e["type"] == 2 and e["name"] == "Contents"
            ],
            "arms": {"source": {}, "control": {}, "mutation": {}},
        },
    )

    detail_dir = output_root / "private" / "master-facing-layout-auth-01" / "blast-radius"
    detail_dir.mkdir(parents=True, exist_ok=True)
    (detail_dir / "convert-two-page.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    baseline_snapshot = baseline["fresh_reopen"]["semantic"]
    converted_snapshot = converted["fresh_reopen"]["semantic"]
    parity_snapshot = parity["fresh_reopen"]["semantic"]
    back_snapshot = back["fresh_reopen"]["semantic"]

    dedicated_master = int(baseline["dedicated_master_page_id"])
    tracked = [int(x) for x in baseline["tracked_customer_page_ids"]]

    bmasters = master_by_id(baseline_snapshot)
    cmasters = master_by_id(converted_snapshot)
    pmasters = master_by_id(parity_snapshot)
    backmasters = master_by_id(back_snapshot)

    old_ids = set(bmasters)
    converted_ids = set(cmasters)
    new_ids = sorted(converted_ids - old_ids)
    removed_ids = sorted(old_ids - converted_ids)

    two_page_ids = sorted(
        mid
        for mid, row in cmasters.items()
        if value_field(row, "is_two_page_master") is True
    )

    if (
        len(new_ids) == 1
        and dedicated_master in cmasters
        and len(converted_ids) == len(old_ids) + 1
    ):
        topology_class = "two-page-master-materializes-one-partner-master-page"
    elif len(new_ids) == 0 and dedicated_master in cmasters:
        topology_class = "single-master-identity-carries-two-page-state"
    else:
        topology_class = "other-master-topology"

    baseline_roles = tagged_role_owners(baseline_snapshot)
    converted_roles = tagged_role_owners(converted_snapshot)
    role_classification: dict[str, Any] = {}
    for role, before_rows in baseline_roles.items():
        after_rows = converted_roles.get(role, [])
        before_ids = {(r["master_page_id"], r["shape_id"]) for r in before_rows}
        after_ids = {(r["master_page_id"], r["shape_id"]) for r in after_rows}
        if len(after_rows) > len(before_rows):
            kind = "persisted-role-multiplied-across-master-pages"
        elif after_ids == before_ids:
            kind = "source-role-identity-preserved-only"
        elif after_rows:
            kind = "source-role-rematerialized-or-moved"
        else:
            kind = "source-role-missing-after-conversion"
        role_classification[role] = {
            "classification": kind,
            "before": before_rows,
            "after": after_rows,
        }

    bpages = page_by_id(baseline_snapshot)
    cpages = page_by_id(converted_snapshot)
    ppages = page_by_id(parity_snapshot)
    backpages = page_by_id(back_snapshot)

    converted_render = render_map(output_root, converted)
    parity_render = render_map(output_root, parity)
    baseline_render = render_map(output_root, baseline)
    back_render = render_map(output_root, back)

    page_transitions: list[dict[str, Any]] = []
    any_binding_flip = False
    any_render_flip = False
    for page_id in tracked:
        if page_id not in cpages or page_id not in ppages:
            raise AnalysisError(f"tracked PageID {page_id} missing after conversion/parity")
        converted_master = int(cpages[page_id]["master_page_id"])
        parity_master = int(ppages[page_id]["master_page_id"])
        binding_flip = converted_master != parity_master
        render_flip = (
            converted_render[page_id]["visual_payload_sha256"]
            != parity_render[page_id]["visual_payload_sha256"]
        )
        any_binding_flip = any_binding_flip or binding_flip
        any_render_flip = any_render_flip or render_flip
        page_transitions.append(
            {
                "page_id": page_id,
                "baseline_index": int(bpages[page_id]["index"]),
                "converted_index": int(cpages[page_id]["index"]),
                "parity_index": int(ppages[page_id]["index"]),
                "converted_master_page_id": converted_master,
                "parity_master_page_id": parity_master,
                "master_binding_flipped": binding_flip,
                "render_flipped": render_flip,
                "converted_render_visual_sha256": converted_render[page_id][
                    "visual_payload_sha256"
                ],
                "parity_render_visual_sha256": parity_render[page_id][
                    "visual_payload_sha256"
                ],
            }
        )

    if any_binding_flip:
        parity_class = "handedness-or-master-half-binding-depends-on-ordinal-position"
    elif any_render_flip:
        parity_class = "projection-handedness-depends-on-ordinal-with-stable-master-binding"
    else:
        parity_class = "tracked-pages-retain-binding-and-render-across-leading-page-insert"

    baseline_mirror = value_field(
        baseline_snapshot["document_layout_guides"], "mirror_guides"
    )
    converted_mirror = value_field(
        converted_snapshot["document_layout_guides"], "mirror_guides"
    )
    mirror_coupling = {
        "baseline": baseline_mirror,
        "converted": converted_mirror,
        "changed": baseline_mirror != converted_mirror,
        "classification": (
            "is-two-page-master-conversion-also-enabled-document-mirror-guides"
            if baseline_mirror is False and converted_mirror is True
            else "no-global-mirror-guides-enable-observed"
        ),
    }

    back_supported = bool(back["mutation"]["supported"])
    back_summary = {
        "supported": back_supported,
        "requested_readback": back["mutation"].get("readback"),
        "error": back["mutation"].get("error"),
        "master_ids": sorted(backmasters),
        "dedicated_master_survives": dedicated_master in backmasters,
        "tracked_pages": [
            {
                "page_id": pid,
                "master_page_id": int(backpages[pid]["master_page_id"]),
                "render_visual_sha256": back_render[pid]["visual_payload_sha256"],
                "matches_baseline_render": (
                    back_render[pid]["visual_payload_sha256"]
                    == baseline_render[pid]["visual_payload_sha256"]
                ),
            }
            for pid in tracked
            if pid in backpages and pid in back_render
        ],
    }

    control_contents = root_contents(control_bytes)
    converted_contents = root_contents(converted_bytes)

    result = {
        "schema": SCHEMA,
        "experiment_id": EXPERIMENT_ID,
        "native_receipt_sha256": sha256_bytes(native_path.read_bytes()),
        "fixture_sha256": native["fixture"]["expected_sha256"],
        "dedicated_master_page_id": dedicated_master,
        "tracked_customer_page_ids": tracked,
        "topology": {
            "classification": topology_class,
            "baseline_master_ids": sorted(old_ids),
            "converted_master_ids": sorted(converted_ids),
            "new_master_ids": new_ids,
            "removed_master_ids": removed_ids,
            "two_page_master_ids": two_page_ids,
            "original_master_survives": dedicated_master in cmasters,
            "baseline_master_count": int(baseline_snapshot["master_page_count"]),
            "converted_master_count": int(converted_snapshot["master_page_count"]),
        },
        "master_role_identity": role_classification,
        "mirror_guides_coupling": mirror_coupling,
        "parity_discriminator": {
            "classification": parity_class,
            "inserted_page_id": int(parity["mutation"]["inserted_page_id"]),
            "page_transitions": page_transitions,
        },
        "conversion_back": back_summary,
        "converted_master_snapshots": converted_snapshot["masters"],
        "converted_customer_snapshots": [
            cpages[pid] for pid in tracked if pid in cpages
        ],
        "blast_radius": summarize_blast(receipt),
        "contents_logical_changed_ranges": logical_ranges(
            control_contents, converted_contents
        ),
        "raw_0x4c_status": {
            "status": "not-yet-decoded-in-current-repo",
            "reason": (
                "The historical OBS-MARGINS-ORACLE-01 0x4C analyzer is not present "
                "on current main. This receipt preserves the exact control/mutation "
                "PUBs privately plus the matched-control root Contents delta; do not "
                "invent 0x4C object boundaries from byte-value scans."
            ),
        },
        "semantic_authority_ready": (
            dedicated_master in cmasters
            and bool(two_page_ids)
            and len(tracked) == 4
            and all(pid in cpages and pid in ppages for pid in tracked)
        ),
        "scope_boundary": native["boundary"],
    }

    out = output_root / "analysis" / "master-facing-layout-auth-01-blast-radius.json"
    out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Analyze T825 two-page-master topology, parity and matched-control persistence"
    )
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    try:
        result = analyze(args.output_root)
    except (
        OSError,
        ValueError,
        KeyError,
        json.JSONDecodeError,
        zlib.error,
        BlastRadiusError,
        AnalysisError,
    ) as exc:
        print(f"master-facing-layout-analysis: {exc}", file=sys.stderr)
        return 2

    print(
        json.dumps(
            {
                "schema": result["schema"],
                "topology": result["topology"]["classification"],
                "parity": result["parity_discriminator"]["classification"],
                "mirror_guides": result["mirror_guides_coupling"]["classification"],
                "conversion_back_supported": result["conversion_back"]["supported"],
                "semantic_authority_ready": result["semantic_authority_ready"],
                "raw_0x4c_status": result["raw_0x4c_status"]["status"],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
