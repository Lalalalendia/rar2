#!/usr/bin/env python3
"""Source-free style coverage tests; synthetic SFNT name tables only."""
from __future__ import annotations

import hashlib
import json
import sys
import struct
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "ci"))
import audit_private_source_font_styles_v1 as audit
import export_cloud_reader_windows_fonts as exporter
import pub_source_font_requirements_v1 as requirements
from test_export_cloud_reader_windows_fonts import standalone_font, collection_font

STORY = "15613e56-726e-5ae7-8c54-ec876c9bcfda"


def source_packet() -> dict:
    text = "ABCDE"
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    runs = []
    for start, (bold, italic) in enumerate(
        ((False, False), (True, False), (False, True), (True, True))
    ):
        runs.append({
            "story_id": STORY, "scalar_start": start, "scalar_end": start + 1,
            "source_story_text_sha256": digest,
            "source_font_name": "Example Serif", "source_font_index": 7,
            "bold": {"effective_value": bold},
            "italic": {"effective_value": italic},
        })
    runs.append({
        "story_id": STORY, "scalar_start": 4, "scalar_end": 5,
        "source_story_text_sha256": digest,
        "source_font_name": "Example Sans", "source_font_index": 8,
        "bold": None, "italic": None,
    })
    viewer = {
        "schema_version": "0.1",
        "document": {"source": {"format": "pub", "source_hash": "a" * 64},
                     "stories": [{"id": STORY, "text": text}]},
        "story_frames": [{"story_id": STORY, "frame_id": STORY}],
        "typography_runs": runs,
        "script_font_maps": [],
    }
    return requirements.source_font_requirements_v1(viewer)


def record(result: dict, family: str, style: str) -> dict:
    return next(item for item in result["records"]
                if item["source_family"] == family and item["requested_style"] == style)




def font_with_os2(family: str, style: str, fs_type: int, *, version: int = 3) -> bytes:
    """Source-free tiny synthetic SFNT with name and OS/2 tables."""
    original = standalone_font(family, style)
    names = original[28:]
    size_by_version = {0: 68, 1: 86, 2: 96, 3: 96, 4: 96, 5: 100}
    os2_data = bytearray(size_by_version.get(version, 100))
    struct.pack_into(">H", os2_data, 0, version)
    struct.pack_into(">H", os2_data, 4, 700 if "Bold" in style else 400)
    struct.pack_into(">H", os2_data, 8, fs_type)
    struct.pack_into(">H", os2_data, 62, 0x20 if "Bold" in style else 0)
    offset_name = 12 + 2 * 16
    offset_os2 = offset_name + len(names)
    return (
        original[:4] + struct.pack(">HHHH", 2, 0, 0, 0)
        + b"name" + struct.pack(">III", 0, offset_name, len(names))
        + b"OS/2" + struct.pack(">III", 0, offset_os2, len(os2_data))
        + names + bytes(os2_data)
    )

def run() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        folder = root / "fonts"
        folder.mkdir()
        expected = source_packet()
        req = root / "requirements.json"
        req.write_text(json.dumps(expected), encoding="utf-8")
        for name in ("Regular", "Bold", "Italic", "Bold Italic"):
            (folder / ("serif-" + name.replace(" ", "") + ".ttf")).write_bytes(
                standalone_font("Example Serif", name)
            )
        out = audit.audit_style_coverage(req, folder)
        assert out["private_operator_only"] is True
        assert out["required_style_pairs"] == 5
        assert out["coverage_status_counts"] == {
            "single_local_candidate_unverified": 4, "unknown_source_style": 1,
        }
        for style in audit.STYLES:
            item = record(out, "Example Serif", style)
            assert item["status"] == "single_local_candidate_unverified"
            assert item["local_candidate"]["face_index"] == 0
            assert len(item["local_candidate"]["sha256"]) == 64
            for gate in ("source_to_physical_face_verified", "license_verified",
                         "editor_authoring_admitted", "publisher_layout_authoritative",
                         "fixed_pdf_allowed"):
                assert item[gate] is False
        assert record(out, "Example Sans", "unknown")["status"] == "unknown_source_style"
        assert out["oversized_local_font_files"] == 0
        assert all(record(out, "Example Serif", style)["local_candidate"]
                   ["os2_embedding_metadata"]["font_license_verified"] is False
                   for style in audit.STYLES)
        # Embedded-font editing restrictions are negative evidence, not
        # a substitute for original publisher face or license verification.
        (folder / "serif-Bold.ttf").write_bytes(
            font_with_os2("Example Serif", "Bold", 0x0002)
        )
        restricted = record(audit.audit_style_coverage(req, folder), "Example Serif", "bold")
        assert restricted["status"] == "single_local_candidate_unverified"
        assert restricted["local_candidate"]["os2_embedding_metadata"][
            "os2_embedding_signal"
        ] == "restricted_license_indicated"
        assert restricted["local_candidate"]["os2_embedding_metadata"][
            "editable_embedding_not_indicated"
        ] is True
        assert restricted["fixed_pdf_allowed"] is False
        (folder / "serif-Bold.ttf").write_bytes(standalone_font("Example Serif", "Bold"))
        # An installed SemiBold face may NOT fill an exact Bold requirement.
        (folder / "serif-Bold.ttf").unlink()
        (folder / "serif-SemiBold.ttf").write_bytes(
            standalone_font("Example Serif", "SemiBold")
        )
        assert record(audit.audit_style_coverage(req, folder), "Example Serif", "bold")[
            "status"
        ] == "missing_local_face"
        (folder / "serif-Bold.ttf").write_bytes(
            standalone_font("Example Serif", "Bold")
        )
        (folder / "serif-Bold-copy.ttf").write_bytes(
            standalone_font("Example Serif", "Bold", "ExampleSerifBoldDifferent")
        )
        assert record(audit.audit_style_coverage(req, folder), "Example Serif", "bold")[
            "status"
        ] == "ambiguous_local_faces"
        (folder / "serif-Bold-copy.ttf").unlink()
        # Collection face-index 1 is not equivalent to a browser-verified face.
        (folder / "serif-Italic.ttf").unlink()
        (folder / "serif-Italic.ttc").write_bytes(collection_font([
            ("Other", "Regular", "OtherRegular"),
            ("Example Serif", "Italic", "ExampleSerifItalic"),
        ]))
        assert record(audit.audit_style_coverage(req, folder), "Example Serif", "italic")[
            "status"
        ] == "nonzero_collection_face_unsupported"
        # Bad source requirement / potentially forged PDF authority fails.
        fake = dict(expected)
        fake["fixed_pdf_allowed"] = True
        req.write_text(json.dumps(fake), encoding="utf-8")
        try:
            audit.audit_style_coverage(req, folder)
        except exporter.FontPacketError:
            pass
        else:
            raise AssertionError("forged native-output authority accepted")
        req.write_text(json.dumps(expected), encoding="utf-8")
        destination = root / "private-coverage.json"
        assert audit.main(["--requirements", str(req), "--font-dir", str(folder),
                           "--output", str(destination)]) == 0
        assert destination.is_file()
        try:
            audit.main(["--requirements", str(req), "--font-dir", str(folder),
                        "--output", str(destination)])
        except SystemExit as exc:
            assert exc.code == 2
        else:
            raise AssertionError("private local report unexpectedly overwritten")


if __name__ == "__main__":
    run()
    print("PRIVATE_PUB_FONT_STYLE_COVERAGE_OK")
