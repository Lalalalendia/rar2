#!/usr/bin/env python3
"""Source-free census of persisted OLE cached WMF presentations in PUB CFB files."""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path


STANDARD_MARKERS = {0xFFFFFFFF, 0xFFFFFFFE}
CF_METAFILEPICT = 3
OLEPRES_RE = re.compile(r"^\x02OlePres(\d{3})$", re.IGNORECASE)
PLACEABLE_KEY = 0x9AC6CDD7
PLACEABLE_HEADER_BYTES = 22
META_HEADER_BYTES = 18
META_HEADER_WORDS = 9
META_EOF = 0x0000
META_CREATEPALETTE = 0x00F7
META_CREATEBRUSH = 0x00F8
META_SELECTCLIPREGION = 0x012C
META_SELECTOBJECT = 0x012D
META_DIBCREATEPATTERNBRUSH = 0x0142
META_DELETEOBJECT = 0x01F0
META_CREATEPATTERNBRUSH = 0x01F9
META_CREATEPENINDIRECT = 0x02FA
META_CREATEFONTINDIRECT = 0x02FB
META_CREATEBRUSHINDIRECT = 0x02FC
META_CREATEREGION = 0x06FF
MAX_RECORDS = 1_000_000


class ParseError(ValueError):
    pass


def u16(raw: bytes, offset: int) -> int:
    end = offset + 2
    if offset < 0 or end > len(raw):
        raise ParseError(f"u16_oob@{offset}")
    return int.from_bytes(raw[offset:end], "little", signed=False)


def u32(raw: bytes, offset: int) -> int:
    end = offset + 4
    if offset < 0 or end > len(raw):
        raise ParseError(f"u32_oob@{offset}")
    return int.from_bytes(raw[offset:end], "little", signed=False)


def parse_ole_presentation(raw: bytes) -> dict:
    marker = u32(raw, 0)
    if marker in STANDARD_MARKERS:
        clipboard = f"standard:{u32(raw, 4)}"
        format_end = 8
    elif marker == 0:
        clipboard = "none"
        format_end = 4
    else:
        if marker > 0x201:
            raise ParseError(f"clipboard_name_too_long:{marker}")
        format_end = 4 + marker
        if format_end > len(raw):
            raise ParseError("clipboard_name_truncated")
        clipboard = f"registered_len:{marker}"

    target_size = u32(raw, format_end)
    if target_size < 4:
        raise ParseError(f"target_device_too_small:{target_size}")
    aspect_offset = format_end + target_size
    if aspect_offset + 28 > len(raw):
        raise ParseError("render_header_truncated")

    aspect = u32(raw, aspect_offset)
    lindex = u32(raw, aspect_offset + 4)
    advf = u32(raw, aspect_offset + 8)
    reserved1 = u32(raw, aspect_offset + 12)
    width = u32(raw, aspect_offset + 16)
    height = u32(raw, aspect_offset + 20)
    data_size = u32(raw, aspect_offset + 24)
    data_offset = aspect_offset + 28
    data_end = data_offset + data_size
    if data_end > len(raw):
        raise ParseError("presentation_data_truncated")
    trailing_len = len(raw) - data_end
    if trailing_len != 0 and trailing_len < 18:
        raise ParseError("presentation_trailer_truncated")

    return {
        "clipboard": clipboard,
        "aspect": aspect,
        "lindex": lindex,
        "advf": advf,
        "reserved1": reserved1,
        "width": width,
        "height": height,
        "data": raw[data_offset:data_end],
        "data_size": data_size,
        "trailing_len": trailing_len,
        "has_reserved2_18": trailing_len >= 18,
    }


