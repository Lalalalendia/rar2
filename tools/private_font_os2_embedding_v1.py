#!/usr/bin/env python3
"""Read-only OpenType OS/2 embedding *signals*, never proof of font rights.

Only used by a private operator-side font inventory. No bytes, paths, legal
licenses, replacement grants, or Publisher/PDF authority are exported.
Reference: https://learn.microsoft.com/en-us/typography/opentype/spec/os2
"""
from __future__ import annotations

import struct

PERMISSION_LABELS = {
    0x0: "installable_indicated",
    0x2: "restricted_license_indicated",
    0x4: "preview_print_only_indicated",
    0x8: "editable_embedding_indicated",
}
# OS/2 table versions 0-5. Some early v0 fonts have the documented
# shortened 68-byte legacy form; every other version needs its complete
# defined table to be treated as readable embedding evidence.
OS2_MIN_BYTES = {0: 68, 1: 86, 2: 96, 3: 96, 4: 96, 5: 100}


def _u16(data: bytes, offset: int) -> int:
    return struct.unpack_from(">H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    return struct.unpack_from(">I", data, offset)[0]


def inspect_os2_embedding_signal(data: bytes, face_index: int = 0) -> dict:
    """Source metadata only; unknown/contradictory bits never grant rights."""
    missing = {
        "os2_embedding_signal": "missing_or_malformed",
        "metadata_only_not_a_license": True,
        "font_license_verified": False,
        "editor_admitted": False,
        "fixed_pdf_allowed": False,
    }
    if not isinstance(data, bytes) or len(data) < 12:
        return missing
    offset = 0
    if data[:4] == b"ttcf":
        if len(data) < 16:
            return missing
        count = _u32(data, 8)
        if count == 0 or count > 4096 or face_index < 0 or face_index >= count:
            return missing
        if 12 + 4 * count > len(data):
            return missing
        offset = _u32(data, 12 + face_index * 4)
    elif face_index != 0:
        return missing
    if offset + 12 > len(data) or data[offset:offset+4] not in (
        b"\x00\x01\x00\x00", b"OTTO", b"true", b"typ1"
    ):
        return missing
    table_count = _u16(data, offset + 4)
    table_dir = offset + 12
    if table_count > 4096 or table_dir + 16 * table_count > len(data):
        return missing
    for i in range(table_count):
        at = table_dir + 16 * i
        if data[at:at+4] != b"OS/2":
            continue
        # SFNT and TTC table offsets are absolute from the container start.
        table_offset, table_size = _u32(data, at + 8), _u32(data, at + 12)
        if table_size < 10 or table_offset + table_size > len(data):
            return missing
        version = _u16(data, table_offset)
        if version not in OS2_MIN_BYTES or table_size < OS2_MIN_BYTES[version]:
            return missing
        fs_type = _u16(data, table_offset + 8)
        permissions = fs_type & 0x000F
        label = PERMISSION_LABELS.get(
            permissions, "invalid_or_ambiguous_permissions"
        )
        # No-subset/bitmap-only bits were undefined in OS/2 v0/v1.
        no_subset = version >= 2 and bool(fs_type & 0x0100)
        bitmap_only = version >= 2 and bool(fs_type & 0x0200)
        if version >= 2 and fs_type & 0xFCF0:
            label = "invalid_or_ambiguous_permissions"
        return {
            "os2_embedding_signal": label,
            "os2_version": version,
            "os2_fs_type_hex": f"0x{fs_type:04x}",
            "no_subsetting_indicated": no_subset,
            "bitmap_only_indicated": bitmap_only,
            "editable_embedding_not_indicated": label in (
                "restricted_license_indicated",
                "preview_print_only_indicated",
                "invalid_or_ambiguous_permissions",
            ) or bitmap_only,
            "metadata_only_not_a_license": True,
            "font_license_verified": False,
            "editor_admitted": False,
            "fixed_pdf_allowed": False,
        }
    return missing
