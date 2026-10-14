"""Pinned, independently verified full OpenType witness for Editor integration.

This parser only attests a bounded sfnt directory and required metrics tables.
It is NOT an arbitrary untrusted-font sanitizer or a layout/shaping engine.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
from pathlib import Path
import struct

from font_authoring_admission_v1 import TrustedFontAuthoringResourceV1

ABEL_SHA256 = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4"
ABEL_RESOURCE_ID = "f27a8036-8492-480f-8fa6-d2e775cc9f12"
ABEL_PATH = Path(__file__).resolve().parents[2] / "assets/fonts/ofl/abel/Abel-Regular.ttf"
ABEL_LICENSE = "SIL Open Font License 1.1"
MAX_BYTES = 32 * 1024 * 1024
REQUIRED_TABLES = frozenset({"head", "hhea", "hmtx", "maxp", "cmap", "name"})


class PinnedFontDenied(ValueError):
    pass


def attest_sfnt(data: bytes) -> int:
    """Validate complete bounded sfnt table ranges, duplicates and key counts."""
    if not isinstance(data, bytes) or len(data) < 12 or len(data) > MAX_BYTES:
        raise PinnedFontDenied("font_size_invalid")
    if data[:4] not in (b"\x00\x01\x00\x00", b"OTTO"):
        raise PinnedFontDenied("unsupported_sfnt_signature")
    num_tables = struct.unpack_from(">H", data, 4)[0]
    if not 1 <= num_tables <= 256 or 12 + 16 * num_tables > len(data):
        raise PinnedFontDenied("sfnt_directory_invalid")
    ranges = []
    tables = {}
    for i in range(num_tables):
        tag, checksum, offset, length = struct.unpack_from(">4sIII", data, 12 + 16 * i)
        if tag in tables or length == 0 or offset < 12 + 16 * num_tables:
            raise PinnedFontDenied("sfnt_table_invalid")
        if offset > len(data) or length > len(data) - offset:
            raise PinnedFontDenied("sfnt_table_out_of_bounds")
        tables[tag] = (offset, length)
        ranges.append((offset, offset + length))
    ranges.sort()
    if any(b[0] < a[1] for a, b in zip(ranges, ranges[1:])):
        raise PinnedFontDenied("sfnt_table_overlap")
    if not REQUIRED_TABLES.issubset({tag.decode("latin1") for tag in tables}):
        raise PinnedFontDenied("sfnt_required_tables_missing")
    head_pos, head_len = tables[b"head"]
    hhea_pos, hhea_len = tables[b"hhea"]
    maxp_pos, maxp_len = tables[b"maxp"]
    hmtx_pos, hmtx_len = tables[b"hmtx"]
    if min(head_len - 54, hhea_len - 36, maxp_len - 6) < 0:
        raise PinnedFontDenied("sfnt_metrics_truncated")
    if struct.unpack_from(">I", data, head_pos + 12)[0] != 0x5F0F3CF5:
        raise PinnedFontDenied("sfnt_head_magic_invalid")
    units = struct.unpack_from(">H", data, head_pos + 18)[0]
    glyphs = struct.unpack_from(">H", data, maxp_pos + 4)[0]
    metrics = struct.unpack_from(">H", data, hhea_pos + 34)[0]
    if not 16 <= units <= 16384 or glyphs == 0 or not 1 <= metrics <= glyphs:
        raise PinnedFontDenied("sfnt_metrics_invalid")
    expected_hmtx = metrics * 4 + (glyphs - metrics) * 2
    if hmtx_len < expected_hmtx:
        raise PinnedFontDenied("sfnt_hmtx_truncated")
    if data[:4] == b"\x00\x01\x00\x00" and not {b"glyf", b"loca"}.issubset(tables):
        raise PinnedFontDenied("sfnt_true_type_outlines_missing")
    if data[:4] == b"OTTO" and b"CFF " not in tables and b"CFF2" not in tables:
        raise PinnedFontDenied("sfnt_cff_outlines_missing")
    return 1  # single sfnt face; TTC collections are intentionally denied


@dataclass(frozen=True)
class PinnedAbelResource:
    raw: bytes
    face_count: int

    def trusted(self, *, tenant_id: str, document_id: str,
                layout_environment_id: str,
                font_set_fingerprint: str) -> TrustedFontAuthoringResourceV1:
        return TrustedFontAuthoringResourceV1(
            tenant_id=tenant_id,
            document_id=document_id,
            layout_environment_id=layout_environment_id,
            font_set_fingerprint=font_set_fingerprint,
            resource_id=ABEL_RESOURCE_ID,
            font_fingerprint="sha256:" + ABEL_SHA256,
            content_hash=ABEL_SHA256,
            face_index=0,
            face_count=self.face_count,
            full_font_bytes=self.raw,
            parser_verified=True,
            is_full_resource=True,
            authoring_admitted=True,
        )


def load_pinned_abel(path: Path = ABEL_PATH) -> PinnedAbelResource:
    """Read only a deployer-pinned file; no host-font lookup or client paths."""
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != ABEL_SHA256:
        raise PinnedFontDenied("pinned_font_sha256_mismatch")
    return PinnedAbelResource(raw=raw, face_count=attest_sfnt(raw))
