#!/usr/bin/env python3
"""Fail-closed Windows process-tree memory boundary for local Rescue workers.

The batch engine joins one unnamed Windows Job Object *before* starting any
salvage subprocess. Normal CreateProcess children inherit its restrictions.
KILL_ON_JOB_CLOSE prevents abandoned child workers if the engine exits.
The handle stays open for the engine's entire lifetime: closing it inside an
active engine would also terminate the engine itself.

This is a Windows 10/11 process-commit limit, not a disk/CPU budget, a Linux
resource limit, or proof of native PUB semantic recovery.
"""
from __future__ import annotations

import ctypes
import os
import threading

DEFAULT_MEMORY_MIB = 1536
MIN_MEMORY_MIB = 512
MAX_MEMORY_MIB = 4096
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
JOB_OBJECT_LIMIT_JOB_MEMORY = 0x00000200
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000

_lock = threading.Lock()
_job_handle: int | None = None
_budget_mib: int | None = None


class JobGuardUnavailable(RuntimeError):
    """No unbounded fallback is permitted when Windows fencing fails."""


class BasicLimitInformation(ctypes.Structure):
    _fields_ = [
        ("PerProcessUserTimeLimit", ctypes.c_longlong),
        ("PerJobUserTimeLimit", ctypes.c_longlong),
        ("LimitFlags", ctypes.c_uint32),
        ("MinimumWorkingSetSize", ctypes.c_size_t),
        ("MaximumWorkingSetSize", ctypes.c_size_t),
        ("ActiveProcessLimit", ctypes.c_uint32),
        ("Affinity", ctypes.c_size_t),
        ("PriorityClass", ctypes.c_uint32),
        ("SchedulingClass", ctypes.c_uint32),
    ]


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_ulonglong) for name in (
        "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
        "ReadTransferCount", "WriteTransferCount", "OtherTransferCount",
    )]


class ExtendedLimitInformation(ctypes.Structure):
    _fields_ = [
        ("BasicLimitInformation", BasicLimitInformation),
        ("IoInfo", IoCounters),
        ("ProcessMemoryLimit", ctypes.c_size_t),
        ("JobMemoryLimit", ctypes.c_size_t),
        ("PeakProcessMemoryUsed", ctypes.c_size_t),
        ("PeakJobMemoryUsed", ctypes.c_size_t),
    ]


def validated_budget(memory_mib: int) -> int:
    if type(memory_mib) is not int or not MIN_MEMORY_MIB <= memory_mib <= MAX_MEMORY_MIB:
        raise JobGuardUnavailable("invalid_job_memory_budget")
    return memory_mib


def activate(memory_mib: int = DEFAULT_MEMORY_MIB) -> bool:
    """Join a bounded non-breakaway job before spawning Windows child workers.

    Return False only for non-Windows hosts. On Windows failures, raise before
    starting any subprocess. Multiple calls in one engine retain one budget.
    """
    global _job_handle, _budget_mib
    memory_mib = validated_budget(memory_mib)
    if os.name != "nt":
        return False
    with _lock:
        if _job_handle is not None:
            if _budget_mib != memory_mib:
                raise JobGuardUnavailable("job_memory_budget_already_fixed")
            return True
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p]
        kernel.CreateJobObjectW.restype = ctypes.c_void_p
        kernel.SetInformationJobObject.argtypes = [ctypes.c_void_p, ctypes.c_int,
                                                   ctypes.c_void_p, ctypes.c_uint32]
        kernel.SetInformationJobObject.restype = ctypes.c_int32
        kernel.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        kernel.AssignProcessToJobObject.restype = ctypes.c_int32
        kernel.GetCurrentProcess.argtypes = []
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        kernel.CloseHandle.restype = ctypes.c_int32
        handle = kernel.CreateJobObjectW(None, None)
        if not handle:
            raise JobGuardUnavailable("job_create_failed")
        assigned = False
        try:
            limits = ExtendedLimitInformation()
            limits.BasicLimitInformation.LimitFlags = (
                JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            )
            limits.JobMemoryLimit = memory_mib * 1024 * 1024
            if not kernel.SetInformationJobObject(
                handle, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                ctypes.byref(limits), ctypes.sizeof(limits),
            ):
                raise JobGuardUnavailable("job_limit_failed")
            if not kernel.AssignProcessToJobObject(handle, kernel.GetCurrentProcess()):
                raise JobGuardUnavailable("job_assignment_failed")
            assigned = True
            _job_handle = int(handle)
            _budget_mib = memory_mib
            return True
        finally:
            if not assigned:
                kernel.CloseHandle(handle)
            # NEVER close the last assigned handle here: this job includes us.
