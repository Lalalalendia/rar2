#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
from collections import Counter

from pub_vba_cfb import CFB_MAGIC, CfbFile, DirEntry, ScanError, decode_codepage, parse_vba_dir_metadata, vba_decompress

def strip_vba_comments(source: str) -> str:
    lines = []
    for line in source.splitlines():
        stripped = line.lstrip()
        if re.match(r"(?i)^rem(?:\s|$)", stripped):
            continue
        out = []
        in_string = False
        i = 0
        while i < len(line):
            ch = line[i]
            if ch == '"':
                out.append(ch)
                if in_string and i + 1 < len(line) and line[i + 1] == '"':
                    out.append('"')
                    i += 2
                    continue
                in_string = not in_string
                i += 1
                continue
            if ch == "'" and not in_string:
                break
            out.append(ch)
            i += 1
        lines.append("".join(out))
    return "\n".join(lines)


CALL_CLASSIFIER_VERSION = "v2"

CALL_PATTERNS: dict[str, list[tuple[str, re.Pattern[str]]]] = {
    "application_lifecycle": [
        ("CreateObject(Publisher.Application)", re.compile(r"(?i)createobject\s*\(\s*\"publisher\.application\"")),
        ("GetObject(Publisher.Application)", re.compile(r"(?i)getobject\s*\([^\)]*\"publisher\.application\"")),
        ("Publisher.Application", re.compile(r"(?i)\bpublisher\.application\b")),
        ("Application.Quit", re.compile(r"(?i)\bapplication\s*\.\s*quit\b|\.quit\b")),
    ],
    "documents": [
        ("ActiveDocument", re.compile(r"(?i)\bactivedocument\b")),
        ("ThisDocument", re.compile(r"(?i)\bthisdocument\b")),
        ("Documents", re.compile(r"(?i)(?:\.|\b)documents\b")),
        ("Documents.Add", re.compile(r"(?i)\bdocuments\s*\.\s*add\b")),
        ("Documents.Open", re.compile(r"(?i)\bdocuments\s*\.\s*open\b")),
    ],
    "pages": [
        ("Pages", re.compile(r"(?i)(?:\.|\b)pages\b")),
    ],
    "page_lifecycle": [
        ("Pages.Add", re.compile(r"(?i)\bpages\s*\.\s*add\b")),
        ("Page.Duplicate", re.compile(r"(?i)\b(?:page|pages\s*\([^\)]*\))\s*\.\s*duplicate\b|\.duplicate\b")),
        ("Page.Delete", re.compile(r"(?i)\b(?:page|pages\s*\([^\)]*\))\s*\.\s*delete\b")),
        ("Page.Move", re.compile(r"(?i)\b(?:page|pages\s*\([^\)]*\))\s*\.\s*move\b")),
    ],
    "shapes": [
        ("Shapes", re.compile(r"(?i)(?:\.|\b)shapes\b")),
        ("ShapeRange", re.compile(r"(?i)\bshaperange\b")),
        ("GroupItems", re.compile(r"(?i)\bgroupitems\b")),
    ],
    "text": [
        ("TextFrame", re.compile(r"(?i)\btextframe\b")),
        ("TextRange", re.compile(r"(?i)\btextrange\b")),
        ("FindReplace", re.compile(r"(?i)\bfindreplace\b")),
        ("Find", re.compile(r"(?i)(?:\.|\b)find\b")),
    ],
    "picture": [
        ("PictureFormat", re.compile(r"(?i)\bpictureformat\b")),
        ("ReplaceEx", re.compile(r"(?i)\breplaceex\b")),
        ("AddPicture", re.compile(r"(?i)\baddpicture\b")),
    ],
    "tables": [
        ("Table", re.compile(r"(?i)(?:\.|\b)table\b")),
        ("Rows", re.compile(r"(?i)(?:\.|\b)rows\b")),
        ("Columns", re.compile(r"(?i)(?:\.|\b)columns\b")),
        ("Cells", re.compile(r"(?i)(?:\.|\b)cells\b")),
        ("AddTable", re.compile(r"(?i)\baddtable\b")),
    ],
    "mail_merge": [
        ("MailMerge", re.compile(r"(?i)\bmailmerge\b")),
        ("DataSource", re.compile(r"(?i)\bdatasource\b")),
        ("DataFields", re.compile(r"(?i)\bdatafields\b")),
        ("Filters", re.compile(r"(?i)(?:\.|\b)filters\b")),
        ("FirstRecord", re.compile(r"(?i)\bfirstrecord\b")),
        ("LastRecord", re.compile(r"(?i)\blastrecord\b")),
    ],
    "layout": [
        ("LayoutGuides", re.compile(r"(?i)\blayoutguides\b")),
        ("RulerGuides", re.compile(r"(?i)\brulerguides\b")),
        ("Align", re.compile(r"(?i)(?:\.|\b)align\b")),
        ("Distribute", re.compile(r"(?i)(?:\.|\b)distribute\b")),
    ],
    "output": [
        ("ExportAsFixedFormat", re.compile(r"(?i)\bexportasfixedformat\b")),
        ("PrintOutEx", re.compile(r"(?i)\bprintoutex\b")),
        ("PrintOut", re.compile(r"(?i)\bprintout\b")),
        ("SaveAsPicture", re.compile(r"(?i)\bsaveaspicture\b")),
        ("SaveAs", re.compile(r"(?i)(?:\.|\b)saveas\b")),
        ("ExportEmailHTML", re.compile(r"(?i)\bexportemailhtml\b")),
        ("WebPagePreview", re.compile(r"(?i)\bwebpagepreview\b")),
    ],
    "hyperlinks": [("Hyperlinks", re.compile(r"(?i)\bhyperlinks?\b"))],
    "linked_text": [
        ("NextLinkedTextFrame", re.compile(r"(?i)\bnextlinkedtextframe\b")),
        ("PreviousLinkedTextFrame", re.compile(r"(?i)\bpreviouslinkedtextframe\b")),
        ("Story", re.compile(r"(?i)(?:\.|\b)story\b")),
    ],
    "metadata_selectors": [
        ("Tags", re.compile(r"(?i)(?:\.|\b)tags\b")),
        ("AlternativeText", re.compile(r"(?i)\balternativetext\b")),
    ],
    "ole_links": [
        ("LinkFormat", re.compile(r"(?i)\blinkformat\b")),
        ("OLEFormat", re.compile(r"(?i)\boleformat\b")),
        ("UpdateOLEObjects", re.compile(r"(?i)\bupdateoleobjects\b")),
    ],
}

