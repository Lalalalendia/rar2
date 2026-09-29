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
MAX_RECORDS = 1_000_000

META_CREATEPENINDIRECT = 0x02FA
META_CREATEBRUSHINDIRECT = 0x02FC
META_DIBCREATEPATTERNBRUSH = 0x0142
META_CREATEREGION = 0x06FF
META_DELETEOBJECT = 0x01F0
META_SELECTCLIPREGION = 0x012C
META_SELECTOBJECT = 0x012D
META_POLYGON = 0x0324
META_POLYLINE = 0x0325
META_RECTANGLE = 0x041B
META_POLYPOLYGON = 0x0538

SUPPORTED_DRAW_FUNCTIONS = {
    META_POLYGON,
    META_POLYLINE,
    META_RECTANGLE,
    META_POLYPOLYGON,
}
FILL_DRAW_FUNCTIONS = {
    META_POLYGON,
    META_RECTANGLE,
    META_POLYPOLYGON,
}


class ParseError(ValueError):
    pass


def u16(raw: bytes, offset: int) -> int:
    end = offset + 2
    if offset < 0 or end > len(raw):
        raise ParseError(f"u16_oob@{offset}")
    return int.from_bytes(raw[offset:end], "little", signed=False)


def i16(raw: bytes, offset: int) -> int:
    end = offset + 2
    if offset < 0 or end > len(raw):
        raise ParseError(f"i16_oob@{offset}")
    return int.from_bytes(raw[offset:end], "little", signed=True)


def i32(raw: bytes, offset: int) -> int:
    end = offset + 4
    if offset < 0 or end > len(raw):
        raise ParseError(f"i32_oob@{offset}")
    return int.from_bytes(raw[offset:end], "little", signed=True)


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



def wmf_records(raw: bytes) -> tuple[dict, list[tuple[int, bytes]]]:
    parsed = parse_wmf(raw)
    header_offset = PLACEABLE_HEADER_BYTES if parsed["placeable"] else 0
    metafile_end = header_offset + parsed["declared_bytes"]
    offset = header_offset + META_HEADER_BYTES
    records: list[tuple[int, bytes]] = []

    while offset < metafile_end:
        record_words = u32(raw, offset)
        record_end = offset + record_words * 2
        function = u16(raw, offset + 4)
        records.append((function, raw[offset + 6 : record_end]))
        offset = record_end
        if function == META_EOF:
            break

    return parsed, records


def dib_pattern_brush_profile(params: bytes) -> dict:
    profile = {
        "payload_bytes": len(params),
        "style": u16(params, 0) if len(params) >= 2 else None,
        "color_usage": u16(params, 2) if len(params) >= 4 else None,
        "dib_bytes": max(0, len(params) - 4),
        "dib_header_bytes": None,
        "width": None,
        "height": None,
        "planes": None,
        "bit_count": None,
        "compression": None,
        "image_bytes": None,
        "colors_used": None,
    }
    target = params[4:] if len(params) >= 4 else b""
    if len(target) < 4:
        return profile

    header_bytes = u32(target, 0)
    profile["dib_header_bytes"] = header_bytes
    if header_bytes >= 40 and len(target) >= 40:
        profile.update(
            {
                "width": i32(target, 4),
                "height": i32(target, 8),
                "planes": u16(target, 12),
                "bit_count": u16(target, 14),
                "compression": u32(target, 16),
                "image_bytes": u32(target, 20),
                "colors_used": u32(target, 32),
            }
        )
    return profile


