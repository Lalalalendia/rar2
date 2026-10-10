#!/usr/bin/env python3
"""Synthetic OpenType OS/2 fsType negative-evidence verification."""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "ci"))
from private_font_os2_embedding_v1 import inspect_os2_embedding_signal
from test_private_source_font_styles_v1 import font_with_os2
from test_export_cloud_reader_windows_fonts import standalone_font, collection_font


def run() -> None:
    expected = {
        0x0000: "installable_indicated",
        0x0002: "restricted_license_indicated",
        0x0004: "preview_print_only_indicated",
        0x0008: "editable_embedding_indicated",
        0x000C: "invalid_or_ambiguous_permissions",
    }
    for flags, signal in expected.items():
        meta = inspect_os2_embedding_signal(
            font_with_os2("Example Serif", "Bold", flags)
        )
        assert meta["os2_embedding_signal"] == signal, meta
        assert meta["os2_fs_type_hex"] == f"0x{flags:04x}"
        assert meta["font_license_verified"] is False
        assert meta["editor_admitted"] is False
        assert meta["fixed_pdf_allowed"] is False
        assert meta["metadata_only_not_a_license"] is True
    no_subset = inspect_os2_embedding_signal(
        font_with_os2("Example Serif", "Bold", 0x0108)
    )
    assert no_subset["no_subsetting_indicated"] is True
    assert no_subset["bitmap_only_indicated"] is False
    bitmap = inspect_os2_embedding_signal(
        font_with_os2("Example Serif", "Regular", 0x0208)
    )
    assert bitmap["bitmap_only_indicated"] is True
    assert bitmap["editable_embedding_not_indicated"] is True
    # OS/2 v0/v1 high permission bits have no standardized meaning.
    legacy = inspect_os2_embedding_signal(
        font_with_os2("Example Serif", "Regular", 0x0100, version=1)
    )
    assert legacy["no_subsetting_indicated"] is False
    assert legacy["os2_embedding_signal"] == "installable_indicated"
    assert inspect_os2_embedding_signal(
        standalone_font("Example Serif")
    )["os2_embedding_signal"] == "missing_or_malformed"
    ttc = collection_font([
        ("Other", "Regular", "Other"),
        ("Example Serif", "Bold", "ExampleSerifBold"),
    ])
    assert inspect_os2_embedding_signal(ttc, 1)[
        "os2_embedding_signal"
    ] == "missing_or_malformed"
    data = bytearray(font_with_os2("Example Serif", "Bold", 0x0008))
    # Corrupt OS/2 table offset, not allowed to escape container bounds.
    offset_of_os2_record = 12 + 16
    data[offset_of_os2_record+8:offset_of_os2_record+12] = (0xFFFFFF00).to_bytes(4, "big")
    assert inspect_os2_embedding_signal(bytes(data))[
        "os2_embedding_signal"
    ] == "missing_or_malformed"


if __name__ == "__main__":
    run()
    print("PRIVATE_FONT_OS2_RESTRICTIONS_OK")
