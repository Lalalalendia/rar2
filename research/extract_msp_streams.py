import ctypes
import hashlib
import json
import re
import sys
from pathlib import Path

if sys.platform != "win32":
    raise SystemExit("Windows only")

msi = ctypes.WinDLL("msi.dll")

UINT = ctypes.c_uint
DWORD = ctypes.c_uint32
MSIHANDLE = ctypes.c_uint

msi.MsiOpenDatabaseW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.POINTER(MSIHANDLE)]
msi.MsiOpenDatabaseW.restype = UINT
msi.MsiDatabaseOpenViewW.argtypes = [MSIHANDLE, ctypes.c_wchar_p, ctypes.POINTER(MSIHANDLE)]
msi.MsiDatabaseOpenViewW.restype = UINT
msi.MsiViewExecute.argtypes = [MSIHANDLE, MSIHANDLE]
msi.MsiViewExecute.restype = UINT
msi.MsiViewFetch.argtypes = [MSIHANDLE, ctypes.POINTER(MSIHANDLE)]
msi.MsiViewFetch.restype = UINT
msi.MsiRecordGetStringW.argtypes = [MSIHANDLE, UINT, ctypes.c_wchar_p, ctypes.POINTER(DWORD)]
msi.MsiRecordGetStringW.restype = UINT
msi.MsiRecordReadStream.argtypes = [MSIHANDLE, UINT, ctypes.c_void_p, ctypes.POINTER(DWORD)]
msi.MsiRecordReadStream.restype = UINT
msi.MsiCloseHandle.argtypes = [MSIHANDLE]
msi.MsiCloseHandle.restype = UINT

ERROR_SUCCESS = 0
ERROR_MORE_DATA = 234
ERROR_NO_MORE_ITEMS = 259
MSIDBOPEN_PATCHFILE = 32


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def get_string(record: int, field: int) -> str:
    size = DWORD(0)
    rc = msi.MsiRecordGetStringW(record, field, None, ctypes.byref(size))
    if rc not in (ERROR_SUCCESS, ERROR_MORE_DATA):
        raise OSError(rc, "MsiRecordGetStringW(size)")
    buf = ctypes.create_unicode_buffer(size.value + 1)
    cap = DWORD(len(buf))
    rc = msi.MsiRecordGetStringW(record, field, buf, ctypes.byref(cap))
    if rc != ERROR_SUCCESS:
        raise OSError(rc, "MsiRecordGetStringW(data)")
    return buf.value


def read_stream(record: int, field: int) -> bytes:
    chunks = []
    while True:
        buf = ctypes.create_string_buffer(1024 * 1024)
        size = DWORD(len(buf))
        rc = msi.MsiRecordReadStream(record, field, buf, ctypes.byref(size))
        if rc != ERROR_SUCCESS:
            raise OSError(rc, "MsiRecordReadStream")
        if size.value:
            chunks.append(buf.raw[:size.value])
        if size.value < len(buf):
            break
    return b"".join(chunks)


def safe_name(name: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9._-]+", "_", name).strip("._")
    return cleaned[:100] or "stream"


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: extract_msp_streams.py PATCH.MSP OUT_DIR META.JSON")

    patch = Path(sys.argv[1]).resolve()
    out_dir = Path(sys.argv[2]).resolve()
    meta_path = Path(sys.argv[3]).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    db = MSIHANDLE()
    rc = msi.MsiOpenDatabaseW(str(patch), ctypes.c_void_p(MSIDBOPEN_PATCHFILE), ctypes.byref(db))
    if rc != ERROR_SUCCESS:
        raise OSError(rc, "MsiOpenDatabaseW(MSIDBOPEN_PATCHFILE)")

    rows = []
    view = MSIHANDLE()
    try:
        rc = msi.MsiDatabaseOpenViewW(db, "SELECT `Name`, `Data` FROM `_Streams`", ctypes.byref(view))
        if rc != ERROR_SUCCESS:
            raise OSError(rc, "MsiDatabaseOpenViewW(_Streams)")
        rc = msi.MsiViewExecute(view, 0)
        if rc != ERROR_SUCCESS:
            raise OSError(rc, "MsiViewExecute")

        index = 0
        while True:
            rec = MSIHANDLE()
            rc = msi.MsiViewFetch(view, ctypes.byref(rec))
            if rc == ERROR_NO_MORE_ITEMS:
                break
            if rc != ERROR_SUCCESS:
                raise OSError(rc, "MsiViewFetch")
            try:
                index += 1
                name = get_string(rec, 1)
                data = read_stream(rec, 2)
                magic = data[:16].hex()
                is_cab = data.startswith(b"MSCF")
                suffix = ".cab" if is_cab else ".bin"
                filename = f"{index:03d}_{safe_name(name)}{suffix}"
                path = out_dir / filename
                path.write_bytes(data)
                rows.append({
                    "index": index,
                    "name": name,
                    "filename": filename,
                    "size": len(data),
                    "sha256": sha256_bytes(data),
                    "magic_hex": magic,
                    "is_cab": is_cab,
                })
            finally:
                msi.MsiCloseHandle(rec)
    finally:
        if view.value:
            msi.MsiCloseHandle(view)
        if db.value:
            msi.MsiCloseHandle(db)

    result = {
        "schema": "windows-installer-msp-streams.v1",
        "patch": patch.name,
        "stream_count": len(rows),
        "cab_stream_count": sum(1 for r in rows if r["is_cab"]),
        "streams": rows,
    }
    meta_path.parent.mkdir(parents=True, exist_ok=True)
    meta_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
