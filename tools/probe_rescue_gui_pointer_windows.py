#!/usr/bin/env python3
"""Real OS mouse/keyboard Scan/Run smoke for extracted Rescue Windows ZIP.

Only synthetically generated PUB is used. No GUI callbacks are mocked. This
does not prove clean Windows 10/11, drag/drop or cancellation of active workers.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

SCHEMA = "chaptera.rescue-gui-pointer-scan.v1"
CF_UNICODETEXT = 13
GMEM_MOVEABLE = 0x0002
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
VK_CONTROL, VK_A, VK_C = 0x11, 0x41, 0x43


class KeyInput(ctypes.Structure):
    _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD),
                ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_size_t)]


class MouseInput(ctypes.Structure):
    _fields_ = [("dx", wintypes.LONG), ("dy", wintypes.LONG),
                ("mouseData", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class HardwareInput(ctypes.Structure):
    _fields_ = [("uMsg", wintypes.DWORD), ("wParamL", wintypes.WORD),
                ("wParamH", wintypes.WORD)]


class InputUnion(ctypes.Union):
    _fields_ = [("ki", KeyInput), ("mi", MouseInput), ("hi", HardwareInput)]


class Input(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", wintypes.DWORD), ("u", InputUnion)]


def slot_coordinates(width: int, height: int, dpi: int) -> dict[str, tuple[int, int]]:
    """Bounded native event targets for Rescue's 300-point egui bottom panel."""
    if not (640 <= width <= 8192 and 550 <= height <= 8192 and 72 <= dpi <= 480):
        raise ValueError("hosted window dimensions/DPI outside GUI probe envelope")
    scale = dpi / 96
    points = {
        "source": (round(270 * scale), height - round(216 * scale)),
        "output": (round(270 * scale), height - round(189 * scale)),
        "scan": (round(88 * scale), height - round(75 * scale)),
        "run": (round(214 * scale), height - round(75 * scale)),
    }
    if any(not (0 < x < width and 0 < y < height) for x, y in points.values()):
        raise ValueError("GUI controls out of visible client rectangle")
    return points


