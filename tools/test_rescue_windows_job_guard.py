#!/usr/bin/env python3
"""Source-free process-fence tests; real job behavior is asserted on Windows."""
from __future__ import annotations

import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from tools import rescue_windows_job_guard as guard
from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_salvage_bundle_v1 as salvage

ROOT = Path(__file__).resolve().parents[1]


class WindowsJobGuardTests(unittest.TestCase):
    def test_explicit_bounded_budget_and_win64_abi_layout(self):
        self.assertEqual(guard.validated_budget(1536), 1536)
        self.assertEqual(ctypes.sizeof(guard.ExtendedLimitInformation), 144)
        for value in (0, 511, 4097, -5, True, 1024.5, "1536"):
            with self.assertRaises(guard.JobGuardUnavailable):
                guard.validated_budget(value)

    def test_batch_guard_fails_before_queue_mutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "sample.pub"
            source.write_bytes(b"PSEUDO-PUB")
            output = root / "output"
            batch.scan([source], output)
            initial = (output / "queue-private.json").read_bytes()
            with patch.object(batch.job_guard, "activate", side_effect=guard.JobGuardUnavailable("job_assignment_failed")):
                with self.assertRaisesRegex(batch.BatchError, "resource_guard_unavailable"):
                    batch.run(output, workers=1)
            self.assertEqual((output / "queue-private.json").read_bytes(), initial)
            self.assertEqual(source.read_bytes(), b"PSEUDO-PUB")

    def test_direct_salvage_rejects_unfenced_worker(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "sample.pub"
            source.write_bytes(b"PSEUDO-PUB")
            output = root / "output"
            with patch.object(salvage.job_guard, "activate", side_effect=guard.JobGuardUnavailable("job_assignment_failed")):
                with self.assertRaisesRegex(salvage.UnsafeInput, "resource_guard_unavailable"):
                    salvage.execute(source, output)
            self.assertFalse(output.exists())
            self.assertEqual(source.read_bytes(), b"PSEUDO-PUB")

    @unittest.skipUnless(os.name == "nt", "Windows Job Object physical probe")
    def test_job_membership_and_parent_death_reaps_child(self):
        with tempfile.TemporaryDirectory() as temporary:
            ready = Path(temporary) / "child.ready"
            # The child proves its own job membership before the owning engine
            # exits without normal cleanup, forcing KILL_ON_JOB_CLOSE.
            child_code = ("import ctypes, pathlib, time; "
                          "k=ctypes.WinDLL('kernel32',use_last_error=True); "
                          "k.GetCurrentProcess.restype=ctypes.c_void_p; "
                          "k.IsProcessInJob.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p]; "
                          "k.IsProcessInJob.restype=ctypes.c_int; "
                          "v=ctypes.c_int(0); "
                          "assert k.IsProcessInJob(k.GetCurrentProcess(),None,ctypes.byref(v)); "
                          f"pathlib.Path({str(ready)!r}).write_text(str(v.value)); "
                          "time.sleep(30)")
            parent_code = ("import json, os, pathlib, subprocess, sys, time; "
                           "from tools import rescue_windows_job_guard as g; "
                           "assert g.activate(512); "
                           f"p=subprocess.Popen([sys.executable,'-c',{child_code!r}],"
                           "stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); "
                           f"f=pathlib.Path({str(ready)!r}); "
                           "t=time.monotonic()+8; "
                           "\nwhile not f.exists() and time.monotonic()<t: time.sleep(.05)\n"
                           "assert f.exists(); "
                           "print(json.dumps({'pid':p.pid}),flush=True); os._exit(0)")
            parent = subprocess.run([sys.executable, "-u", "-c", parent_code],
                                    cwd=ROOT, capture_output=True, timeout=18, text=True)
            self.assertEqual(parent.returncode, 0, parent.stderr)
            self.assertEqual(ready.read_text(), "1")
            pid = json.loads(parent.stdout)["pid"]
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int32, ctypes.c_uint32]
            kernel.OpenProcess.restype = ctypes.c_void_p
            kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
            kernel.WaitForSingleObject.restype = ctypes.c_uint32
            kernel.CloseHandle.argtypes = [ctypes.c_void_p]
            handle = kernel.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
            if handle:
                try:
                    self.assertEqual(kernel.WaitForSingleObject(handle, 5000), 0,
                                     "orphan subprocess was not reaped by job close")
                finally:
                    kernel.CloseHandle(handle)

    @unittest.skipUnless(os.name == "nt", "Windows Job Object physical probe")
    def test_memory_limit_actually_rejects_commits(self):
        child = ("from tools import rescue_windows_job_guard as g; "
                 "assert g.activate(512); "
                 "print('admitted',flush=True); "
                 "blocks=[]; "
                 "\ntry:\n"
                 " for i in range(24): blocks.append(bytearray(32*1024*1024))\n"
                 "except MemoryError: print('denied',flush=True)\n"
                 "else: print('exceeded',flush=True)\n")
        result = subprocess.run([sys.executable, "-u", "-c", child],
                                cwd=ROOT, capture_output=True, timeout=25, text=True)
        self.assertIn("admitted", result.stdout)
        self.assertNotIn("exceeded", result.stdout)
        self.assertTrue("denied" in result.stdout or result.returncode != 0,
                        "the 512 MiB job memory limit did not reject >768 MiB")


if __name__ == "__main__":
    unittest.main()
