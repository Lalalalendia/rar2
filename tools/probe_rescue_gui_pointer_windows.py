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
BATCH_REPORT_SCHEMA = "chaptera.rescue-desktop-batch-report.v1"
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
VK_CONTROL, VK_A = 0x11, 0x41


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
        self.user.GetForegroundWindow.argtypes = ()
        self.user.GetForegroundWindow.restype = wintypes.HWND

    def coordinates(self) -> dict[str, tuple[int, int]]:
        rect = wintypes.RECT()
        if not self.user.GetClientRect(self.hwnd, ctypes.byref(rect)):
            raise OSError(ctypes.get_last_error(), "GetClientRect failed")
        return slot_coordinates(rect.right - rect.left, rect.bottom - rect.top,
                                self.user.GetDpiForWindow(self.hwnd) or 96)

    def _require_foreground(self) -> None:
        # SendInput is a global OS input facility: it does NOT target a HWND.
        # Never inject into a desktop when the actual Rescue top-level window
        # is not foreground (e.g. another app stole focus on hosted Windows).
        if self.user.GetForegroundWindow() != self.hwnd:
            raise RuntimeError("Rescue GUI is not foreground; refusing global SendInput")

    def _focus_target(self) -> None:
        self.user.SetForegroundWindow(self.hwnd)
        deadline = time.monotonic() + 1.0
        while time.monotonic() < deadline:
            if self.user.GetForegroundWindow() == self.hwnd:
                return
            time.sleep(0.02)
        self._require_foreground()

    def _send(self, *events: Input) -> None:
        if not events:
            raise ValueError("Win32 input batch cannot be empty")
        self._require_foreground()
        # One SendInput call preserves event order without interleaving input
        # from another SendInput stream. Always release Ctrl in the same batch.
        buffer = (Input * len(events))(*events)
        sent = self.user.SendInput(len(events), buffer, ctypes.sizeof(Input))
        if sent != len(events):
            raise OSError(ctypes.get_last_error(), "SendInput batch not fully delivered")

    @staticmethod
    def _key(vk: int, up: bool = False) -> Input:
        return Input(type=1, ki=KeyInput(vk, 0, KEYEVENTF_KEYUP if up else 0, 0, 0))

    def combo(self, vk: int) -> None:
        self._send(
            self._key(VK_CONTROL), self._key(vk),
            self._key(vk, up=True), self._key(VK_CONTROL, up=True),
        )

    def unicode(self, value: str) -> None:
        encoded = value.encode("utf-16-le")
        for offset in range(0, len(encoded), 2):
            unit = int.from_bytes(encoded[offset:offset + 2], "little")
            self._send(
                Input(type=1, ki=KeyInput(0, unit, KEYEVENTF_UNICODE, 0, 0)),
                Input(type=1, ki=KeyInput(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0, 0)),
            )
        time.sleep(0.2)

    def click(self, point: tuple[int, int]) -> None:
        screen = wintypes.POINT(*point)
        if not self.user.ClientToScreen(self.hwnd, ctypes.byref(screen)):
            raise OSError(ctypes.get_last_error(), "ClientToScreen failed")
        self._focus_target()
        if not self.user.SetCursorPos(screen.x, screen.y):
            raise OSError(ctypes.get_last_error(), "SetCursorPos failed")
        time.sleep(0.08)
        self._send(
            Input(type=0, mi=MouseInput(0, 0, 0, 0x0002, 0, 0)),
            Input(type=0, mi=MouseInput(0, 0, 0, 0x0004, 0, 0)),
        )
        time.sleep(0.15)

    def set_field(self, point: tuple[int, int], value: str) -> None:
        # No clipboard dependency: the canonical Scan/Run output verifies
        # whether both typed paths actually reached the GUI controls.
        self.click(point)
        self.combo(VK_A)
        self.unicode(value)


def _admitted_row(state: dict, digest: str, expected_status: str) -> dict:
    """Assert exact per-source evidence, not a permissive summary count."""
    if (not isinstance(state, dict)
            or state.get("schema_version") != BATCH_REPORT_SCHEMA
            or state.get("source_free") is not True
            or state.get("unique_sources") != 1
            or state.get("duplicate_paths") != 0
            or state.get("rejected_paths", {}) != {}
            or state.get("counts") != {expected_status: 1}):
        raise AssertionError("canonical synthetic GUI batch summary rejected")
    rows = state.get("entries")
    if not isinstance(rows, list) or len(rows) != 1:
        raise AssertionError("GUI did not retain one exact synthetic source")
    row = rows[0]
    if (not isinstance(row, dict)
            or row.get("sha256") != digest
            or row.get("status") != expected_status):
        raise AssertionError("GUI batch source SHA or expected state mismatch")
    return row


def validate_scanned_source(state: dict, digest: str) -> None:
    row = _admitted_row(state, digest, "pending")
    if row.get("attempts") != 0:
        raise AssertionError("new GUI scan unexpectedly attempted salvage")


def validate_completed_source(state: dict, digest: str) -> None:
    row = _admitted_row(state, digest, "done_partial")
    if (row.get("stream_count") != 2
            or row.get("outcome") != "partial"
            or row.get("reason") is not None
            or not isinstance(row.get("attempts"), int)
            or row["attempts"] < 1):
        raise AssertionError("GUI did not independently verify two SID streams")


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
            native.set_field(fields["source"], str(inputs))
            native.set_field(fields["output"], str(output))
            native.click(fields["scan"])
            scan = wait_summary(output / "summary.json",
                                lambda value: value.get("unique_sources") == 1)
            validate_scanned_source(scan, original_hash)
            # The summary may become visible slightly before the GUI has reaped
            # the scan subprocess and reenabled the Run button.
            time.sleep(1.0)
            native.click(fields["run"])
            report = wait_summary(
                output / "summary.json",
                lambda state: any(row.get("status") == "done_partial"
                                  for row in state.get("entries", [])), limit=35,
            )
            validate_completed_source(report, original_hash)
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
                "python_hidden_from_gui_path": True, "python_absent_on_host": False,
                "clean_windows_10_11": False,
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
