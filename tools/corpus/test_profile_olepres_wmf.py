#!/usr/bin/env python3

from profile_olepres_wmf import (
    CF_METAFILEPICT,
    META_CREATEREGION,
    META_DIBCREATEPATTERNBRUSH,
    META_EOF,
    META_POLYGON,
    META_RECTANGLE,
    META_SELECTCLIPREGION,
    META_SELECTOBJECT,
    ParseError,
    classify_special_object_selections,
    parse_ole_presentation,
    parse_wmf,
)


def minimal_wmf() -> bytes:
    raw = bytearray()
    raw += (1).to_bytes(2, "little")
    raw += (9).to_bytes(2, "little")
    raw += (0x0300).to_bytes(2, "little")
    raw += (12).to_bytes(4, "little")
    raw += (0).to_bytes(2, "little")
    raw += (3).to_bytes(4, "little")
    raw += (0).to_bytes(2, "little")
    raw += (3).to_bytes(4, "little")
    raw += (0).to_bytes(2, "little")
    return bytes(raw)



def record(function: int, params: bytes = b"") -> bytes:
    assert len(params) % 2 == 0
    raw = bytearray()
    raw += ((6 + len(params)) // 2).to_bytes(4, "little")
    raw += function.to_bytes(2, "little")
    raw += params
    return bytes(raw)


def wmf_with_records(records: list[bytes], object_count: int = 4) -> bytes:
    payload = b"".join(records + [record(META_EOF)])
    raw = bytearray()
    raw += (1).to_bytes(2, "little")
    raw += (9).to_bytes(2, "little")
    raw += (0x0300).to_bytes(2, "little")
    raw += ((18 + len(payload)) // 2).to_bytes(4, "little")
    raw += object_count.to_bytes(2, "little")
    raw += max(len(item) // 2 for item in records + [record(META_EOF)]).to_bytes(
        4, "little"
    )
    raw += (0).to_bytes(2, "little")
    raw += payload
    return bytes(raw)


def dib_pattern_params() -> bytes:
    dib = bytearray(40)
    dib[0:4] = (40).to_bytes(4, "little")
    dib[4:8] = (8).to_bytes(4, "little", signed=True)
    dib[8:12] = (8).to_bytes(4, "little", signed=True)
    dib[12:14] = (1).to_bytes(2, "little")
    dib[14:16] = (1).to_bytes(2, "little")
    dib[16:20] = (0).to_bytes(4, "little")
    dib[20:24] = (8).to_bytes(4, "little")
    dib[32:36] = (2).to_bytes(4, "little")
    return (6).to_bytes(2, "little") + (0).to_bytes(2, "little") + bytes(dib)


def region_params() -> bytes:
    raw = bytearray()
    raw += (0).to_bytes(2, "little")
    raw += (6).to_bytes(2, "little", signed=True)
    raw += (0).to_bytes(4, "little")
    raw += (34).to_bytes(2, "little", signed=True)
    raw += (1).to_bytes(2, "little", signed=True)
    raw += (2).to_bytes(2, "little", signed=True)
    raw += bytes(8)
    raw += (2).to_bytes(2, "little")
    raw += (0).to_bytes(2, "little")
    raw += (1).to_bytes(2, "little")
    raw += (0).to_bytes(2, "little")
    raw += (4).to_bytes(2, "little")
    raw += (2).to_bytes(2, "little")
    return bytes(raw)


def region_params_with_zero_scan_tail() -> bytes:
    raw = bytearray(region_params())
    raw[8:10] = (42).to_bytes(2, "little", signed=True)
    raw += (0).to_bytes(2, "little")  # Count
    raw += (7).to_bytes(2, "little")  # Top
    raw += (7).to_bytes(2, "little")  # Bottom
    raw += (0).to_bytes(2, "little")  # Count2
    assert len(raw) == 42
    return bytes(raw)


def presentation(payload: bytes, trailer_len: int = 18) -> bytes:
    raw = bytearray()
    raw += (0xFFFFFFFF).to_bytes(4, "little")
    raw += CF_METAFILEPICT.to_bytes(4, "little")
    raw += (4).to_bytes(4, "little")
    raw += (1).to_bytes(4, "little")
    raw += (0xFFFFFFFF).to_bytes(4, "little")
    raw += (2).to_bytes(4, "little")
    raw += (0).to_bytes(4, "little")
    raw += (640).to_bytes(4, "little")
    raw += (480).to_bytes(4, "little")
    raw += len(payload).to_bytes(4, "little")
    raw += payload
    raw += bytes(trailer_len)
    return bytes(raw)


def main() -> int:
    wmf = minimal_wmf()
    parsed_wmf = parse_wmf(wmf)
    assert parsed_wmf["record_count"] == 1
    assert parsed_wmf["functions"] == [0]

    parsed_pres = parse_ole_presentation(presentation(wmf))
    assert parsed_pres["clipboard"] == "standard:3"
    assert parsed_pres["aspect"] == 1
    assert parsed_pres["has_reserved2_18"] is True
    assert parsed_pres["trailing_len"] == 18
    assert parse_wmf(parsed_pres["data"])["version"] == 0x0300

    eof_variant = parse_ole_presentation(presentation(wmf, trailer_len=0))
    assert eof_variant["trailing_len"] == 0
    assert eof_variant["has_reserved2_18"] is False
    assert parse_wmf(eof_variant["data"])["version"] == 0x0300

    try:
        parse_ole_presentation(presentation(wmf, trailer_len=17))
    except ParseError as exc:
        assert str(exc) == "presentation_trailer_truncated"
    else:
        raise AssertionError("non-empty short trailer must fail closed")

    bad = bytearray(wmf)
    bad[6:10] = (11).to_bytes(4, "little")
    try:
        parse_wmf(bytes(bad))
    except ParseError as exc:
        assert str(exc).startswith("meta_size_mismatch")
    else:
        raise AssertionError("declared-size mismatch must fail closed")


    pattern = wmf_with_records(
        [
            record(META_DIBCREATEPATTERNBRUSH, dib_pattern_params()),
            record(META_SELECTOBJECT, (0).to_bytes(2, "little")),
            record(META_POLYGON),
        ]
    )
    pattern_selections = classify_special_object_selections(pattern)
    assert len(pattern_selections) == 1
    pattern_selection = pattern_selections[0]
    assert pattern_selection["kind"] == "pattern_brush"
    assert pattern_selection["selection_record"] == "META_SELECTOBJECT"
    assert pattern_selection["creation_profile"]["dib_header_bytes"] == 40
    assert pattern_selection["creation_profile"]["width"] == 8
    assert pattern_selection["creation_profile"]["height"] == 8
    assert pattern_selection["fill_draw_counts"] == {"0x0324": 1}

    region = wmf_with_records(
        [
            record(META_CREATEREGION, region_params()),
            record(META_SELECTOBJECT, (0).to_bytes(2, "little")),
            record(META_RECTANGLE),
        ]
    )
    region_selections = classify_special_object_selections(region)
    assert len(region_selections) == 1
    region_selection = region_selections[0]
    assert region_selection["kind"] == "region"
    assert region_selection["selection_record"] == "META_SELECTOBJECT"
    assert region_selection["creation_profile"]["object_type"] == 6
    assert region_selection["creation_profile"]["scan_count"] == 1
    assert region_selection["creation_profile"]["scan_structure"] == "valid_exact"
    assert region_selection["creation_profile"]["total_scan_coordinates"] == 2
    assert region_selection["supported_draw_counts"] == {"0x041b": 1}

    region_tail = wmf_with_records(
        [
            record(META_CREATEREGION, region_params_with_zero_scan_tail()),
            record(META_SELECTOBJECT, (0).to_bytes(2, "little")),
            record(META_RECTANGLE),
        ]
    )
    region_tail_selection = classify_special_object_selections(region_tail)[0]
    tail_profile = region_tail_selection["creation_profile"]
    assert tail_profile["payload_bytes"] == 42
    assert tail_profile["scan_structure"] == "valid_with_tail"
    assert tail_profile["tail_bytes"] == 8
    assert tail_profile["tail_scan_candidate"] == {
        "count": 0,
        "count2_matches": True,
        "zero_count": True,
        "vertical_relation": "equal",
    }

    clip_region = wmf_with_records(
        [
            record(META_CREATEREGION, region_params()),
            record(META_SELECTCLIPREGION, (0).to_bytes(2, "little")),
            record(META_RECTANGLE),
        ]
    )
    clip_selections = classify_special_object_selections(clip_region)
    assert len(clip_selections) == 1
    assert clip_selections[0]["selection_record"] == "META_SELECTCLIPREGION"

    no_eof = bytearray(wmf)
    no_eof[-2:] = (0x0103).to_bytes(2, "little")
    try:
        parse_wmf(bytes(no_eof))
    except ParseError as exc:
        assert str(exc) == "eof_missing"
    else:
        raise AssertionError("missing EOF must fail closed")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
