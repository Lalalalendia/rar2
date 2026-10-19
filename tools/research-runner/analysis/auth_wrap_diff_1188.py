#!/usr/bin/env python3
"""Fail-closed, source-free AUTH-WRAP 0x47/FOPT comparison; no Publisher law inferred.

Inputs are private auth_wrap_oracle_tool 'inspect' records. Public output holds
only comparison booleans/enumerations, never original PUB bytes or document text.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


class UnsafeObservation(ValueError):
    pass


def unique(rows: list[dict[str, Any]], field: str, value: int) -> dict[str, Any]:
    found = [r for r in rows if r.get(field) == value]
    if len(found) != 1:
        raise UnsafeObservation(f"expected exactly one {field}={value}, found {len(found)}")
    return found[0]


def refs(frame: dict[str, Any]) -> tuple[int, ...]:
    items = frame.get("refs")
    if not isinstance(items, list):
        raise UnsafeObservation("missing 0x47 refs")
    result = tuple(int(x["target_seq_num"]) for x in items)
    if len(set(result)) != len(result):
        raise UnsafeObservation("duplicate 0x47 target references")
    if frame.get("count") != len(result):
        raise UnsafeObservation("0x46 count does not equal 0x47 count")
    return result


def check_snapshot(snapshot: dict[str, Any]) -> None:
    if snapshot.get("schema") != "chaptera.auth-wrap-oracle-tool.v1":
        raise UnsafeObservation("inspector schema mismatch")
    if not isinstance(snapshot.get("shapes"), list) or not isinstance(snapshot.get("wrap_frames"), list):
        raise UnsafeObservation("inspector snapshot missing shape or frame catalog")
    shape_keys = [(str(x.get("page_id")), x.get("seq_num")) for x in snapshot["shapes"]]
    if len(shape_keys) != len(set(shape_keys)):
        raise UnsafeObservation("ambiguous page/sequence shape identity")
    frame_keys = [x.get("frame_seq_num") for x in snapshot["wrap_frames"]]
    if len(frame_keys) != len(set(frame_keys)):
        raise UnsafeObservation("ambiguous frame identity")
    for f in snapshot["wrap_frames"]:
        refs(f)


def compare(before: dict[str, Any], prepared: dict[str, Any], saved: dict[str, Any],
            *, family: str, frame_seq: int) -> dict[str, Any]:
    if family not in ("A", "B"):
        raise UnsafeObservation("family must be A or B")
    for snapshot in (before, prepared, saved):
        check_snapshot(snapshot)
    b = unique(before["wrap_frames"], "frame_seq_num", frame_seq)
    p = unique(prepared["wrap_frames"], "frame_seq_num", frame_seq)
    a = unique(saved["wrap_frames"], "frame_seq_num", frame_seq)
    original, mutated, output = refs(b), refs(p), refs(a)
    if b["page_id"] != p["page_id"] or p["page_id"] != a["page_id"]:
        raise UnsafeObservation("frame moved pages across copies")
    if b["bounds_emu"] != p["bounds_emu"] or p["bounds_emu"] != a["bounds_emu"]:
        raise UnsafeObservation("frame bounds changed: geometry not isolated")
    before_keys = {(str(x["page_id"]), x["seq_num"]): x for x in before["shapes"]}
    prepared_keys = {(str(x["page_id"]), x["seq_num"]): x for x in prepared["shapes"]}
    after_keys = {(str(x["page_id"]), x["seq_num"]): x for x in saved["shapes"]}
    if before_keys.keys() != prepared_keys.keys() or prepared_keys.keys() != after_keys.keys():
        raise UnsafeObservation("shape inventory changed across experiment")
    for key in before_keys:
        for x in (prepared_keys[key], after_keys[key]):
            if x["bounds_emu"] != before_keys[key]["bounds_emu"]:
                raise UnsafeObservation("shape geometry changed across experiment")
            if x["officeart_spid"] != before_keys[key]["officeart_spid"]:
                raise UnsafeObservation("OfficeArt shape identity changed")
            if x.get("officeart_shape_type") != before_keys[key].get("officeart_shape_type"):
                raise UnsafeObservation("OfficeArt shape type changed")
            if x.get("is_text_frame") != before_keys[key].get("is_text_frame"):
                raise UnsafeObservation("shape/TextFrame membership changed")
    if {x["frame_seq_num"] for x in before["wrap_frames"]} != {
        x["frame_seq_num"] for x in prepared["wrap_frames"]
    }:
        raise UnsafeObservation("non-target frame inventory changed before Publisher")
    if {x["frame_seq_num"] for x in before["wrap_frames"]} != {
        x["frame_seq_num"] for x in saved["wrap_frames"]
    }:
        raise UnsafeObservation("wrap frame inventory changed after Publisher SaveAs")
    p_by_id = {x["frame_seq_num"]: x for x in prepared["wrap_frames"]}
    for frame in before["wrap_frames"]:
        if frame["frame_seq_num"] == frame_seq:
            continue
        pf = p_by_id.get(frame["frame_seq_num"])
        if pf is None or refs(frame) != refs(pf):
            raise UnsafeObservation("non-target 0x47 changed before Publisher")
    for key in before_keys:
        if before_keys[key]["fopt"] != prepared_keys[key]["fopt"]:
            raise UnsafeObservation("FOPT changed before Publisher COM mutation")

    if family == "A":
        if len(mutated) != len(original):
            raise UnsafeObservation("family A changed 0x47 cardinality")
        indices = [i for i, (x, y) in enumerate(zip(original, mutated)) if x != y]
        if len(indices) != 1:
            raise UnsafeObservation("family A must replace precisely one 0x47 ref")
        if mutated[indices[0]] in original:
            raise UnsafeObservation("family A replacement was already a referenced obstacle")
        if saved["source_sha256"] == prepared["source_sha256"]:
            raise UnsafeObservation("Publisher did not produce a fresh saved candidate")
        post_ref = ("patched_ref_preserved" if output == mutated else
                    "baseline_ref_restored" if output == original else "other_regeneration")
        post_fopt = "unchanged" if all(prepared_keys[k]["fopt"] == after_keys[k]["fopt"] for k in before_keys) else "changed"
        return {
            "schema": "chaptera.auth-wrap-diff-1188.v1", "family": family,
            "frame_seq_num": frame_seq, "one_0x47_target_ref_changed": True,
            "0x46_count_kept_consistent": True, "geometry_unchanged_before_open": True,
            "textwrap_fopt_unchanged_before_open": True,
            "membership_changed_before_open": set(original) != set(mutated),
            "order_changed_before_open": original != mutated,
            "post_save_0x47_outcome": post_ref,
            "post_save_fopt_outcome": post_fopt,
            "layout_outcome": "not_observed",
        }
    if original != mutated or before["source_sha256"] != prepared["source_sha256"]:
        raise UnsafeObservation("family B must leave on-disk 0x47/source untouched pre-COM")
    return {
        "schema": "chaptera.auth-wrap-diff-1188.v1", "family": family,
        "frame_seq_num": frame_seq, "geometry_unchanged_before_save": True,
        "old_0x47_left_untouched_before_save": True,
        "membership_changed_after_save": set(original) != set(output),
        "order_changed_after_save": original != output,
        "post_save_0x47_outcome": "unchanged" if original == output else "other_regeneration",
        "post_save_fopt_outcome": "not_observed",  # Readback of COM setter is separate.
        "layout_outcome": "not_observed",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--family", choices=("A", "B"), required=True)
    parser.add_argument("--frame-seq", type=int, required=True)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--prepared", type=Path, required=True)
    parser.add_argument("--saved", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = compare(*(json.loads(p.read_text(encoding="utf-8-sig")) for p in
                           (args.before, args.prepared, args.saved)), family=args.family,
                         frame_seq=args.frame_seq)
    except (UnsafeObservation, KeyError, TypeError, ValueError) as exc:
        parser.exit(1, f"AUTH-WRAP-1188 unsafe/ambiguous: {exc}\n")
    args.output.write_text(json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