STRING_BEARING_SYMBOLS = {
    "CreateObject(Publisher.Application)",
    "GetObject(Publisher.Application)",
}


def mask_vba_strings(source: str) -> str:
    out = []
    i = 0
    while i < len(source):
        if source[i] != '"':
            out.append(source[i])
            i += 1
            continue
        out.append('"')
        i += 1
        while i < len(source):
            if source[i] == '"':
                if i + 1 < len(source) and source[i + 1] == '"':
                    i += 2
                    continue
                out.append('"')
                i += 1
                break
            if source[i] in "\r\n":
                out.append(source[i])
                i += 1
                break
            i += 1
    return "".join(out)


def classify_calls(source: str) -> tuple[dict[str, int], dict[str, int]]:
    cleaned = strip_vba_comments(source)
    stringless = mask_vba_strings(cleaned)
    family_hits: dict[str, int] = {}
    symbol_hits: dict[str, int] = {}
    for family, patterns in CALL_PATTERNS.items():
        family_total = 0
        for symbol, pattern in patterns:
            haystack = cleaned if symbol in STRING_BEARING_SYMBOLS else stringless
            count = len(pattern.findall(haystack))
            if count:
                family_total += count
                symbol_hits[symbol] = symbol_hits.get(symbol, 0) + count
        if family_total:
            family_hits[family] = family_total
    return family_hits, symbol_hits


def _project_children(cfb: CfbFile, storage: DirEntry) -> dict[str, DirEntry]:
    prefix = storage.path
    wanted_len = len(prefix) + 1
    return {
        entry.name.casefold(): entry
        for entry in cfb.entries
        if len(entry.path) == wanted_len and entry.path[: len(prefix)] == prefix
    }


