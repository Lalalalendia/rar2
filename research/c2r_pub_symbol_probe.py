import hashlib
import json
import os
import struct
import sys
import urllib.error
import urllib.request
import uuid
from pathlib import Path

SYMBOL_ROOT = "https://msdl.microsoft.com/download/symbols"
TARGETS = ("MSPUB.EXE", "MORPH9.DLL", "PTXT9.DLL", "PUBCONV.DLL")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def rva_to_offset(rva, sections):
    for va, virtual_size, raw, raw_size in sections:
        if va <= rva < va + max(virtual_size, raw_size):
            return raw + (rva - va)
    return None


def codeview_records(path: Path):
    data = path.read_bytes()
    if len(data) < 0x100 or data[:2] != b"MZ":
        return []
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        return []
    coff = pe + 4
    section_count = struct.unpack_from("<H", data, coff + 2)[0]
    optional_size = struct.unpack_from("<H", data, coff + 16)[0]
    optional = coff + 20
    magic = struct.unpack_from("<H", data, optional)[0]
    if magic == 0x10B:
        data_directory = optional + 96
    elif magic == 0x20B:
        data_directory = optional + 112
    else:
        return []

    debug_rva, debug_size = struct.unpack_from("<II", data, data_directory + 6 * 8)
    section_table = optional + optional_size
    sections = []
    for index in range(section_count):
        s = section_table + index * 40
        virtual_size = struct.unpack_from("<I", data, s + 8)[0]
        va = struct.unpack_from("<I", data, s + 12)[0]
        raw_size = struct.unpack_from("<I", data, s + 16)[0]
        raw = struct.unpack_from("<I", data, s + 20)[0]
        sections.append((va, virtual_size, raw, raw_size))

    debug_offset = rva_to_offset(debug_rva, sections)
    if debug_offset is None:
        return []

    result = []
    for entry in range(debug_offset, min(debug_offset + debug_size, len(data) - 27), 28):
        debug_type = struct.unpack_from("<I", data, entry + 12)[0]
        size = struct.unpack_from("<I", data, entry + 16)[0]
        pointer = struct.unpack_from("<I", data, entry + 24)[0]
        if debug_type != 2 or pointer <= 0 or pointer + size > len(data):
            continue
        blob = data[pointer:pointer + size]
        if blob[:4] == b"RSDS" and len(blob) >= 25:
            guid = uuid.UUID(bytes_le=blob[4:20])
            age = struct.unpack_from("<I", blob, 20)[0]
            pdb_path = blob[24:].split(b"\0", 1)[0].decode("utf-8", "replace")
            result.append({
                "kind": "RSDS",
                "guid": str(guid),
                "age": age,
                "pdb_path": pdb_path,
                "pdb_name": Path(pdb_path.replace("\\", "/")).name,
            })
        elif blob[:4] == b"NB10" and len(blob) >= 17:
            signature, age = struct.unpack_from("<II", blob, 8)
            pdb_path = blob[16:].split(b"\0", 1)[0].decode("utf-8", "replace")
            result.append({
                "kind": "NB10",
                "signature": f"{signature:08X}",
                "age": age,
                "pdb_path": pdb_path,
                "pdb_name": Path(pdb_path.replace("\\", "/")).name,
            })
    return result


def symbol_index(cv):
    if cv["kind"] == "RSDS":
        return uuid.UUID(cv["guid"]).hex.upper() + format(cv["age"], "X")
    return cv["signature"] + format(cv["age"], "X")


def probe_symbol(cv):
    index = symbol_index(cv)
    pdb = cv["pdb_name"]
    compressed = pdb[:-1] + "_" if pdb else pdb
    probes = []
    for candidate in dict.fromkeys((pdb, pdb.lower(), compressed, compressed.lower())):
        if not candidate:
            continue
        url = f"{SYMBOL_ROOT}/{pdb}/{index}/{candidate}"
        req = urllib.request.Request(
            url,
            method="GET",
            headers={"User-Agent": "chaptera-c2r-pub-symbol-census/2", "Range": "bytes=0-0"},
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as response:
                record = {
                    "url": url,
                    "status": int(getattr(response, "status", 200)),
                    "content_length": response.headers.get("Content-Length"),
                    "final_url": response.geturl(),
                }
        except urllib.error.HTTPError as exc:
            record = {
                "url": url,
                "status": int(exc.code),
                "content_length": exc.headers.get("Content-Length"),
            }
        except Exception as exc:
            record = {"url": url, "status": None, "error": f"{type(exc).__name__}: {exc}"}
        probes.append(record)
        if record.get("status") in (200, 206):
            return index, "SYMBOL_PRESENT", record, probes
    return index, "CODEVIEW_PRESENT_SYMBOL_NOT_FOUND", None, probes


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: c2r_pub_symbol_probe.py MODULE_DIR OUTPUT_JSON")
    module_dir = Path(sys.argv[1])
    output = Path(sys.argv[2])
    records = []

    for module in TARGETS:
        path = module_dir / module
        if not path.exists():
            records.append({"module": module, "symbol_result": "MODULE_NOT_FOUND"})
            continue
        base = {
            "module": module,
            "sha256": sha256(path),
            "size": path.stat().st_size,
        }
        cvs = codeview_records(path)
        if not cvs:
            records.append({**base, "codeview": "NO_CODEVIEW", "symbol_result": "NO_CODEVIEW"})
            continue
        for cv in cvs:
            index, verdict, hit, probes = probe_symbol(cv)
            records.append({
                **base,
                **cv,
                "codeview": cv["kind"],
                "symbol_index": index,
                "symbol_result": verdict,
                "symbol_hit": hit,
                "probes": probes,
            })

    summary = {
        "schema": "c2r-publisher-symbol-census.v2",
        "requested_office_version": os.environ.get("PUB_OFFICE_VERSION", "unknown"),
        "requested_architecture": os.environ.get("PUB_OFFICE_ARCH", "unknown"),
        "office_public_symbol_boundary": "16.0.15601.20037",
        "symbol_store": SYMBOL_ROOT,
        "module_records": len(records),
        "codeview_present": sum(r.get("codeview") not in (None, "NO_CODEVIEW") for r in records),
        "symbol_present": sum(r.get("symbol_result") == "SYMBOL_PRESENT" for r in records),
        "symbol_missing": sum(r.get("symbol_result") == "CODEVIEW_PRESENT_SYMBOL_NOT_FOUND" for r in records),
        "records": records,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
