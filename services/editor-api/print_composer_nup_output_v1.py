"""Publish a bounded nine-up PDF/PNG/receipt bundle without clobbering files.

The producer must validate every output before calling this helper. Exclusive
creation plus best-effort rollback protects pre-existing and immutable sources.
This is not a cross-file atomic transaction against power loss.
"""
from __future__ import annotations

import os
from pathlib import Path
import stat
from typing import Sequence


class NupOutputPublishError(ValueError):
    """Path admission failure with a stable, source-safe error code."""


def publish_nup_outputs_v1(
    *, inputs: Sequence[Path], outputs: Sequence[tuple[Path, bytes]]
) -> None:
    """Exclusively create all outputs; rollback our own files on I/O failure.

    Does not emit file names/paths or payload bytes to public error reports.
    No payload is changed. All caller inputs must already have been snapshotted.
    """
    if not (1 <= len(outputs) <= 8):
        raise NupOutputPublishError("output_count_invalid")
    paths = [*(Path(p) for p in inputs), *(Path(p) for p, _ in outputs)]
    if len({p.resolve() for p in paths}) != len(paths):
        raise NupOutputPublishError("input_output_path_collision")
    for path, data in outputs:
        if not isinstance(data, bytes):
            raise NupOutputPublishError("output_payload_invalid")
        if not path.parent.is_dir():
            raise NupOutputPublishError("output_parent_unavailable")
        if path.exists() or path.is_symlink():
            raise NupOutputPublishError("output_exists")

    created: list[tuple[Path, int, int]] = []
    try:
        for path, payload in outputs:
            # 'xb' is O_EXCL; an existing file/symlink (even in a race)
            # will not be truncated or followed. No non-exclusive 'wb'.
            with path.open("xb") as stream:
                info = os.fstat(stream.fileno())
                if not stat.S_ISREG(info.st_mode):
                    raise NupOutputPublishError("output_not_regular")
                created.append((path, info.st_dev, info.st_ino))
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
    except BaseException:
        # Only remove files that this exact attempt created. Never delete
        # a replacement placed at the same pathname by another process.
        for path, device, inode in reversed(created):
            try:
                current = path.lstat()
                if (stat.S_ISREG(current.st_mode) and current.st_dev == device
                        and current.st_ino == inode):
                    path.unlink()
            except OSError:
                pass
        raise
