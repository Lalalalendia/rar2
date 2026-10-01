#!/usr/bin/env python3
"""Source-safe structural census of embedded EOT records in an exact PUB."""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path

import olefile


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def find_eot_candidates(contents: bytes) -> list[dict[str, int | str]]:
    candidates: list[dict[str, int | str]] = []
    cursor = 0
    while True:
        record_offset = contents.find(b"\x0c\x80", cursor)
        if record_offset < 0:
            break
        cursor = record_offset + 1
        if record_offset + 42 > len(contents):
            continue

        block_data_length = u32(contents, record_offset + 2)
        eot_offset = record_offset + 6
        eot_size = u32(contents, eot_offset)
        font_data_size = u32(contents, eot_offset + 4)
        version = u32(contents, eot_offset + 8)
        flags = u32(contents, eot_offset + 12)
        charset = contents[eot_offset + 26]
        weight = u32(contents, eot_offset + 28)
        fs_type = u16(contents, eot_offset + 32)
        magic = u16(contents, eot_offset + 34)

        if magic != 0x504C:
            continue
        if block_data_length != eot_size + 4:
            continue
        if eot_size < 36 or eot_offset + eot_size > len(contents):
            continue
        if font_data_size > eot_size:
            continue

        eot = contents[eot_offset : eot_offset + eot_size]
        candidates.append(
            {
                "eot_offset_in_contents": eot_offset,
                "eot_size": eot_size,
                "font_data_size": font_data_size,
                "version": f"0x{version:08x}",
                "flags": f"0x{flags:08x}",
                "charset": charset,
                "weight": weight,
                "fs_type": f"0x{fs_type:04x}",
                "eot_sha256": sha256_hex(eot),
            }
        )
    return candidates


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pub", required=True, type=Path)
    parser.add_argument("--expected-pub-sha256", required=True)
    parser.add_argument("--receipt", required=True, type=Path)
    args = parser.parse_args()

    pub_bytes = args.pub.read_bytes()
    pub_sha256 = sha256_hex(pub_bytes)
    if pub_sha256 != args.expected_pub_sha256.lower():
        raise SystemExit(
            f"PUB SHA-256 mismatch: expected {args.expected_pub_sha256.lower()} got {pub_sha256}"
        )

    with olefile.OleFileIO(str(args.pub)) as compound:
        if not compound.exists("Contents"):
            raise SystemExit("exact PUB has no Contents stream")
        contents = compound.openstream("Contents").read()

    candidates = find_eot_candidates(contents)
    receipt = {
        "schema": "chaptera.embedded-eot-census.v1",
        "source_pub_sha256": pub_sha256,
        "source_pub_byte_len": len(pub_bytes),
        "contents_byte_len": len(contents),
        "structurally_valid_eot_count": len(candidates),
        "candidates": candidates,
        "claims": {
            "source_text_emitted": False,
            "font_program_bytes_emitted": False,
            "font_family_name_emitted": False,
            "embedded_font_name_join_claimed": False,
        },
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        "CLOUD_READER_EMBEDDED_EOT_CENSUS "
        f"source_sha256={pub_sha256} valid_eot={len(candidates)} "
        f"candidate_hashes={[candidate['eot_sha256'] for candidate in candidates]}"
    )


if __name__ == "__main__":
    main()