def inspect_vba_project(cfb: CfbFile, storage: DirEntry) -> dict:
    children = _project_children(cfb, storage)
    dir_entry = children.get("dir")
    vba_project_entry = children.get("_vba_project")
    project_root = storage.path[:-1]
    project_entry = cfb.entry(project_root + ("PROJECT",))
    dir_valid = bool(dir_entry and dir_entry.object_type == 2)
    vba_project_valid = bool(vba_project_entry and vba_project_entry.object_type == 2)
    project_valid = bool(project_entry and project_entry.object_type == 2)
    structural_valid = dir_valid and vba_project_valid and project_valid
    result = {
        "structural_valid": structural_valid,
        "dir_status": (
            "missing"
            if dir_entry is None
            else "present" if dir_valid else "wrong_type"
        ),
        "project_stream_status": (
            "missing"
            if project_entry is None
            else "present" if project_valid else "wrong_type"
        ),
        "vba_project_stream_status": (
            "missing"
            if vba_project_entry is None
            else "present" if vba_project_valid else "wrong_type"
        ),
        "dir_metadata_status": "not_attempted",
        "module_count_declared": None,
        "module_streams_found": 0,
        "module_sources_extracted": 0,
        "module_source_failures": 0,
        "codepage": None,
        "call_families": {},
        "symbols": {},
    }
    if not structural_valid:
        return result
    try:
        raw_dir = cfb.read_stream(dir_entry)
    except ScanError as exc:
        result["dir_status"] = f"read_error:{exc}"
        return result
    if not raw_dir:
        result["dir_status"] = "empty"
        return result
    try:
        decompressed_dir = vba_decompress(raw_dir)
    except ScanError as exc:
        result["dir_status"] = f"decompress_error:{exc}"
        return result
    result["dir_status"] = "decompressed"
    metadata = parse_vba_dir_metadata(decompressed_dir)
    result["dir_metadata_status"] = metadata["parse_status"]
    result["codepage"] = metadata["codepage"]
    result["module_count_declared"] = metadata["module_count_declared"]
    family_counter: Counter[str] = Counter()
    symbol_counter: Counter[str] = Counter()
    for module in metadata["modules"]:
        stream_name = decode_codepage(module["stream_name_bytes"], metadata["codepage"])
        module_entry = children.get(stream_name.casefold())
        if module_entry is None:
            result["module_source_failures"] += 1
            continue
        result["module_streams_found"] += 1
        if module["text_offset"] is None:
            result["module_source_failures"] += 1
            continue
        try:
            module_bytes = cfb.read_stream(module_entry)
            offset = module["text_offset"]
            if offset > len(module_bytes):
                raise ScanError("module_text_offset_out_of_bounds")
            source_bytes = vba_decompress(module_bytes[offset:])
        except ScanError:
            result["module_source_failures"] += 1
            continue
        source = decode_codepage(source_bytes, metadata["codepage"])
        families, symbols = classify_calls(source)
        family_counter.update(families)
        symbol_counter.update(symbols)
        result["module_sources_extracted"] += 1
    result["call_families"] = dict(sorted(family_counter.items()))
    result["symbols"] = dict(sorted(symbol_counter.items()))
    return result


def inspect_pub_bytes(data: bytes) -> dict:
    base = {
        "sha256": hashlib.sha256(data).hexdigest(),
        "byte_len": len(data),
        "cfb_status": "ok",
        "vba_state": "absent",
        "vba_project_count": 0,
        "vba_storage_count": 0,
        "vba_projects": [],
        "call_families": {},
        "symbols": {},
        "error": None,
    }
    try:
        cfb = CfbFile(data)
    except ScanError as exc:
        base["cfb_status"] = "parse_failed"
        base["vba_state"] = "unknown"
        base["error"] = str(exc)
        return base
    storages = [e for e in cfb.entries if e.object_type == 1 and e.name.casefold() == "vba"]
    if not storages:
        return base
    projects = [inspect_vba_project(cfb, storage) for storage in storages]
    base["vba_project_count"] = sum(1 for project in projects if project["structural_valid"])
    base["vba_storage_count"] = len(storages)
    base["vba_projects"] = projects
    family_counter: Counter[str] = Counter()
    symbol_counter: Counter[str] = Counter()
    for project in projects:
        family_counter.update(project["call_families"])
        symbol_counter.update(project["symbols"])
    base["call_families"] = dict(sorted(family_counter.items()))
    base["symbols"] = dict(sorted(symbol_counter.items()))
    admitted = [project for project in projects if project["structural_valid"]]
    if any(p["module_sources_extracted"] for p in admitted):
        if any(p["module_source_failures"] for p in admitted):
            base["vba_state"] = "source_partial"
        else:
            base["vba_state"] = "source_extracted"
    elif admitted:
        base["vba_state"] = "structural_only"
    else:
        # Publisher files commonly contain a storage literally named VBA that is
        # not an MS-OVBA project. Do not promote that name collision to macro-present.
        base["vba_state"] = "non_project_vba_storage"
    return base