def region_profile(params: bytes) -> dict:
    profile = {
        "payload_bytes": len(params),
        "object_type": None,
        "region_size": None,
        "scan_count": None,
        "max_scan": None,
        "parsed_scan_count": 0,
        "total_scan_coordinates": 0,
        "max_scan_coordinates": 0,
        "scan_structure": "header_truncated",
        "exact_payload_consumed": False,
    }
    if len(params) < 22:
        return profile

    object_type = i16(params, 2)
    region_size = i16(params, 8)
    scan_count = i16(params, 10)
    max_scan = i16(params, 12)
    profile.update(
        {
            "object_type": object_type,
            "region_size": region_size,
            "scan_count": scan_count,
            "max_scan": max_scan,
        }
    )
    if scan_count < 0:
        profile["scan_structure"] = "negative_scan_count"
        return profile

    cursor = 22
    total_coordinates = 0
    max_coordinates = 0
    for _ in range(scan_count):
        if cursor + 8 > len(params):
            profile["scan_structure"] = "scan_header_truncated"
            return profile
        count = u16(params, cursor)
        if count % 2 != 0:
            profile["scan_structure"] = "odd_scan_coordinate_count"
            return profile
        scan_end = cursor + 8 + count * 2
        if scan_end > len(params):
            profile["scan_structure"] = "scan_points_truncated"
            return profile
        count2 = u16(params, cursor + 6 + count * 2)
        if count2 != count:
            profile["scan_structure"] = "scan_count2_mismatch"
            return profile
        total_coordinates += count
        max_coordinates = max(max_coordinates, count)
        profile["parsed_scan_count"] += 1
        cursor = scan_end

    profile["total_scan_coordinates"] = total_coordinates
    profile["max_scan_coordinates"] = max_coordinates
    profile["exact_payload_consumed"] = cursor == len(params)

    tail = params[cursor:]
    profile["tail_bytes"] = len(tail)
    profile["tail_scan_candidate"] = None
    if len(tail) == 8:
        tail_count = u16(tail, 0)
        tail_top = u16(tail, 2)
        tail_bottom = u16(tail, 4)
        tail_count2 = u16(tail, 6)
        profile["tail_scan_candidate"] = {
            "count": tail_count,
            "count2_matches": tail_count2 == tail_count,
            "zero_count": tail_count == 0,
            "vertical_relation": (
                "ascending"
                if tail_bottom > tail_top
                else "equal"
                if tail_bottom == tail_top
                else "descending"
            ),
        }

    profile["scan_structure"] = (
        "valid_exact" if profile["exact_payload_consumed"] else "valid_with_tail"
    )
    return profile


def _profile_key(payload: dict) -> str:
    return json.dumps(payload, sort_keys=True, separators=(",", ":"))


