#!/usr/bin/env python3
"""Frozen Windows Rescue engine entrypoint; reuses existing batch/salvage modules.

The frozen executable must serve its own bounded ``python -m`` subprocess
calls.  Module names are deliberately allowlisted; no arbitrary interpreter or
network service is exposed. A regular checkout can run this entrypoint too.
"""
from __future__ import annotations

import importlib
import json
import sys

SCHEMA = "chaptera.rescue-portable-engine.v1"
BATCH = "tools.rescue_desktop_batch_v1"
ALLOWED_MODULES = frozenset({
    BATCH,
    "tools.rescue_salvage_bundle_v1",
    "tools.rescue_cfb_stream_export",
    "tools.verify_rescue_cfb_stream_export",
})


def dispatch(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if args == ["--self-check"]:
        try:
            # Even in a frozen executable, prove all requisite modules import.
            for name in sorted(ALLOWED_MODULES):
                importlib.import_module(name)
        except ImportError:
            print(json.dumps({"schema_version": SCHEMA, "ok": False}))
            return 2
        print(json.dumps({"schema_version": SCHEMA, "ok": True,
                          "frozen": bool(getattr(sys, "frozen", False)),
                          "module_count": len(ALLOWED_MODULES)}))
        return 0

    if args[:1] == ["-m"]:
        if len(args) < 2 or args[1] not in ALLOWED_MODULES:
            print(json.dumps({"schema_version": SCHEMA, "error": "unsupported_child_module"}))
            return 2
        module_name, module_args = args[1], args[2:]
    else:
        module_name, module_args = BATCH, args

    module = importlib.import_module(module_name)
    old_argv = sys.argv
    try:
        sys.argv = [module.__file__, *module_args]
        # exporter/verifier main() reads sys.argv; batch and salvage are also
        # compatible with it. This is the same code used outside the package.
        result = module.main()
        return int(result) if result is not None else 0
    finally:
        sys.argv = old_argv


if __name__ == "__main__":
    raise SystemExit(dispatch())