class OsInput:
    def __init__(self, hwnd: int):
        if os.name != "nt":
            raise RuntimeError("physical pointer test requires Windows")
        self.hwnd = hwnd
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.user.SendInput.argtypes = (wintypes.UINT, ctypes.POINTER(Input), ctypes.c_int)
        self.user.SendInput.restype = wintypes.UINT
        self.user.GetClientRect.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.RECT))
        self.user.GetClientRect.restype = wintypes.BOOL
        self.user.ClientToScreen.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.POINT))
        self.user.ClientToScreen.restype = wintypes.BOOL
        self.user.GetDpiForWindow.argtypes = (wintypes.HWND,)
        self.user.GetDpiForWindow.restype = wintypes.UINT
        self.user.SetCursorPos.argtypes = (ctypes.c_int, ctypes.c_int)
        self.user.SetCursorPos.restype = wintypes.BOOL
        self.user.SetForegroundWindow.argtypes = (wintypes.HWND,)
        self.user.SetForegroundWindow.restype = wintypes.BOOL
        self.user.OpenClipboard.argtypes = (wintypes.HWND,)
        self.user.OpenClipboard.restype = wintypes.BOOL
        self.user.CloseClipboard.argtypes = ()
        self.user.CloseClipboard.restype = wintypes.BOOL
        self.user.EmptyClipboard.argtypes = ()
        self.user.EmptyClipboard.restype = wintypes.BOOL
        self.user.SetClipboardData.argtypes = (wintypes.UINT, ctypes.c_void_p)
        self.user.SetClipboardData.restype = ctypes.c_void_p
        self.user.GetClipboardData.argtypes = (wintypes.UINT,)
        self.user.GetClipboardData.restype = ctypes.c_void_p
        self.kernel.GlobalAlloc.argtypes = (wintypes.UINT, ctypes.c_size_t)
        self.kernel.GlobalAlloc.restype = ctypes.c_void_p
        self.kernel.GlobalFree.argtypes = (ctypes.c_void_p,)
        self.kernel.GlobalFree.restype = ctypes.c_void_p
        self.kernel.GlobalLock.argtypes = (ctypes.c_void_p,)
        self.kernel.GlobalLock.restype = ctypes.c_void_p
        self.kernel.GlobalUnlock.argtypes = (ctypes.c_void_p,)
        self.kernel.GlobalUnlock.restype = wintypes.BOOL

    def coordinates(self) -> dict[str, tuple[int, int]]:
        rect = wintypes.RECT()
        if not self.user.GetClientRect(self.hwnd, ctypes.byref(rect)):
            raise OSError(ctypes.get_last_error(), "GetClientRect failed")
        return slot_coordinates(rect.right - rect.left, rect.bottom - rect.top,
                                self.user.GetDpiForWindow(self.hwnd) or 96)

    def _send(self, event: Input) -> None:
        if self.user.SendInput(1, ctypes.byref(event), ctypes.sizeof(Input)) != 1:
            raise OSError(ctypes.get_last_error(), "SendInput not delivered")

    def key(self, vk: int, up: bool = False) -> None:
        self._send(Input(type=1, ki=KeyInput(vk, 0, KEYEVENTF_KEYUP if up else 0, 0, 0)))

    def combo(self, vk: int) -> None:
        self.key(VK_CONTROL)
        self.key(vk)
        self.key(vk, up=True)
        self.key(VK_CONTROL, up=True)

    def unicode(self, value: str) -> None:
        encoded = value.encode("utf-16-le")
        for offset in range(0, len(encoded), 2):
            unit = int.from_bytes(encoded[offset:offset + 2], "little")
            self._send(Input(type=1, ki=KeyInput(0, unit, KEYEVENTF_UNICODE, 0, 0)))
            self._send(Input(type=1, ki=KeyInput(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0, 0)))
        time.sleep(0.2)

    def click(self, point: tuple[int, int]) -> None:
        screen = wintypes.POINT(*point)
        if not self.user.ClientToScreen(self.hwnd, ctypes.byref(screen)):
            raise OSError(ctypes.get_last_error(), "ClientToScreen failed")
        self.user.SetForegroundWindow(self.hwnd)
        if not self.user.SetCursorPos(screen.x, screen.y):
            raise OSError(ctypes.get_last_error(), "SetCursorPos failed")
        time.sleep(0.08)
        self._send(Input(type=0, mi=MouseInput(0, 0, 0, 0x0002, 0, 0)))
        time.sleep(0.06)
        self._send(Input(type=0, mi=MouseInput(0, 0, 0, 0x0004, 0, 0)))
        time.sleep(0.15)

    def clipboard(self, value: str | None = None) -> str:
        if not self.user.OpenClipboard(self.hwnd):
            raise OSError(ctypes.get_last_error(), "OpenClipboard failed")
        try:
            if value is not None:
                self.user.EmptyClipboard()
                payload = value.encode("utf-16-le") + b"\x00\x00"
                mem = self.kernel.GlobalAlloc(GMEM_MOVEABLE, len(payload))
                if not mem:
                    raise OSError(ctypes.get_last_error(), "GlobalAlloc failed")
                ptr = self.kernel.GlobalLock(mem)
                if not ptr:
                    self.kernel.GlobalFree(mem)
                    raise OSError(ctypes.get_last_error(), "GlobalLock failed")
                ctypes.memmove(ptr, payload, len(payload))
                self.kernel.GlobalUnlock(mem)
                if not self.user.SetClipboardData(CF_UNICODETEXT, mem):
                    self.kernel.GlobalFree(mem)
                    raise OSError(ctypes.get_last_error(), "SetClipboardData failed")
                return value
            handle = self.user.GetClipboardData(CF_UNICODETEXT)
            if not handle:
                return ""
            locked = self.kernel.GlobalLock(handle)
            if not locked:
                return ""
            try:
                return ctypes.wstring_at(locked)
            finally:
                self.kernel.GlobalUnlock(handle)
        finally:
            self.user.CloseClipboard()

    def set_field(self, point: tuple[int, int], value: str) -> bool:
        self.click(point)
        self.combo(VK_A)
        self.unicode(value)
        return self.field_equals(point, value)

    def field_equals(self, point: tuple[int, int], value: str) -> bool:
        self.click(point)
        self.clipboard("SYNTHETIC_TEST_NO_SELECTION")
        self.combo(VK_A)
        self.combo(VK_C)
        time.sleep(0.15)
        # Never log clipboard contents: only compare against a generated path.
        return self.clipboard() == value