def parse_wmf(raw: bytes) -> dict:
    placeable = len(raw) >= 4 and u32(raw, 0) == PLACEABLE_KEY
    header_offset = 0

    if placeable:
        if len(raw) < PLACEABLE_HEADER_BYTES:
            raise ParseError("placeable_header_truncated")
        if u32(raw, 16) != 0:
            raise ParseError("placeable_reserved_nonzero")
        expected = u16(raw, 20)
        actual = 0
        for index in range(10):
            actual ^= u16(raw, index * 2)
        if actual != expected:
            raise ParseError("placeable_checksum_mismatch")
        header_offset = PLACEABLE_HEADER_BYTES

    if header_offset + META_HEADER_BYTES > len(raw):
        raise ParseError("meta_header_truncated")

    metafile_type = u16(raw, header_offset)
    if metafile_type not in (1, 2):
        raise ParseError(f"meta_type:{metafile_type}")

    header_words = u16(raw, header_offset + 2)
    if header_words != META_HEADER_WORDS:
        raise ParseError(f"meta_header_words:{header_words}")

    version = u16(raw, header_offset + 4)
    if version not in (0x0100, 0x0300):
        raise ParseError(f"meta_version:{version:#06x}")

    declared_words = u32(raw, header_offset + 6)
    declared_bytes = declared_words * 2
    if declared_bytes < META_HEADER_BYTES:
        raise ParseError("meta_declared_too_small")
    metafile_end = header_offset + declared_bytes
    if metafile_end != len(raw):
        raise ParseError(f"meta_size_mismatch:{metafile_end}:{len(raw)}")

    object_count = u16(raw, header_offset + 10)
    max_record_words = u32(raw, header_offset + 12)
    if max_record_words < 3:
        raise ParseError(f"max_record_too_small:{max_record_words}")

    functions: list[int] = []
    offset = header_offset + META_HEADER_BYTES
    saw_eof = False
    while offset < metafile_end:
        if len(functions) >= MAX_RECORDS:
            raise ParseError("record_count_limit")

        record_words = u32(raw, offset)
        if record_words < 3:
            raise ParseError(f"record_too_small:{record_words}")
        if record_words > max_record_words:
            raise ParseError(f"record_exceeds_max:{record_words}:{max_record_words}")

        record_end = offset + record_words * 2
        if record_end > metafile_end:
            raise ParseError("record_out_of_bounds")
        function = u16(raw, offset + 4)
        functions.append(function)
        offset = record_end

        if function == META_EOF:
            if offset != metafile_end:
                raise ParseError("eof_not_final")
            saw_eof = True
            break

    if not saw_eof:
        raise ParseError("eof_missing")

    return {
        "placeable": placeable,
        "metafile_type": metafile_type,
        "version": version,
        "declared_bytes": declared_bytes,
        "object_count": object_count,
        "max_record_words": max_record_words,
        "record_count": len(functions),
        "functions": functions,
    }



def audit_wmf_object_lifecycle(raw: bytes) -> dict:
    """Audit object-table allocation/reuse and region selection without retaining source identity."""

    parsed = parse_wmf(raw)
    header_offset = PLACEABLE_HEADER_BYTES if parsed["placeable"] else 0
    metafile_end = header_offset + parsed["declared_bytes"]
    object_count = parsed["object_count"]

    slots: list[dict | None] = [None] * object_count
    ever_used = [False] * object_count

    counts = Counter()
    region_selectobject_reused = 0
    region_selectobject_fresh = 0
    selectclipregion_region = 0
    selectclipregion_nonregion = 0

    def allocate(kind: str) -> None:
        for index, slot in enumerate(slots):
            if slot is None:
                reused = ever_used[index]
                slots[index] = {"kind": kind, "reused": reused}
                ever_used[index] = True
                counts[f"create_{kind}"] += 1
                return
        raise ParseError("object_table_full")

    offset = header_offset + META_HEADER_BYTES
    while offset < metafile_end:
        record_words = u32(raw, offset)
        record_end = offset + record_words * 2
        function = u16(raw, offset + 4)
        params = raw[offset + 6 : record_end]

        if function in (META_CREATEPALETTE, META_CREATEBRUSH, META_CREATEPATTERNBRUSH,
                        META_CREATEFONTINDIRECT):
            allocate("other")
        elif function == META_CREATEPENINDIRECT:
            allocate("pen")
        elif function == META_CREATEBRUSHINDIRECT:
            allocate("brush")
        elif function == META_DIBCREATEPATTERNBRUSH:
            allocate("pattern_brush")
        elif function == META_CREATEREGION:
            allocate("region")
        elif function == META_DELETEOBJECT:
            index = u16(params, 0)
            if index >= len(slots):
                raise ParseError("delete_object_oob")
            if slots[index] is None:
                raise ParseError("delete_object_empty")
            counts[f"delete_{slots[index]['kind']}"] += 1
            slots[index] = None
        elif function == META_SELECTOBJECT:
            index = u16(params, 0)
            if index >= len(slots):
                raise ParseError("select_object_oob")
            slot = slots[index]
            if slot is None:
                raise ParseError("select_object_empty")
            kind = slot["kind"]
            counts[f"selectobject_{kind}"] += 1
            if kind == "region":
                if slot["reused"]:
                    region_selectobject_reused += 1
                else:
                    region_selectobject_fresh += 1
        elif function == META_SELECTCLIPREGION:
            index = u16(params, 0)
            if index >= len(slots):
                raise ParseError("select_clip_region_oob")
            slot = slots[index]
            if slot is None:
                raise ParseError("select_clip_region_empty")
            counts["selectclipregion_total"] += 1
            if slot["kind"] == "region":
                selectclipregion_region += 1
            else:
                selectclipregion_nonregion += 1

        offset = record_end
        if function == META_EOF:
            break

    return {
        "counts": dict(counts),
        "region_selectobject_fresh_slot_count": region_selectobject_fresh,
        "region_selectobject_reused_slot_count": region_selectobject_reused,
        "selectclipregion_region_count": selectclipregion_region,
        "selectclipregion_nonregion_count": selectclipregion_nonregion,
    }


