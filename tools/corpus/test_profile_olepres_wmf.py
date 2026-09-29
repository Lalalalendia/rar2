#!/usr/bin/env python3

from profile_olepres_wmf import (
    CF_METAFILEPICT,
    META_CREATEREGION,
    META_DELETEOBJECT,
    META_SELECTCLIPREGION,
    META_SELECTOBJECT,
    ParseError,
    audit_wmf_object_lifecycle,
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
    words = 3 + len(params) // 2
    return words.to_bytes(4, "little") + function.to_bytes(2, "little") + params


def lifecycle_wmf(records: list[bytes], object_count: int = 1) -> bytes:
    body = b"".join(records) + record(0)
    max_record_words = max(len(item) // 2 for item in records + [record(0)])
    total_words = (18 + len(body)) // 2
    raw = bytearray()
    raw += (1).to_bytes(2, "little")
    raw += (9).to_bytes(2, "little")
    raw += (0x0300).to_bytes(2, "little")
    raw += total_words.to_bytes(4, "little")
    raw += object_count.to_bytes(2, "little")
    raw += max_record_words.to_bytes(4, "little")
    raw += (0).to_bytes(2, "little")
    raw += body
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

    no_eof = bytearray(wmf)
    no_eof[-2:] = (0x0103).to_bytes(2, "little")
    try:
        parse_wmf(bytes(no_eof))
    except ParseError as exc:
        assert str(exc) == "eof_missing"
    else:
        raise AssertionError("missing EOF must fail closed")

    fresh_region = lifecycle_wmf(
        [
            record(META_CREATEREGION),
            record(META_SELECTOBJECT, (0).to_bytes(2, "little")),
        ]
    )
    audit = audit_wmf_object_lifecycle(fresh_region)
    assert audit["region_selectobject_fresh_slot_count"] == 1
    assert audit["region_selectobject_reused_slot_count"] == 0
    assert audit["selectclipregion_region_count"] == 0

    reused_region = lifecycle_wmf(
        [
            record(META_CREATEREGION),
            record(META_DELETEOBJECT, (0).to_bytes(2, "little")),
            record(META_CREATEREGION),
            record(META_SELECTOBJECT, (0).to_bytes(2, "little")),
            record(META_SELECTCLIPREGION, (0).to_bytes(2, "little")),
        ]
    )
    audit = audit_wmf_object_lifecycle(reused_region)
    assert audit["region_selectobject_fresh_slot_count"] == 0
    assert audit["region_selectobject_reused_slot_count"] == 1
    assert audit["selectclipregion_region_count"] == 1
    assert audit["selectclipregion_nonregion_count"] == 0

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
