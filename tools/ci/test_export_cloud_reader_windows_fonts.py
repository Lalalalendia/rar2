#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import json
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


def test_private_requirements_regular_only_fence(tmp: Path) -> None:
    private_module = MODULE_PATH.parent / "pub_source_font_requirements_v1.py"
    source_spec = importlib.util.spec_from_file_location("source_requirements", private_module)
    assert source_spec and source_spec.loader
    module = importlib.util.module_from_spec(source_spec)
    sys.modules[source_spec.name] = module
    source_spec.loader.exec_module(module)
    source_sha = "a" * 64
    story_id = "15613e56-726e-5ae7-8c54-ec876c9bcfda"
    text = "Hello!"
    proof = hashlib.sha256(text.encode()).hexdigest()
    def run(start: int, end: int, family: str, bold: bool) -> dict:
        return {
            "story_id": story_id, "scalar_start": start, "scalar_end": end,
            "source_story_text_sha256": proof, "source_font_name": family,
            "bold": {"effective_value": bold},
            "italic": {"effective_value": False},
        }
    viewer = {
        "schema_version": "0.1",
        "document": {
            "source": {"format": "pub", "source_hash": source_sha},
            "stories": [{"id": story_id, "text": text}],
        },
        "story_frames": [{"story_id": story_id, "frame_id": story_id}],
        "typography_runs": [
            run(0, 3, "Example Serif", False),
            run(3, 6, "Example Sans", True),
        ],
        "script_font_maps": [{
            "story_id": story_id, "scalar_start": 0, "scalar_end": 6,
            "source_story_text_sha256": proof, "entries": [
                {"script_slot": 2, "source_font_index": 7,
                 "source_font_name": "Example Serif", "disposition": "resolved"},
                {"script_slot": 2, "source_font_index": 8,
                 "source_font_name": "Example Sans", "disposition": "resolved"},
            ],
        }],
    }
    viewer["typography_runs"][0]["source_font_index"] = 7
    viewer["typography_runs"][1]["source_font_index"] = 8
    requirements = module.source_font_requirements_v1(viewer)
    assert requirements["direct_source_binding_count"] == 2
    plan = tmp / "requirements.json"
    plan.write_text(json.dumps(requirements), encoding="utf-8")
    font_dir = tmp / "installed-private-fonts"
    font_dir.mkdir()
    for family in ("Example Serif", "Example Sans"):
        (font_dir / (family.replace(" ", "") + ".ttf")).write_bytes(standalone_font(family))

    families, source = font_export.read_private_source_requirements(plan)
    assert set(families) == {"Example Serif", "Example Sans"}
    assert source["source_sha256"] == source_sha
    assert source["partial_style_or_source_coverage"] is True
    blocked = tmp / "blocked"
    assert font_export.main([
        "--requirements", str(plan), "--font-dir", str(font_dir),
        "--output-dir", str(blocked),
    ]) == 2
    assert not blocked.exists(), "incomplete style gate must precede font copying"

    out = tmp / "private-partial"
    assert font_export.main([
        "--requirements", str(plan), "--font-dir", str(font_dir),
        "--output-dir", str(out), "--allow-incomplete-styles",
    ]) == 0
    manifest = json.loads((out / "manifest.json").read_text(encoding="utf-8"))
    assert len(manifest["resources"]) == 2
    metadata = manifest["publisher_source_requirements"]
    assert metadata["source_sha256"] == source_sha
    assert metadata["source_family_requirements_only"] is True
    assert metadata["partial_style_or_source_coverage"] is True
    assert metadata["regular_only_packet"] is True
    assert metadata["private_licensed_source_face_mapping_unverified"] is True
    assert metadata["publisher_visual_parity_verified"] is False
    assert metadata["fixed_pdf_allowed"] is False
    crosswalk = manifest["source_binding_file_candidates"]
    assert len(crosswalk) == 2
    expected_by_family = {
        item["source_family"]: item for item in manifest["resources"]
    }
    for record in crosswalk:
        expected = expected_by_family[record["source_family"]]
        assert record["packet_file"] == expected["packet_file"]
        assert record["packet_sha256"] == expected["sha256"]
        assert record["face_index"] == 0
        assert record["source_to_physical_face_verified"] is False
        assert record["editor_authoring_admitted"] is False
        assert record["native_publisher_layout_authoritative"] is False
        assert record["fixed_pdf_allowed"] is False
        assert record["source_font_binding_id"] == font_export._editor_source_binding_id(
            source_sha, record["source_font_index"], record["source_family"]
        )
    assert font_export._editor_source_binding_id(
        "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
        18, "Rockwell Condensed",
    ) == "pub-source-font:b402e726-72b4-5d52-bb0d-07a0c13dc05d"
    for record in manifest["resources"]:
        assert (out / record["packet_file"]).is_file()

    # A script-font alternative can be a valid source candidate but does not
    # authorize inventing a direct typography-run font index, even when the
    # attacker can compute a self-consistent UUIDv5 identifier.
    script_only = json.loads(plan.read_text(encoding="utf-8"))
    serif = next(f for f in script_only["families"]
                 if f["source_family"] == "Example Serif")
    assert serif["direct_run_source_font_indices"] == [7]
    serif["source_font_index_candidates"] = [7, 9]
    script_only["direct_source_bindings"].append({
        "story_id": story_id,
        "scalar_start": 2,
        "scalar_end": 3,
        "source_family": "Example Serif",
        "source_font_index": 9,
        "source_font_binding_id": font_export._editor_source_binding_id(
            source_sha, 9, "Example Serif",
        ),
    })
    script_only["direct_source_binding_count"] += 1
    plan.write_text(json.dumps(script_only), encoding="utf-8")
    try:
        font_export.read_private_source_requirements(plan)
    except font_export.FontPacketError as exc:
        assert "not canonical source evidence" in str(exc)
    else:
        raise AssertionError("script-only Quill alternative invented direct font identity")
    serif["direct_run_source_font_indices"] = [7, 9]
    plan.write_text(json.dumps(script_only), encoding="utf-8")
    # Even an internally consistent but tampered packet cannot become Editor
    # admission. This parser is explicitly non-authorizing.
    recovered, provenance = font_export.read_private_source_requirements(plan)
    assert len(provenance["direct_binding_identity_candidates"]) == 3
    assert provenance["native_publisher_layout_authoritative"] is False
    assert provenance["fixed_pdf_allowed"] is False
    plan.write_text(json.dumps(requirements), encoding="utf-8")

    forged = json.loads(plan.read_text(encoding="utf-8"))
    forged["direct_source_bindings"][0]["source_font_binding_id"] = (
        "pub-source-font:00000000-0000-5000-8000-000000000000"
    )
    plan.write_text(json.dumps(forged), encoding="utf-8")
    try:
        font_export.read_private_source_requirements(plan)
    except font_export.FontPacketError as exc:
        assert "not canonical source evidence" in str(exc)
    else:
        raise AssertionError("forged source-to-file candidate identity accepted")
    forged["direct_source_bindings"][0]["source_font_binding_id"] = (
        requirements["direct_source_bindings"][0]["source_font_binding_id"]
    )
    forged["direct_source_binding_count"] += 1
    plan.write_text(json.dumps(forged), encoding="utf-8")
    try:
        font_export.read_private_source_requirements(plan)
    except font_export.FontPacketError as exc:
        assert "untrusted Publisher direct source-binding" in str(exc)
    else:
        raise AssertionError("invented source binding count accepted")
    plan.write_text(json.dumps(requirements), encoding="utf-8")

    fake = json.loads(plan.read_text(encoding="utf-8"))
    fake["fixed_pdf_allowed"] = True
    plan.write_text(json.dumps(fake), encoding="utf-8")
    try:
        font_export.read_private_source_requirements(plan)
    except font_export.FontPacketError as exc:
        assert "falsely authorized" in str(exc)
    else:
        raise AssertionError("source font names must never authorize a PDF or Publisher layout")


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
            ("requirements", test_private_requirements_regular_only_fence),
        ]:
            case = root / name
            case.mkdir()
            test(case)
    print("cloud reader Windows exact-font packet exporter: PASS")


if __name__ == "__main__":
    main()
