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
