#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "tools" / "pub_vba_estate_scan.py"
spec = importlib.util.spec_from_file_location("pub_vba_estate_scan", MODULE_PATH)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
assert spec.loader is not None
spec.loader.exec_module(module)

FREE = 0xFFFFFFFF
END = 0xFFFFFFFE
FAT = 0xFFFFFFFD


def literal_vba_container(data: bytes) -> bytes:
    out = bytearray([1])
    for chunk_start in range(0, len(data), 3000):
        chunk = data[chunk_start:chunk_start + 3000]
        payload = bytearray()
        for group_start in range(0, len(chunk), 8):
            group = chunk[group_start:group_start + 8]
            payload.append(0)
            payload.extend(group)
        assert len(payload) <= 4096
        header = 0xB000 | (len(payload) - 1)
        out.extend(struct.pack("<H", header))
        out.extend(payload)
    return bytes(out)


def compressed_copy_container(prefix: bytes, offset: int, length: int) -> bytes:
    assert 1 <= len(prefix) <= 16
    groups = []
    remaining = prefix
    while len(remaining) >= 8:
        groups.append(bytes([0x00]) + remaining[:8])
        remaining = remaining[8:]
    difference = len(prefix)
    bit_count = max(4, (difference - 1).bit_length())
    length_bits = 16 - bit_count
    token = ((offset - 1) << length_bits) | (length - 3)
    copy = struct.pack("<H", token)
    if remaining:
        groups.append(bytes([1 << len(remaining)]) + remaining + copy)
    else:
        groups.append(bytes([0x01]) + copy)
    data = b"".join(groups)
    header = 0xB000 | (len(data) - 1)
    return b"\x01" + struct.pack("<H", header) + data


def directory_entry(name: str, object_type: int, *, left=FREE, right=FREE, child=FREE, start=END, size=0) -> bytes:
    row = bytearray(128)
    encoded = (name + "\x00").encode("utf-16le") if name else b""
    assert len(encoded) <= 64 * 2
    row[:len(encoded)] = encoded
    struct.pack_into("<H", row, 64, len(encoded))
    row[66] = object_type
    row[67] = 1
    struct.pack_into("<III", row, 68, left, right, child)
    struct.pack_into("<I", row, 116, start)
    struct.pack_into("<Q", row, 120, size)
    return bytes(row)