def wait_summary(path: Path, predicate, limit: float = 25) -> dict:
    deadline = time.monotonic() + limit
    while time.monotonic() < deadline:
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
            if predicate(value):
                return value
        except (OSError, ValueError):
            pass
        time.sleep(0.2)
    raise AssertionError("GUI pointer action did not produce the canonical batch summary")


def main() -> int:
    if os.name != "nt":
        raise SystemExit("Windows native pointer acceptance only")
    from tools.probe_rescue_gui_window_windows import Desktop, validate_package
    from tools.test_rescue_cfb_stream_identity import collision_fixture

    parser = argparse.ArgumentParser()
    parser.add_argument("--portable-dir", required=True, type=Path)
    args = parser.parse_args()
    root = args.portable_dir.resolve(strict=True)
    inventory = validate_package(root)
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PYTHON")}
    env["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")
    desktop = Desktop()
    with tempfile.TemporaryDirectory(prefix="rescue-gui-pointer-") as temporary:
        tmp = Path(temporary)
        inputs = tmp / "synthetic-input"
        inputs.mkdir()
        doc = inputs / "example.pub"
        source = collision_fixture()
        doc.write_bytes(source)
        original_hash = hashlib.sha256(source).hexdigest()
        output = tmp / "output"
        process = subprocess.Popen([str(root / "chaptera-rescue.exe")], cwd=root, env=env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 35
            hwnd = None
            while time.monotonic() < deadline and process.poll() is None:
                hwnd = desktop.window_for(process.pid)
                if hwnd is not None:
                    break
                time.sleep(0.2)
            if hwnd is None:
                raise AssertionError("real GUI window unavailable for OS pointer acceptance")
            native = OsInput(hwnd)
            fields = native.coordinates()
            if not native.set_field(fields["source"], str(inputs)):
                raise AssertionError("real pointer/keyboard did not set source folder")
            if not native.set_field(fields["output"], str(output)):
                raise AssertionError("real pointer/keyboard did not set output folder")
            if not native.field_equals(fields["source"], str(inputs)):
                raise AssertionError("output edit corrupted source field or selected wrong widget")
            native.click(fields["scan"])
            scan = wait_summary(output / "summary.json",
                                lambda value: value.get("unique_sources") == 1)
            if scan.get("source_free") is not True or scan.get("duplicate_paths") != 0:
                raise AssertionError("GUI scan does not preserve source-free queue")
            # The summary may become visible slightly before the GUI has reaped
            # the scan subprocess and reenabled the Run button.
            time.sleep(1.0)
            native.click(fields["run"])
            report = wait_summary(
                output / "summary.json",
                lambda state: any(row.get("status") == "done_partial"
                                  for row in state.get("entries", [])), limit=35,
            )
            rows = report.get("entries", [])
            if len(rows) != 1 or rows[0].get("sha256") != original_hash:
                raise AssertionError("GUI run changed the exact source SHA identity")
            if rows[0].get("stream_count") != 2:
                raise AssertionError("GUI run did not verify two physical SID streams")
            if hashlib.sha256(doc.read_bytes()).hexdigest() != original_hash:
                raise AssertionError("GUI batch modified source bytes")
            desktop.close(hwnd)
            process.wait(timeout=15)
            if process.returncode != 0:
                raise AssertionError("GUI did not close normally after Scan/Run")
            print(json.dumps({
                "schema_version": SCHEMA, "source_commit": inventory.get("source_commit"),
                "physical_windows_mouse_keyboard": True, "source_and_output_fields": True,
                "scan_button": True, "run_button": True, "synthetic_sources": 1,
                "exact_verified_sid_streams": 2, "source_unchanged": True,
                "no_system_python": True, "clean_windows_10_11": False,
                "active_workers_cancel_proven": False, "drag_drop_proven": False,
                "source_free": True,
            }, sort_keys=True))
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=10)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
