#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
from pathlib import Path
import struct
import sys
import tempfile

MODULE_PATH = Path(__file__).resolve().parents[1] / "export_cloud_reader_windows_fonts.py"
if not MODULE_PATH.is_file():
    MODULE_PATH = Path(__file__).with_name("export_cloud_reader_windows_fonts.py")
spec = importlib.util.spec_from_file_location("font_export", MODULE_PATH)
assert spec and spec.loader
font_export = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = font_export
spec.loader.exec_module(font_export)


def name_table(family: str, subfamily: str, postscript: str) -> bytes:
    values = [(16, family), (17, subfamily), (1, family), (2, subfamily), (6, postscript)]
    strings = bytearray()
    records = bytearray()
    for name_id, value in values:
        raw = value.encode("utf-16-be")
        offset = len(strings)
        strings.extend(raw)
        records.extend(struct.pack(">HHHHHH", 3, 1, 0x0409, name_id, len(raw), offset))
    string_offset = 6 + len(records)
    return struct.pack(">HHH", 0, len(values), string_offset) + records + strings


def standalone_font(family: str, subfamily: str = "Regular", postscript: str | None = None, *, otf: bool = False) -> bytes:
    postscript = postscript or family.replace(" ", "")
    names = name_table(family, subfamily, postscript)
    table_offset = 28
    scaler = b"OTTO" if otf else b"\x00\x01\x00\x00"
    header = scaler + struct.pack(">HHHH", 1, 0, 0, 0)
    record = b"name" + struct.pack(">III", 0, table_offset, len(names))
    return header + record + names


def collection_font(faces: list[tuple[str, str, str]]) -> bytes:
    count = len(faces)
    header_size = 12 + 4 * count
    face_offsets = [header_size + 28 * index for index in range(count)]
    name_tables = [name_table(*face) for face in faces]
    name_offsets = []
    cursor = header_size + 28 * count
    for table in name_tables:
        name_offsets.append(cursor)
        cursor += len(table)
    out = bytearray()
    out.extend(b"ttcf")
    out.extend(struct.pack(">I", 0x00010000))
    out.extend(struct.pack(">I", count))
    for offset in face_offsets:
        out.extend(struct.pack(">I", offset))
    for face_offset, name_offset, names in zip(face_offsets, name_offsets, name_tables):
        assert len(out) == face_offset
        out.extend(b"\x00\x01\x00\x00" + struct.pack(">HHHH", 1, 0, 0, 0))
        out.extend(b"name" + struct.pack(">III", 0, name_offset, len(names)))
    for names in name_tables:
        out.extend(names)
    return bytes(out)


def test_standalone_parser(tmp: Path) -> None:
    path = tmp / "Calibri.ttf"
    path.write_bytes(standalone_font("Calibri", "Regular", "Calibri"))
    faces = font_export.parse_font_faces(path)
    assert len(faces) == 1
    face = faces[0]
    assert face.family == "Calibri"
    assert face.subfamily == "Regular"
    assert face.postscript_name == "Calibri"
    assert face.face_index == 0
    assert face.container_kind == "sfnt"
    assert face.mime == "font/ttf"


def test_otf_parser(tmp: Path) -> None:
    path = tmp / "Example.otf"
    path.write_bytes(standalone_font("Example Serif", "Regular", "ExampleSerif", otf=True))
    face = font_export.parse_font_faces(path)[0]
    assert face.mime == "font/otf"


def test_collection_and_nonzero_face_fence(tmp: Path) -> None:
    path = tmp / "Cambria.ttc"
    path.write_bytes(collection_font([
        ("Cambria", "Regular", "Cambria"),
        ("Cambria Math", "Regular", "CambriaMath"),
    ]))
    faces = font_export.parse_font_faces(path)
    assert [(f.family, f.face_index) for f in faces] == [("Cambria", 0), ("Cambria Math", 1)]
    assert font_export.discover_regular_face(tmp, "Cambria").face_index == 0
    try:
        font_export.discover_regular_face(tmp, "Cambria Math")
    except font_export.FontPacketError as exc:
        assert "non-zero collection-face paint" in str(exc)
    else:
        raise AssertionError("non-zero collection face must fail closed")


