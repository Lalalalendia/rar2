#!/usr/bin/env python3

from profile_olepres_wmf import (
    CF_METAFILEPICT,
    ParseError,
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


def presentation(payload: bytes) -> bytes:
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
    raw += bytes(18)
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
    assert parse_wmf(parsed_pres["data"])["version"] == 0x0300

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

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