def minimal_cfb(with_vba: bool, *, complete_vba: bool = True) -> bytes:
    sector_size = 512
    header = bytearray(sector_size)
    header[:8] = module.CFB_MAGIC
    struct.pack_into("<H", header, 24, 0x003E)
    struct.pack_into("<H", header, 26, 3)
    struct.pack_into("<H", header, 28, 0xFFFE)
    struct.pack_into("<H", header, 30, 9)
    struct.pack_into("<H", header, 32, 6)
    struct.pack_into("<I", header, 40, 0)
    struct.pack_into("<I", header, 44, 1)
    struct.pack_into("<I", header, 48, 0)
    struct.pack_into("<I", header, 56, 4096)
    struct.pack_into("<I", header, 60, END)
    struct.pack_into("<I", header, 64, 0)
    struct.pack_into("<I", header, 68, END)
    struct.pack_into("<I", header, 72, 0)
    for i in range(109):
        struct.pack_into("<I", header, 76 + i * 4, 1 if i == 0 else FREE)

    if with_vba and complete_vba:
        entries = [
            directory_entry("Root Entry", 5, child=1),
            directory_entry("VBA", 1, child=2),
            directory_entry("dir", 2, right=3),
            directory_entry("_VBA_PROJECT", 2),
        ]
    elif with_vba:
        entries = [
            directory_entry("Root Entry", 5, child=1),
            directory_entry("VBA", 1),
        ]
    else:
        entries = [directory_entry("Root Entry", 5)]
    directory = b"".join(entries).ljust(sector_size, b"\x00")
    fat = bytearray(sector_size)
    values = [END, FAT] + [FREE] * (sector_size // 4 - 2)
    struct.pack_into(f"<{len(values)}I", fat, 0, *values)
    return bytes(header + directory + fat)


def stream_record(record_id: int, payload: bytes) -> bytes:
    return struct.pack("<HI", record_id, len(payload)) + payload


def source_macro_cfb(source: bytes) -> bytes:
    name = b"Module1"
    name_u = "Module1".encode("utf-16le")
    module_record = b"".join([
        stream_record(0x0019, name),
        stream_record(0x0047, name_u),
        stream_record(0x001A, name),
        struct.pack("<HI", 0x0032, len(name_u)) + name_u,
        stream_record(0x001C, b""),
        struct.pack("<HI", 0x0048, 0),
        stream_record(0x0031, struct.pack("<I", 0)),
        stream_record(0x001E, struct.pack("<I", 0)),
        stream_record(0x002C, struct.pack("<H", 0xFFFF)),
        struct.pack("<HI", 0x0021, 0),
        struct.pack("<HI", 0x002B, 0),
    ])
    dir_plain = b"".join([
        stream_record(0x0003, struct.pack("<H", 1252)),
        stream_record(0x000F, struct.pack("<H", 1)),
        stream_record(0x0013, struct.pack("<H", 0xFFFF)),
        module_record,
        struct.pack("<HI", 0x0010, 0),
    ])
    dir_stream = literal_vba_container(dir_plain)
    module_stream = literal_vba_container(source)

    sector_size = 512
    header = bytearray(sector_size)
    header[:8] = module.CFB_MAGIC
    struct.pack_into("<H", header, 24, 0x003E)
    struct.pack_into("<H", header, 26, 3)
    struct.pack_into("<H", header, 28, 0xFFFE)
    struct.pack_into("<H", header, 30, 9)
    struct.pack_into("<H", header, 32, 6)
    struct.pack_into("<I", header, 40, 0)
    struct.pack_into("<I", header, 44, 1)
    struct.pack_into("<I", header, 48, 0)
    struct.pack_into("<I", header, 56, 0)
    struct.pack_into("<I", header, 60, END)
    struct.pack_into("<I", header, 64, 0)
    struct.pack_into("<I", header, 68, END)
    struct.pack_into("<I", header, 72, 0)
    for i in range(109):
        struct.pack_into("<I", header, 76 + i * 4, 4 if i == 0 else FREE)

    entries = [
        directory_entry("Root Entry", 5, child=1),
        directory_entry("VBA", 1, child=2),
        directory_entry("dir", 2, right=3, start=2, size=len(dir_stream)),
        directory_entry("_VBA_PROJECT", 2, right=4),
        directory_entry("Module1", 2, start=3, size=len(module_stream)),
    ]
    directory_bytes = b"".join(entries).ljust(sector_size * 2, b"\x00")
    dir_sector_0 = directory_bytes[:sector_size]
    dir_sector_1 = directory_bytes[sector_size:sector_size * 2]
    data_dir = dir_stream.ljust(sector_size, b"\x00")
    data_module = module_stream.ljust(sector_size, b"\x00")
    fat = bytearray(sector_size)
    values = [1, END, END, END, FAT] + [FREE] * (sector_size // 4 - 5)
    struct.pack_into(f"<{len(values)}I", fat, 0, *values)
    return bytes(header + dir_sector_0 + dir_sector_1 + data_dir + data_module + fat)


def main() -> int:
    source = b'Attribute VB_Name = "M"\r\nSub X()\r\nActiveDocument.Pages(1).Shapes(1).TextFrame.TextRange.Text = "x"\r\nEnd Sub\r\n'
    assert module.vba_decompress(literal_vba_container(source)) == source
    assert module.vba_decompress(compressed_copy_container(b"abc", 3, 3)) == b"abcabc"
    sixteen = b"abcdefghijklmnop"
    assert module.vba_decompress(compressed_copy_container(sixteen, 16, 3)) == sixteen + b"abc"

    raw_payload = bytes((i % 251 for i in range(4096)))
    raw_header = struct.pack("<H", 0x3FFF)
    assert module.vba_decompress(b"\x01" + raw_header + raw_payload) == raw_payload

    calls = """\nRem MailMerge.DataSource\n' ExportAsFixedFormat\nSet p = ActiveDocument.Pages(1)\np.Shapes(1).TextFrame.TextRange.Text = \"ok\"\nActiveDocument.ExportAsFixedFormat pbFixedFormatTypePDF, \"x.pdf\"\n"""
    families, symbols = module.classify_calls(calls)
    assert families["pages"] >= 1
    assert families["shapes"] >= 1
    assert families["text"] >= 2
    assert families["output"] == 1
    assert "mail_merge" not in families
    assert symbols["ExportAsFixedFormat"] == 1

    string_calls = 'msg = "MailMerge.DataSource ExportAsFixedFormat"\nSet app = CreateObject("Publisher.Application")\n'
    string_families, string_symbols = module.classify_calls(string_calls)
    assert "mail_merge" not in string_families
    assert "output" not in string_families
    assert string_families["application_lifecycle"] == 1
    assert string_symbols["CreateObject(Publisher.Application)"] == 1

    extracted_source = b'Attribute VB_Name = "Module1"\r\nSub X()\r\nActiveDocument.Pages(1).Shapes(1).TextFrame.TextRange.Text = "x"\r\nActiveDocument.ExportAsFixedFormat 2, "x.pdf"\r\nEnd Sub\r\n'
    extracted = module.inspect_pub_bytes(source_macro_cfb(extracted_source))
    assert extracted["cfb_status"] == "ok"
    assert extracted["vba_state"] == "source_extracted"
    assert extracted["vba_projects"][0]["module_sources_extracted"] == 1
    assert extracted["call_families"]["pages"] >= 1
    assert extracted["call_families"]["shapes"] >= 1
    assert extracted["call_families"]["text"] >= 1
    assert extracted["call_families"]["output"] == 1

    macro = module.inspect_pub_bytes(minimal_cfb(True))
    assert macro["cfb_status"] == "ok"
    assert macro["vba_state"] == "structural_only"
    assert macro["vba_project_count"] == 1
    assert macro["vba_projects"][0]["structural_valid"] is True

    plain = module.inspect_pub_bytes(minimal_cfb(False))
    assert plain["cfb_status"] == "ok"
    assert plain["vba_state"] == "absent"

    name_collision = module.inspect_pub_bytes(minimal_cfb(True, complete_vba=False))
    assert name_collision["cfb_status"] == "ok"
    assert name_collision["vba_state"] == "non_project_vba_storage"
    assert name_collision["vba_project_count"] == 0

    bad = module.inspect_pub_bytes(b"not a cfb")
    assert bad["cfb_status"] == "parse_failed"
    assert bad["vba_state"] == "unknown"

    assert module.build_receipt([], include_paths=False)["claims"]["source_text_emitted"] is False

    print({"tests": "ok", "macro_state": macro["vba_state"], "plain_state": plain["vba_state"]})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