def classify_special_object_selections(raw: bytes) -> list[dict]:
    parsed, records = wmf_records(raw)
    object_slots: list[dict | None] = [None] * max(1, int(parsed["object_count"]))
    active: dict[str, dict | None] = {"pattern_brush": None, "region": None}
    completed: list[dict] = []

    def allocate(kind: str, creation_profile: dict) -> None:
        for index, slot in enumerate(object_slots):
            if slot is None:
                object_slots[index] = {
                    "kind": kind,
                    "creation_profile": creation_profile,
                }
                return
        object_slots.append({"kind": kind, "creation_profile": creation_profile})

    def close(kind: str) -> None:
        current = active.get(kind)
        if current is None:
            return
        current["function_counts"] = dict(current["function_counts"].most_common())
        current["supported_draw_counts"] = dict(
            current["supported_draw_counts"].most_common()
        )
        current["fill_draw_counts"] = dict(current["fill_draw_counts"].most_common())
        completed.append(current)
        active[kind] = None

    for function, params in records:
        selected = None
        selected_index = None
        if function in (META_SELECTOBJECT, META_SELECTCLIPREGION) and len(params) >= 2:
            selected_index = u16(params, 0)
            if selected_index < len(object_slots):
                selected = object_slots[selected_index]

        if function == META_SELECTOBJECT and selected is not None:
            if selected["kind"] in ("brush", "pattern_brush"):
                close("pattern_brush")
            elif selected["kind"] == "region":
                close("region")
        elif function == META_SELECTCLIPREGION:
            close("region")
        elif function == META_DELETEOBJECT and len(params) >= 2:
            delete_index = u16(params, 0)
            for kind in ("pattern_brush", "region"):
                current = active.get(kind)
                if current is not None and current["object_index"] == delete_index:
                    close(kind)

        for current in active.values():
            if current is None:
                continue
            key = f"0x{function:04x}"
            current["record_count"] += 1
            current["function_counts"][key] += 1
            if len(current["first_functions"]) < 16:
                current["first_functions"].append(key)
            if function in SUPPORTED_DRAW_FUNCTIONS:
                current["supported_draw_counts"][key] += 1
            if function in FILL_DRAW_FUNCTIONS:
                current["fill_draw_counts"][key] += 1

        if function == META_CREATEPENINDIRECT:
            allocate("pen", {"payload_bytes": len(params)})
        elif function == META_CREATEBRUSHINDIRECT:
            allocate("brush", {"payload_bytes": len(params)})
        elif function == META_DIBCREATEPATTERNBRUSH:
            allocate("pattern_brush", dib_pattern_brush_profile(params))
        elif function == META_CREATEREGION:
            allocate("region", region_profile(params))
        elif function == META_DELETEOBJECT and len(params) >= 2:
            delete_index = u16(params, 0)
            if delete_index < len(object_slots):
                object_slots[delete_index] = None
        elif function in (META_SELECTOBJECT, META_SELECTCLIPREGION):
            if selected is None or selected_index is None:
                continue
            kind = selected["kind"]
            if kind == "pattern_brush" and function == META_SELECTOBJECT:
                active["pattern_brush"] = {
                    "kind": kind,
                    "selection_record": "META_SELECTOBJECT",
                    "object_index": selected_index,
                    "creation_profile": selected["creation_profile"],
                    "record_count": 0,
                    "first_functions": [],
                    "function_counts": Counter(),
                    "supported_draw_counts": Counter(),
                    "fill_draw_counts": Counter(),
                }
            elif kind == "region":
                active["region"] = {
                    "kind": kind,
                    "selection_record": (
                        "META_SELECTCLIPREGION"
                        if function == META_SELECTCLIPREGION
                        else "META_SELECTOBJECT"
                    ),
                    "object_index": selected_index,
                    "creation_profile": selected["creation_profile"],
                    "record_count": 0,
                    "first_functions": [],
                    "function_counts": Counter(),
                    "supported_draw_counts": Counter(),
                    "fill_draw_counts": Counter(),
                }

        if function == META_EOF:
            close("pattern_brush")
            close("region")

    close("pattern_brush")
    close("region")
    return completed


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
    special_selection_events = Counter()
    special_selection_stream_counts = Counter()
    special_selection_file_counts = Counter()
    special_creation_profiles = {
        "pattern_brush": Counter(),
        "region": Counter(),
    }
    special_followup_profiles = {
        "pattern_brush": Counter(),
        "region": Counter(),
    }

    for path in paths:
        file_has_olepres = False
        file_functions: set[int] = set()
        file_special_kinds: set[str] = set()
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

                    selections = classify_special_object_selections(pres["data"])
                    stream_special_kinds = set()
                    for selection in selections:
                        kind = selection["kind"]
                        mode = selection["selection_record"]
                        event_key = f"{kind}:{mode}"
                        special_selection_events[event_key] += 1
                        stream_special_kinds.add(event_key)
                        special_creation_profiles[kind][
                            _profile_key(selection["creation_profile"])
                        ] += 1
                        followup = {
                            "selection_record": mode,
                            "record_count": selection["record_count"],
                            "first_functions": selection["first_functions"],
                            "supported_draw_counts": selection["supported_draw_counts"],
                            "fill_draw_counts": selection["fill_draw_counts"],
                        }
                        special_followup_profiles[kind][_profile_key(followup)] += 1
                    for event_key in stream_special_kinds:
                        special_selection_stream_counts[event_key] += 1
                    file_functions.update(unique_functions)
                    if stream_special_kinds:
                        file_special_kinds.update(stream_special_kinds)
        except Exception:
            cfb_errors += 1
            continue

        if file_has_olepres:
            files_with_olepres += 1
        for fn in file_functions:
            function_file_counts[f"0x{fn:04x}"] += 1
        for event_key in file_special_kinds:
            special_selection_file_counts[event_key] += 1

    def profile_rows(counter: Counter) -> list[dict]:
        rows = []
        for encoded, count in counter.most_common():
            rows.append({"profile": json.loads(encoded), "count": count})
        return rows

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
        "function_set_profile_count": len(function_set_profiles),
        "top_function_set_profiles": [
            {"functions": profile.split(",") if profile else [], "count": count}
            for profile, count in function_set_profiles.most_common(50)
        ],
        "selected_special_object_census": {
            "selection_event_counts": dict(special_selection_events.most_common()),
            "selection_stream_counts": dict(
                special_selection_stream_counts.most_common()
            ),
            "selection_file_counts": dict(special_selection_file_counts.most_common()),
            "creation_profiles": {
                kind: profile_rows(counter)
                for kind, counter in special_creation_profiles.items()
            },
            "followup_profiles": {
                kind: profile_rows(counter)
                for kind, counter in special_followup_profiles.items()
            },
        },
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
