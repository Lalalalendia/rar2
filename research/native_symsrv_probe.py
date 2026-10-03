import ctypes
import hashlib
import json
import os
import sys
import uuid
from pathlib import Path

IDENTITIES = [
    ("wwlib.pdb", "dd4e6758-0ac5-481c-9f58-f09c641067ec", 2, "word2016_kb5002323"),
    ("mspub.pdb", "97dac081-3063-4944-b9a0-424ed2248b1b", 2, "publisher_15601_20088"),
    ("morph9.pdb", "1af3b00e-3013-4e0c-a835-03d6cbc7c5c1", 2, "publisher_15601_20088"),
    ("ptxt9.pdb", "de6dc716-b94b-4a0f-b01d-9650c09d5267", 2, "publisher_15601_20088"),
    ("pubconv.pdb", "d06ca34a-3b35-48ac-94c8-801e201ec8e1", 2, "publisher_15601_20088"),
]

class GUID(ctypes.Structure):
    _fields_ = [
        ("Data1", ctypes.c_uint32),
        ("Data2", ctypes.c_uint16),
        ("Data3", ctypes.c_uint16),
        ("Data4", ctypes.c_ubyte * 8),
    ]

def guid_struct(s):
    u = uuid.UUID(s)
    b = u.bytes_le
    g = GUID()
    g.Data1 = int.from_bytes(b[0:4], "little")
    g.Data2 = int.from_bytes(b[4:6], "little")
    g.Data3 = int.from_bytes(b[6:8], "little")
    for i, v in enumerate(b[8:16]):
        g.Data4[i] = v
    return g

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def find_debuggers():
    roots = [
        Path(r"C:\Program Files (x86)\Windows Kits\10\Debuggers"),
        Path(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\Common7\IDE"),
        Path(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\Common7\IDE\Remote Debugger"),
        Path(r"C:\Program Files (x86)\Microsoft Visual Studio\2022\Enterprise\Common7\IDE"),
    ]
    pairs = []
    seen = set()
    for root in roots:
        if not root.exists():
            continue
        for symsrv in root.rglob("symsrv.dll"):
            dbghelp = symsrv.parent / "dbghelp.dll"
            if dbghelp.exists():
                key = str(symsrv.parent).lower()
                if key not in seen:
                    seen.add(key)
                    pairs.append((dbghelp, symsrv))
    def score(pair):
        p = str(pair[0].parent).lower()
        return (0 if ("\\x64" in p or "\\amd64" in p) else 1, p)
    return sorted(pairs, key=score)

def probe_with_pair(dbghelp_path, symsrv_path):
    dll_dir = str(dbghelp_path.parent)
    if hasattr(os, "add_dll_directory"):
        os.add_dll_directory(dll_dir)
    ctypes.WinDLL(str(symsrv_path), use_last_error=True)
    dbg = ctypes.WinDLL(str(dbghelp_path), use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    dbg.SymInitializeW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]
    dbg.SymInitializeW.restype = ctypes.c_int
    dbg.SymFindFileInPathW.argtypes = [
        ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_void_p,
        ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_wchar_p,
        ctypes.c_void_p, ctypes.c_void_p
    ]
    dbg.SymFindFileInPathW.restype = ctypes.c_int
    dbg.SymCleanup.argtypes = [ctypes.c_void_p]
    dbg.SymCleanup.restype = ctypes.c_int
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p

    cache = Path("out/native-symcache").resolve()
    cache.mkdir(parents=True, exist_ok=True)
    search = f"srv*{cache}*https://msdl.microsoft.com/download/symbols"
    process = kernel32.GetCurrentProcess()
    if not dbg.SymInitializeW(process, search, 0):
        return {"init": False, "error": ctypes.get_last_error(), "records": []}

    records = []
    try:
        for pdb, guid, age, role in IDENTITIES:
            g = guid_struct(guid)
            buf = ctypes.create_unicode_buffer(32768)
            ctypes.set_last_error(0)
            ok = bool(dbg.SymFindFileInPathW(
                process, None, pdb, ctypes.byref(g), age, 0, 0x0008,
                buf, None, None
            ))
            err = ctypes.get_last_error()
            rec = {
                "pdb": pdb,
                "guid": guid,
                "age": age,
                "role": role,
                "found": ok,
                "last_error": err,
                "resolved_path": buf.value if ok else None,
            }
            if ok and buf.value and Path(buf.value).exists():
                p = Path(buf.value)
                rec["size"] = p.stat().st_size
                rec["sha256"] = sha256(p)
            records.append(rec)
    finally:
        dbg.SymCleanup(process)
    return {"init": True, "search_path": search, "records": records}

def main():
    if sys.platform != "win32":
        raise SystemExit("Windows only")
    pairs = find_debuggers()
    out = {
        "schema": "native-symsrv-guid-probe.v1",
        "pairs": [{"dbghelp": str(a), "symsrv": str(b)} for a,b in pairs],
        "attempts": [],
    }
    for dbghelp, symsrv in pairs:
        attempt = {
            "dbghelp": str(dbghelp),
            "symsrv": str(symsrv),
        }
        try:
            attempt["result"] = probe_with_pair(dbghelp, symsrv)
        except OSError as exc:
            attempt["result"] = {
                "init": False,
                "load_error": f"{type(exc).__name__}: {exc}",
                "winerror": getattr(exc, "winerror", None),
                "records": [],
            }
        except Exception as exc:
            attempt["result"] = {
                "init": False,
                "load_error": f"{type(exc).__name__}: {exc}",
                "records": [],
            }
        out["attempts"].append(attempt)
        if attempt["result"].get("init"):
            break
    Path("out").mkdir(exist_ok=True)
    Path("out/native-symsrv.json").write_text(json.dumps(out, indent=2), encoding="utf-8")
    print(json.dumps(out, indent=2))

if __name__ == "__main__":
    main()
