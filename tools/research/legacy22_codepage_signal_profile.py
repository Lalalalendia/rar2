#!/usr/bin/env python3
"""Profile persisted codepage signals for legacy 0x22 no-Quill PUB text.

Metadata-only: no recovered document text is retained.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
from collections import Counter
from pathlib import Path

import olefile

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
CONTENTS_MAGIC_0X22 = 0x22
HEADER_PTR_OFF = 0x12
DESCRIPTOR_DELTA = 14
PID_CODEPAGE = 1

SUMMARY = "\x05SummaryInformation"
DOCSUMMARY = "\x05DocumentSummaryInformation"
QUILL_PREFIX = "Quill"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def u32le(buf: bytes, off: int) -> int:
    if off < 0 or off + 4 > len(buf):
        raise ValueError(f"u32 out of bounds at {off:#x}")
    return struct.unpack_from("<I", buf, off)[0]


def codepage_from_property_set(ole: olefile.OleFileIO, name: str):
    if not ole.exists(name):
        return None
    try:
        props = ole.getproperties(name, convert_time=False, no_conversion=[])
    except Exception as exc:
        return {"status": "parse_error", "error": type(exc).__name__}
    raw = props.get(PID_CODEPAGE)
    if raw is None:
        return {"status": "missing"}
    try:
        value = int(raw)
    except Exception:
        return {"status": "non_integer", "repr": repr(raw)[:80]}
    if value < 0:
        value += 65536
    return {"status": "present", "value": value}


def text_range(contents: bytes):
    if len(contents) < 4 or contents[2] != CONTENTS_MAGIC_0X22:
        return None
    hp = u32le(contents, HEADER_PTR_OFF)
    d = hp + DESCRIPTOR_DELTA
    start = u32le(contents, d)
    end = u32le(contents, d + 4)
    if start > end or end > len(contents):
        raise ValueError(f"invalid text range {start:#x}..{end:#x}/{len(contents):#x}")
    return start, end


def printable_candidates(data: bytes) -> list[str]:
    out = []
    current = bytearray()
    for b in data:
        if 0x20 <= b <= 0x7e:
            current.append(b)
        else:
            if len(current) >= 3:
                out.append(current.decode("ascii"))
            current.clear()
    if len(current) >= 3:
        out.append(current.decode("ascii"))
    return out[:8]


def utf16le_candidates(data: bytes) -> list[str]:
    out = []
    for parity in (0, 1):
        buf = bytearray()
        i = parity
        while i + 1 < len(data):
            a, b = data[i], data[i + 1]
            if b == 0 and 0x20 <= a <= 0x7e:
                buf.extend((a, b))
            else:
                if len(buf) >= 6:
                    out.append(buf.decode("utf-16le", errors="strict"))
                buf.clear()
            i += 2
        if len(buf) >= 6:
            out.append(buf.decode("utf-16le", errors="strict"))
    return out[:8]


def legacy_directory(contents: bytes):
    trailer = int.from_bytes(contents[0x16:0x1a], "little")
    if trailer + 2 > len(contents):
        raise ValueError("directory_trailer_oob")
    count = int.from_bytes(contents[trailer:trailer + 2], "little")
    entries = []
    for index in range(count):
        off = trailer + 2 + index * 10
        if off + 10 > len(contents):
            raise ValueError("directory_entry_oob")
        object_id = int.from_bytes(contents[off + 2:off + 4], "little")
        parent_id = int.from_bytes(contents[off + 4:off + 6], "little")
        chunk_offset = int.from_bytes(contents[off + 6:off + 10], "little")
        if chunk_offset + 2 > len(contents):
            raise ValueError("directory_chunk_oob")
        chunk_type = int.from_bytes(contents[chunk_offset:chunk_offset + 2], "little")
        entries.append({
            "index": index,
            "object_id": object_id,
            "parent_id": parent_id,
            "chunk_offset": chunk_offset,
            "chunk_type": chunk_type,
        })
    distinct = sorted({e["chunk_offset"] for e in entries} | {trailer})
    for entry in entries:
        starts = [x for x in distinct if x > entry["chunk_offset"]]
        entry["chunk_end"] = starts[0] if starts else trailer
    return entries


def parse_font_pointer_list(contents: bytes):
    rows = []
    for entry in legacy_directory(contents):
        if entry["chunk_type"] != 0x001e:
            continue
        chunk = contents[entry["chunk_offset"]:entry["chunk_end"]]
        if len(chunk) < 4:
            continue
        decal = chunk[3]
        if decal == 0 or decal >= len(chunk):
            rows.append({
                "directory_index": entry["index"],
                "object_id": entry["object_id"],
                "status": "no_payload",
                "chunk_len": len(chunk),
            })
            continue
        data = chunk[decal:]
        if len(data) < 10:
            rows.append({
                "directory_index": entry["index"],
                "object_id": entry["object_id"],
                "status": "short_payload",
                "chunk_len": len(chunk),
                "payload_len": len(data),
            })
            continue
        a = int.from_bytes(data[0:2], "little")
        b = int.from_bytes(data[2:4], "little")
        ptr_size = 4 if a > b or a == 0 else 2
        read = lambda off: int.from_bytes(data[off:off + ptr_size], "little")
        if len(data) < ptr_size * 3 + 2 + ptr_size:
            continue
        n = read(0)
        nmax = read(ptr_size)
        last_ptr = read(ptr_size * 2)
        off = ptr_size * 3
        f0 = int.from_bytes(data[off:off + 2], "little", signed=True)
        off += 2
        f1 = int.from_bytes(data[off:off + ptr_size], "little", signed=True)
        off += ptr_size
        ptr_base = off
        if n > 4096 or ptr_base + ptr_size * (n + 1) > len(data):
            rows.append({
                "directory_index": entry["index"],
                "object_id": entry["object_id"],
                "status": "pointer_bounds",
                "n": n,
                "nmax": nmax,
                "last_ptr": last_ptr,
                "payload_len": len(data),
            })
            continue
        ptrs = [read(ptr_base + i * ptr_size) for i in range(n + 1)]
        records = []
        for i in range(n):
            lo = ptr_base + ptrs[i]
            hi = ptr_base + ptrs[i + 1]
            if hi <= lo or hi > len(data):
                records.append({"ordinal": i, "status": "invalid_bounds", "lo": lo, "hi": hi})
                continue
            rec = data[lo:hi]
            records.append({
                "ordinal": i,
                "byte_len": len(rec),
                "sha256": sha256(rec),
                "ascii_candidates": printable_candidates(rec),
                "utf16le_candidates": utf16le_candidates(rec),
                "head_hex": rec[:32].hex(),
            })
        rows.append({
            "directory_index": entry["index"],
            "object_id": entry["object_id"],
            "status": "ok",
            "chunk_len": len(chunk),
            "payload_len": len(data),
            "ptr_size": ptr_size,
            "n": n,
            "nmax": nmax,
            "last_ptr": last_ptr,
            "f0": f0,
            "f1": f1,
            "records": records,
        })
    return rows


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    rows = []
    errors = []
    for path in sorted(args.corpus.rglob("*.pub")):
        data = path.read_bytes()
        file_sha = sha256(data)
        row = {"sha256": file_sha, "byte_len": len(data)}
        try:
            if not data.startswith(CFB_MAGIC):
                continue
            with olefile.OleFileIO(path) as ole:
                if not ole.exists("Contents"):
                    continue
                contents = ole.openstream("Contents").read()
                tr = text_range(contents)
                if tr is None:
                    continue
                streams = ["/".join(x) for x in ole.listdir(streams=True, storages=False)]
                has_quill = any(x == QUILL_PREFIX or x.startswith(QUILL_PREFIX + "/") for x in streams)
                if has_quill:
                    continue
                start, end = tr
                body = contents[start:end]
                high = [b for b in body if b >= 0x80]
                row.update({
                    "profile": "legacy22_noquill",
                    "contents_len": len(contents),
                    "text_start": start,
                    "text_end": end,
                    "text_byte_len": len(body),
                    "high_byte_count": len(high),
                    "high_byte_distinct": sorted(set(high)),
                    "high_byte_sha256": sha256(bytes(high)) if high else None,
                    "summary_codepage": codepage_from_property_set(ole, SUMMARY),
                    "document_summary_codepage": codepage_from_property_set(ole, DOCSUMMARY),
                    "font_pointer_lists": parse_font_pointer_list(contents),
                })
                rows.append(row)
        except Exception as exc:
            errors.append({
                "sha256": file_sha,
                "error": f"{type(exc).__name__}:{exc}",
            })

    high_rows = [r for r in rows if r["high_byte_count"] > 0]
    summary_cp = Counter(
        str((r["summary_codepage"] or {}).get("value"))
        for r in high_rows
        if (r["summary_codepage"] or {}).get("status") == "present"
    )
    doc_cp = Counter(
        str((r["document_summary_codepage"] or {}).get("value"))
        for r in high_rows
        if (r["document_summary_codepage"] or {}).get("status") == "present"
    )
    pair_cp = Counter()
    for r in high_rows:
        a = (r["summary_codepage"] or {}).get("value") if (r["summary_codepage"] or {}).get("status") == "present" else None
        b = (r["document_summary_codepage"] or {}).get("value") if (r["document_summary_codepage"] or {}).get("status") == "present" else None
        pair_cp[f"{a}/{b}"] += 1

    high_byte_counts = Counter()
    high_byte_set_counts = Counter()
    for r in high_rows:
        for value in r["high_byte_distinct"]:
            high_byte_counts[f"0x{value:02x}"] += 1
        high_byte_set_counts[",".join(f"{value:02x}" for value in r["high_byte_distinct"])] += 1
    error_counts = Counter(r["error"] for r in errors)

    summary = {
        "schema": "chaptera.legacy22-codepage-signal-profile.v1",
        "legacy22_noquill_file_count": len(rows),
        "non_ascii_text_file_count": len(high_rows),
        "ascii_only_text_file_count": len(rows) - len(high_rows),
        "summary_codepage_counts_on_non_ascii": dict(sorted(summary_cp.items())),
        "document_summary_codepage_counts_on_non_ascii": dict(sorted(doc_cp.items())),
        "codepage_pair_counts_on_non_ascii": dict(sorted(pair_cp.items())),
        "high_byte_presence_counts_on_non_ascii": dict(sorted(high_byte_counts.items())),
        "high_byte_set_counts_on_non_ascii": dict(sorted(high_byte_set_counts.items())),
        "profile_error_count": len(errors),
        "profile_error_counts": dict(sorted(error_counts.items())),
        "evidence_boundary": (
            "OLE Property Set PID_CODEPAGE governs strings in that property set. "
            "It is profiled here only as a persisted discriminator candidate; "
            "it is not assumed to encode Publisher body text."
        ),
    }

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    (args.out / "rows.json").write_text(json.dumps(rows, indent=2) + "\n")
    (args.out / "errors.json").write_text(json.dumps(errors, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