def scan_path(path: pathlib.Path) -> dict:
    try:
        data = path.read_bytes()
    except OSError as exc:
        return {
            "sha256": None,
            "byte_len": None,
            "cfb_status": "read_failed",
            "vba_state": "unknown",
            "vba_project_count": 0,
            "vba_storage_count": 0,
            "vba_projects": [],
            "call_families": {},
            "symbols": {},
            "error": f"read_failed:{exc.__class__.__name__}",
        }
    return inspect_pub_bytes(data)


def collect_pub_files(inputs: list[pathlib.Path], recursive: bool) -> list[pathlib.Path]:
    files: set[pathlib.Path] = set()
    for item in inputs:
        if item.is_file():
            files.add(item.resolve())
        elif item.is_dir():
            iterator = item.rglob("*") if recursive else item.glob("*")
            for candidate in iterator:
                if candidate.is_file() and candidate.suffix.casefold() == ".pub":
                    files.add(candidate.resolve())
        else:
            raise SystemExit(f"input does not exist: {item}")
    return sorted(files, key=lambda x: str(x).casefold())


def build_receipt(files: list[pathlib.Path], *, include_paths: bool = False) -> dict:
    rows = []
    state_counts: Counter[str] = Counter()
    family_files: Counter[str] = Counter()
    family_hits: Counter[str] = Counter()
    symbol_hits: Counter[str] = Counter()
    for path in files:
        row = scan_path(path)
        state_counts[row["vba_state"]] += 1
        for family, count in row["call_families"].items():
            family_files[family] += 1
            family_hits[family] += count
        symbol_hits.update(row["symbols"])
        if include_paths:
            row["path"] = str(path)
        rows.append(row)
    rows.sort(key=lambda row: (row["sha256"] or "~", row.get("path", "")))
    return {
        "schema": "chaptera.pub-vba-estate-scan.v1",
        "call_classifier_version": CALL_CLASSIFIER_VERSION,
        "claims": {
            "vba_executed": False,
            "ole_com_activated": False,
            "external_links_refreshed": False,
            "publisher_required": False,
            "source_text_emitted": False,
            "frequency_is_market_prevalence": False,
        },
        "totals": {
            "file_count": len(rows),
            "cfb_ok": sum(row["cfb_status"] == "ok" for row in rows),
            "cfb_parse_failed": sum(row["cfb_status"] == "parse_failed" for row in rows),
            "read_failed": sum(row["cfb_status"] == "read_failed" for row in rows),
            "vba_storage_seen_any": sum(row.get("vba_storage_count", 0) > 0 for row in rows),
            "vba_project_present_any": sum(
                row["vba_state"] in ("structural_only", "source_partial", "source_extracted")
                for row in rows
            ),
            "vba_state_counts": dict(sorted(state_counts.items())),
            "call_family_files": dict(sorted(family_files.items())),
            "call_family_hits": dict(sorted(family_hits.items())),
            "symbol_hits": dict(sorted(symbol_hits.items())),
        },
        "files": rows,
    }


def cmd_scan(args) -> int:
    files = collect_pub_files(args.input, args.recursive)
    receipt = build_receipt(files, include_paths=args.include_paths)
    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")
    print(json.dumps(receipt["totals"], indent=2, sort_keys=True))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Inert Microsoft Publisher PUB/CFB VBA estate scanner. Never executes embedded code."
    )
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("scan")
    p.add_argument("--input", action="append", type=pathlib.Path, required=True)
    p.add_argument("--recursive", action="store_true")
    p.add_argument("--include-paths", action="store_true")
    p.add_argument("--output", type=pathlib.Path)
    p.set_defaults(func=cmd_scan)
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
