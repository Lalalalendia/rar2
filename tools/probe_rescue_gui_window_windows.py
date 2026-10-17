#!/usr/bin/env python3
"""Source-free, real-window smoke for an extracted Rescue portable Windows ZIP.

This probe opens the *actual* eframe executable, finds its Win32 window,
closes it, and relaunches it with Python absent from the child's PATH.
It is not a mouse/pointer, clean-machine, or real-corpus acceptance test.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tempfile
import time


WINDOW_TITLE = "Chaptera Rescue"
REQUIRED_EXES = ("chaptera-rescue.exe", "rescue-engine.exe", "pub.exe")
RECEIPT_SCHEMA = "chaptera.rescue-gui-window-smoke.v1"


def validate_package(root: Path) -> dict:
    """Check extracted bytes against the ZIP's source-free build inventory."""
    manifest = json.loads((root / "portable-build.json").read_text(encoding="utf-8-sig"))
    if (manifest.get("schema_version") != "chaptera.rescue-portable-build.v1"
            or manifest.get("signing") != "unsigned_preview"
            or manifest.get("target") != "windows-x64"):
        raise AssertionError("unexpected portable build manifest")
    entries = manifest.get("files")
    if not isinstance(entries, list) or not 3 <= len(entries) <= 20_000:
        raise AssertionError("portable file inventory is missing or unbounded")
    index = {}
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("file"), str):
            raise AssertionError("invalid portable inventory row")
        name = entry["file"]
        relative = PurePosixPath(name)
        if (relative.is_absolute() or not relative.parts or
                any(part in (".", "..") for part in relative.parts) or
                "\\" in name or ":" in name):
            raise AssertionError("unsafe portable inventory path")
        if name in index:
            raise AssertionError("duplicate portable inventory row")
        index[name] = entry
        path = (root / Path(*relative.parts)).resolve()
        if not path.is_relative_to(root.resolve()) or not path.is_file():
            raise AssertionError("portable component absent or outside archive")
        if not isinstance(entry.get("byte_len"), int) or path.stat().st_size != entry["byte_len"]:
            raise AssertionError(f"portable component integrity mismatch: {name}")
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        if digest != entry.get("sha256"):
            raise AssertionError(f"portable component integrity mismatch: {name}")
    if any(name not in index for name in REQUIRED_EXES):
        raise AssertionError("required portable executable is absent")
    if not (root / "_internal").is_dir():
        raise AssertionError("frozen PyInstaller dependencies absent")
    actual = {file.relative_to(root).as_posix() for file in root.rglob("*") if file.is_file()}
    if actual != set(index) | {"portable-build.json"}:
        raise AssertionError("portable archive contents do not match build inventory")
    return manifest


class Desktop:
    """Win32 enumeration restricted to windows owned by the exact launched PID."""

    def __init__(self):
        if os.name != "nt":
            raise RuntimeError("This is a Windows-only physical GUI test")
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        self.callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        self.user.EnumWindows.argtypes = (self.callback, wintypes.LPARAM)
        self.user.EnumWindows.restype = wintypes.BOOL
        self.user.GetWindowThreadProcessId.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.DWORD))
        self.user.GetWindowThreadProcessId.restype = wintypes.DWORD
        self.user.IsWindowVisible.argtypes = (wintypes.HWND,)
        self.user.IsWindowVisible.restype = wintypes.BOOL
        self.user.GetWindowTextLengthW.argtypes = (wintypes.HWND,)
        self.user.GetWindowTextLengthW.restype = ctypes.c_int
        self.user.GetWindowTextW.argtypes = (wintypes.HWND, wintypes.LPWSTR, ctypes.c_int)
        self.user.GetWindowTextW.restype = ctypes.c_int
        self.user.GetClientRect.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.RECT))
        self.user.GetClientRect.restype = wintypes.BOOL
        self.user.PostMessageW.argtypes = (wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)
        self.user.PostMessageW.restype = wintypes.BOOL

    def window_for(self, pid: int) -> int | None:
        found = []

        @self.callback
        def inspect(hwnd, _unused):
            owner = wintypes.DWORD()
            self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value != pid or not self.user.IsWindowVisible(hwnd):
                return True
            length = self.user.GetWindowTextLengthW(hwnd)
            if length <= 0:
                return True
            title = ctypes.create_unicode_buffer(length + 1)
            self.user.GetWindowTextW(hwnd, title, length + 1)
            if WINDOW_TITLE not in title.value:
                return True
            rectangle = wintypes.RECT()
            if not self.user.GetClientRect(hwnd, ctypes.byref(rectangle)):
                return True
            if rectangle.right - rectangle.left >= 450 and rectangle.bottom - rectangle.top >= 250:
                found.append(hwnd)
            return True

        if not self.user.EnumWindows(inspect, 0):
            raise OSError(ctypes.get_last_error(), "EnumWindows failed")
        return found[0] if found else None

    def close(self, hwnd: int):
        if not self.user.PostMessageW(hwnd, 0x0010, 0, 0):  # WM_CLOSE
            raise OSError(ctypes.get_last_error(), "WM_CLOSE was not delivered")


