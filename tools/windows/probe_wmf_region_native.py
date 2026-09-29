#!/usr/bin/env python3
import ctypes
import struct

gdi = ctypes.WinDLL("gdi32", use_last_error=True)

gdi.CreateMetaFileW.argtypes = [ctypes.c_wchar_p]
gdi.CreateMetaFileW.restype = ctypes.c_void_p
gdi.CreateRectRgn.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int]
gdi.CreateRectRgn.restype = ctypes.c_void_p
gdi.SelectClipRgn.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
gdi.SelectClipRgn.restype = ctypes.c_int
gdi.IntersectClipRect.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int]
gdi.IntersectClipRect.restype = ctypes.c_int
gdi.Rectangle.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int]
gdi.Rectangle.restype = ctypes.c_int
gdi.DeleteObject.argtypes = [ctypes.c_void_p]
gdi.DeleteObject.restype = ctypes.c_int
gdi.CloseMetaFile.argtypes = [ctypes.c_void_p]
gdi.CloseMetaFile.restype = ctypes.c_void_p
gdi.GetMetaFileBitsEx.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p]
gdi.GetMetaFileBitsEx.restype = ctypes.c_uint
gdi.DeleteMetaFile.argtypes = [ctypes.c_void_p]
gdi.DeleteMetaFile.restype = ctypes.c_int

hdc = gdi.CreateMetaFileW(None)
if not hdc:
    raise ctypes.WinError(ctypes.get_last_error())
hrgn = gdi.CreateRectRgn(10, 20, 30, 40)
if not hrgn:
    raise ctypes.WinError(ctypes.get_last_error())

select_result = gdi.SelectClipRgn(hdc, hrgn)
intersect_result = gdi.IntersectClipRect(hdc, 12, 22, 28, 38)
gdi.Rectangle(hdc, 0, 0, 100, 100)
gdi.DeleteObject(hrgn)

hmf = gdi.CloseMetaFile(hdc)
if not hmf:
    raise ctypes.WinError(ctypes.get_last_error())
try:
    size = gdi.GetMetaFileBitsEx(hmf, 0, None)
    if not size:
        raise ctypes.WinError(ctypes.get_last_error())
    buf = ctypes.create_string_buffer(size)
    got = gdi.GetMetaFileBitsEx(hmf, size, buf)
    if got != size:
        raise RuntimeError(f"short GetMetaFileBitsEx: {got}/{size}")
    raw = buf.raw[:got]
finally:
    gdi.DeleteMetaFile(hmf)

def u16(buf, off):
    return struct.unpack_from("<H", buf, off)[0]

def i16(buf, off):
    return struct.unpack_from("<h", buf, off)[0]

def u32(buf, off):
    return struct.unpack_from("<I", buf, off)[0]

offset = 18
records = []
region = None
while offset + 6 <= len(raw):
    words = u32(raw, offset)
    if words < 3:
        raise RuntimeError("bad record size")
    end = offset + words * 2
    if end > len(raw):
        raise RuntimeError("record OOB")
    fn = u16(raw, offset + 4)
    params = raw[offset + 6:end]
    records.append(f"0x{fn:04x}")
    if fn == 0x06FF:
        region = params
    offset = end
    if fn == 0:
        break

receipt = {
    "select_clip_result": select_result,
    "intersect_clip_result": intersect_result,
    "record_functions": records,
    "region_present": region is not None,
}
if region is not None:
    cursor = 22
    scan_count = i16(region, 10) if len(region) >= 12 else -1
    scan_ok = scan_count >= 0 and len(region) >= 22
    parsed = 0
    if scan_ok:
        for _ in range(scan_count):
            if cursor + 8 > len(region):
                scan_ok = False
                break
            count = u16(region, cursor)
            end = cursor + 8 + count * 2
            if count % 2 or end > len(region) or u16(region, cursor + 6 + count * 2) != count:
                scan_ok = False
                break
            cursor = end
            parsed += 1
    receipt.update({
        "region_payload_bytes": len(region),
        "region_size": i16(region, 8) if len(region) >= 10 else None,
        "scan_count": scan_count,
        "max_scan": i16(region, 12) if len(region) >= 14 else None,
        "parsed_scan_count": parsed,
        "declared_scans_valid": scan_ok,
        "bytes_after_declared_scans": len(region) - cursor if scan_ok else None,
    })

import json
print(json.dumps(receipt, indent=2, sort_keys=True))