def error_code(exc: Exception) -> str:
    text = str(exc)
    return text.split(":", 1)[0] if text else exc.__class__.__name__


def percentile(values: list[int], numerator: int, denominator: int) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    index = ((len(ordered) - 1) * numerator) // denominator
    return ordered[index]


def profile_corpus(corpus_dir: Path) -> dict:
    import olefile

    paths = sorted(corpus_dir.glob("*.pub"))

    files_with_olepres = 0
    cfb_errors = 0
    olepres_streams = 0
    canonical_streams = 0
    noncanonical_olepres_names = Counter()
    envelope_errors = Counter()
    clipboard_counts = Counter()
    aspect_counts = Counter()
    advf_counts = Counter()
    reserved1_counts = Counter()
    presentation_trailer_len_counts = Counter()
    wmf_errors = Counter()
    wmf_placeable_counts = Counter()
    wmf_type_counts = Counter()
    wmf_version_counts = Counter()
    function_counts = Counter()
    function_file_counts = Counter()
    function_set_profiles = Counter()
    record_counts: list[int] = []
    declared_sizes: list[int] = []
    valid_wmf_count = 0
    object_lifecycle_counts = Counter()
    region_selectobject_fresh_slot_count = 0
    region_selectobject_reused_slot_count = 0
    selectclipregion_region_count = 0
    selectclipregion_nonregion_count = 0

    for path in paths:
        file_has_olepres = False
        file_functions: set[int] = set()
        try:
            with olefile.OleFileIO(str(path)) as ole:
                for parts in ole.listdir(streams=True, storages=False):
                    if not parts:
                        continue
                    leaf = parts[-1]
                    if "olepres" not in leaf.casefold():
                        continue

                    olepres_streams += 1
                    file_has_olepres = True
                    match = OLEPRES_RE.fullmatch(leaf)
                    if match is None:
                        noncanonical_olepres_names["noncanonical"] += 1
                        continue
                    canonical_streams += 1

                    try:
                        raw = ole.openstream(parts).read()
                        pres = parse_ole_presentation(raw)
                    except Exception as exc:
                        envelope_errors[error_code(exc)] += 1
                        continue

                    clipboard_counts[pres["clipboard"]] += 1
                    aspect_counts[str(pres["aspect"])] += 1
                    advf_counts[str(pres["advf"])] += 1
                    reserved1_counts[str(pres["reserved1"])] += 1
                    presentation_trailer_len_counts[str(pres["trailing_len"])] += 1

                    if pres["clipboard"] != f"standard:{CF_METAFILEPICT}":
                        continue

                    try:
                        wmf = parse_wmf(pres["data"])
                    except Exception as exc:
                        wmf_errors[error_code(exc)] += 1
                        continue

                    valid_wmf_count += 1
                    lifecycle = audit_wmf_object_lifecycle(pres["data"])
                    object_lifecycle_counts.update(lifecycle["counts"])
                    region_selectobject_fresh_slot_count += lifecycle["region_selectobject_fresh_slot_count"]
                    region_selectobject_reused_slot_count += lifecycle["region_selectobject_reused_slot_count"]
                    selectclipregion_region_count += lifecycle["selectclipregion_region_count"]
                    selectclipregion_nonregion_count += lifecycle["selectclipregion_nonregion_count"]
                    wmf_placeable_counts[str(wmf["placeable"]).lower()] += 1
                    wmf_type_counts[str(wmf["metafile_type"])] += 1
                    wmf_version_counts[f"0x{wmf['version']:04x}"] += 1
                    record_counts.append(wmf["record_count"])
                    declared_sizes.append(wmf["declared_bytes"])

                    unique_functions = sorted(set(wmf["functions"]))
                    profile = ",".join(f"0x{fn:04x}" for fn in unique_functions)
                    function_set_profiles[profile] += 1
                    for fn in wmf["functions"]:
                        function_counts[f"0x{fn:04x}"] += 1
                    file_functions.update(unique_functions)
        except Exception:
            cfb_errors += 1
            continue

        if file_has_olepres:
            files_with_olepres += 1
        for fn in file_functions:
            function_file_counts[f"0x{fn:04x}"] += 1

    return {
        "schema": "chaptera.olepres-wmf-census.v1",
        "corpus_file_count": len(paths),
        "cfb_parse_error_count": cfb_errors,
        "files_with_olepres": files_with_olepres,
        "olepres_stream_count": olepres_streams,
        "canonical_olepres_stream_count": canonical_streams,
        "noncanonical_olepres_name_counts": dict(noncanonical_olepres_names),
        "presentation_envelope_error_counts": dict(envelope_errors.most_common()),
        "clipboard_format_counts": dict(clipboard_counts.most_common()),
        "aspect_counts": dict(aspect_counts.most_common()),
        "advf_counts": dict(advf_counts.most_common()),
        "reserved1_counts": dict(reserved1_counts.most_common()),
        "presentation_trailer_len_counts": dict(
            sorted(presentation_trailer_len_counts.items(), key=lambda item: int(item[0]))
        ),
        "valid_wmf_count": valid_wmf_count,
        "wmf_error_counts": dict(wmf_errors.most_common()),
        "wmf_placeable_counts": dict(wmf_placeable_counts.most_common()),
        "wmf_type_counts": dict(wmf_type_counts.most_common()),
        "wmf_version_counts": dict(wmf_version_counts.most_common()),
        "wmf_record_count": {
            "min": min(record_counts) if record_counts else None,
            "p50": percentile(record_counts, 1, 2),
            "p90": percentile(record_counts, 9, 10),
            "p99": percentile(record_counts, 99, 100),
            "max": max(record_counts) if record_counts else None,
        },
        "wmf_declared_bytes": {
            "min": min(declared_sizes) if declared_sizes else None,
            "p50": percentile(declared_sizes, 1, 2),
            "p90": percentile(declared_sizes, 9, 10),
            "p99": percentile(declared_sizes, 99, 100),
            "max": max(declared_sizes) if declared_sizes else None,
        },
        "record_function_counts": dict(function_counts.most_common()),
        "record_function_file_counts": dict(function_file_counts.most_common()),
        "object_lifecycle_counts": dict(object_lifecycle_counts.most_common()),
        "region_selection_audit": {
            "selectobject_region_total": (
                region_selectobject_fresh_slot_count + region_selectobject_reused_slot_count
            ),
            "selectobject_region_fresh_slot": region_selectobject_fresh_slot_count,
            "selectobject_region_reused_slot": region_selectobject_reused_slot_count,
            "selectclipregion_region": selectclipregion_region_count,
            "selectclipregion_nonregion": selectclipregion_nonregion_count,
        },
        "function_set_profile_count": len(function_set_profiles),
        "top_function_set_profiles": [
            {"functions": profile.split(",") if profile else [], "count": count}
            for profile, count in function_set_profiles.most_common(50)
        ],
        "evidence_boundary": (
            "source-free aggregate census of persisted OlePres WMF wire structure; "
            "no presentation/native bytes or source paths are retained"
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    summary = profile_corpus(args.corpus_dir)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