def _bounded_startup_diagnostic(handle, exe: Path) -> str:
    """Report only synthetic-run failure details, without ephemeral user paths."""
    handle.flush()
    handle.seek(0, os.SEEK_END)
    size = handle.tell()
    handle.seek(max(0, size - 4096))
    raw = handle.read(4096).decode("utf-8", errors="replace")
    for location in (str(exe.parent), os.environ.get("GITHUB_WORKSPACE", ""),
                     os.environ.get("USERPROFILE", "")):
        if location:
            raw = raw.replace(location, "<redacted-path>")
    # Keep the CI error compact and source-free; no PUB inputs are launched.
    safe = "".join(char if char.isprintable() or char in "\r\n\t" else "?"
                   for char in raw).strip()
    return safe[-1500:] if safe else "(no process stderr)"


def exercise_gui(exe: Path, env: dict[str, str], desktop: Desktop, *, force: bool):
    with tempfile.TemporaryFile() as diagnostics:
        process = subprocess.Popen([str(exe)], cwd=exe.parent, env=env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=diagnostics)
        try:
            deadline = time.monotonic() + 35
            hwnd = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    reason = _bounded_startup_diagnostic(diagnostics, exe)
                    raise AssertionError(
                        f"Rescue exited before visible GUI, code={process.returncode}, "
                        f"startup_stderr={reason}"
                    )
                hwnd = desktop.window_for(process.pid)
                if hwnd is not None:
                    break
                time.sleep(0.2)
            if hwnd is None:
                raise AssertionError(
                    "Rescue GUI window not visible on Windows runner desktop; "
                    f"running_pid={process.pid}"
                )
            if force:
                process.kill()
            else:
                desktop.close(hwnd)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired as error:
                raise AssertionError("Rescue GUI did not terminate within 15 seconds") from error
            if not force and process.returncode != 0:
                reason = _bounded_startup_diagnostic(diagnostics, exe)
                raise AssertionError(
                    f"WM_CLOSE exited abnormally: {process.returncode}; stderr={reason}"
                )
            deadline = time.monotonic() + 5
            while desktop.window_for(process.pid) is not None and time.monotonic() < deadline:
                time.sleep(0.1)
            if desktop.window_for(process.pid) is not None:
                raise AssertionError("Rescue left a top-level window after shutdown")
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=10)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--portable-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    if os.name != "nt":
        raise SystemExit("physical Rescue GUI smoke requires Windows")
    root = args.portable_dir.resolve(strict=True)
    inventory = validate_package(root)
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PYTHON")}
    env["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")
    desktop = Desktop()
    gui = root / "chaptera-rescue.exe"
    exercise_gui(gui, env, desktop, force=False)
    exercise_gui(gui, env, desktop, force=True)
    print(json.dumps({"schema_version": RECEIPT_SCHEMA,
                      "source_commit": inventory.get("source_commit"),
                      "extracted_zip_verified": True, "python_hidden_from_gui": True,
                      "visible_gui_window": True, "wm_close": True,
                      "restart_after_close": True, "forced_gui_exit": True,
                      "pointer_events_proven": False, "clean_machine": False,
                      "source_free": True}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