def test_regular_selection_ignores_light(tmp: Path) -> None:
    (tmp / "calibri-light.ttf").write_bytes(standalone_font("Calibri", "Light", "Calibri-Light"))
    regular = tmp / "calibri.ttf"
    regular.write_bytes(standalone_font("Calibri", "Regular", "Calibri"))
    face = font_export.discover_regular_face(tmp, "Calibri")
    assert face.path == regular


def test_packet_generation(tmp: Path) -> None:
    fonts = tmp / "fonts-source"
    fonts.mkdir()
    calibri = standalone_font("Calibri", "Regular", "Calibri")
    cambria = collection_font([
        ("Cambria", "Regular", "Cambria"),
        ("Cambria Math", "Regular", "CambriaMath"),
    ])
    (fonts / "Calibri.ttf").write_bytes(calibri)
    (fonts / "Cambria.ttc").write_bytes(cambria)
    out = tmp / "packet"
    manifest = font_export.build_packet(
        ["Calibri", "Cambria"], fonts, out, "/etc/chaptera/fonts"
    )
    assert manifest["schema"] == font_export.TOOL_SCHEMA
    assert manifest["private_operator_packet"] is True
    assert len(manifest["resources"]) == 2
    by_family = {item["source_family"]: item for item in manifest["resources"]}
    assert by_family["Calibri"]["sha256"] == hashlib.sha256(calibri).hexdigest()
    assert by_family["Cambria"]["sha256"] == hashlib.sha256(cambria).hexdigest()
    assert by_family["Cambria"]["face_index"] == 0
    assert by_family["Cambria"]["browser_collection_policy"] == "first_face_only"
    toml = (out / "cloud-reader-font-resources.toml").read_text(encoding="utf-8")
    assert toml.count("[[cloud_reader_guest.font_resources]]") == 2
    assert 'source_family = "Calibri"' in toml
    assert 'source_family = "Cambria"' in toml
    assert "face_index = 0" in toml
    assert str(fonts) not in (out / "manifest.json").read_text(encoding="utf-8")
    for item in manifest["resources"]:
        assert (out / item["packet_file"]).is_file()


def test_cloud_path_fence(tmp: Path) -> None:
    fonts = tmp / "fonts-source"
    fonts.mkdir()
    (fonts / "Calibri.ttf").write_bytes(standalone_font("Calibri", "Regular", "Calibri"))
    for bad in ["relative/fonts", "/etc/chaptera/../tmp"]:
        try:
            font_export.build_packet(["Calibri"], fonts, tmp / ("out-" + str(len(bad))), bad)
        except font_export.FontPacketError as exc:
            assert "absolute normalized POSIX path" in str(exc)
        else:
            raise AssertionError(f"unsafe cloud path must fail closed: {bad}")


def test_ambiguous_family_fails(tmp: Path) -> None:
    (tmp / "a.ttf").write_bytes(standalone_font("Calibri", "Regular", "CalibriA"))
    (tmp / "b.ttf").write_bytes(standalone_font("Calibri", "Regular", "CalibriB"))
    try:
        font_export.discover_regular_face(tmp, "Calibri")
    except font_export.FontPacketError as exc:
        assert "ambiguous" in str(exc)
    else:
        raise AssertionError("duplicate Regular faces must fail closed")


def main() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        for name, test in [
            ("standalone", test_standalone_parser),
            ("otf", test_otf_parser),
            ("collection", test_collection_and_nonzero_face_fence),
            ("regular_selection", test_regular_selection_ignores_light),
            ("packet", test_packet_generation),
            ("cloud_path_fence", test_cloud_path_fence),
            ("ambiguous", test_ambiguous_family_fails),
        ]:
            case = root / name
            case.mkdir()
            test(case)
    print("cloud reader Windows exact-font packet exporter: PASS")


if __name__ == "__main__":
    main()
