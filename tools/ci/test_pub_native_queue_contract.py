#!/usr/bin/env python3
"""Check lossless serialized Publisher-native Actions job admission.

GitHub concurrency's default queue is single and replaces the previous
pending job. `queue: max` retains bounded pending work without
allowing simultaneous Publisher jobs in the shared group.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GROUP = "pub-re-native-publisher-oracle"
EXPECTED_OWNERS = frozenset(
    {
        "pub-native-fixed-footprint-control.yml",
        "pub-native-length-preserving-story.yml",
        "pub-native-one-unit-delete-control.yml",
        "pub-native-saveas-source-control.yml",
        "pub-native-story-pwsh7-transport.yml",
        "pub-native-story-save-copy.yml",
        "pub-native-syid-header-control.yml",
        "pub-re-native.yml",
        "pub-save-roundtrip-handoff.yml",
    }
)


def check_blocks(source: str) -> tuple[int, list[str]]:
    """Return count and violations for this exact shared concurrency group."""
    lines = source.splitlines()
    errors: list[str] = []
    count = 0
    for i, line in enumerate(lines):
        match = re.fullmatch(r"(\s+)group:\s*" + re.escape(GROUP) + r"\s*", line)
        if not match:
            continue
        count += 1
        indent = len(match.group(1))
        settings: dict[str, str] = {}
        for follower in lines[i + 1 :]:
            if not follower.strip() or follower.lstrip().startswith("#"):
                continue
            n = len(follower) - len(follower.lstrip())
            if n != indent:
                break
            name, sep, value = follower.strip().partition(":")
            if not sep or name not in {"queue", "cancel-in-progress"}:
                break
            settings[name] = value.strip()
        if settings.get("queue") != "max" or settings.get("cancel-in-progress") != "false":
            errors.append(
                f"line {i + 1}: shared Publisher lock must have "
                "queue: max and cancel-in-progress: false"
            )
    return count, errors


class PublisherQueueContractTests(unittest.TestCase):
    def test_every_shared_native_job_is_non_lossy_and_exclusive(self) -> None:
        owners: set[str] = set()
        total_blocks = 0
        errors: list[str] = []
        for path in sorted((ROOT / ".github/workflows").glob("*.yml")):
            count, violations = check_blocks(path.read_text(encoding="utf-8"))
            if count:
                owners.add(path.name)
                total_blocks += count
            errors.extend(f"{path.name}: {message}" for message in violations)
        self.assertFalse(errors, "\n".join(errors))
        self.assertTrue(EXPECTED_OWNERS.issubset(owners))
        self.assertGreaterEqual(total_blocks, 10)

    def test_default_single_pending_is_rejected(self) -> None:
        source = "    concurrency:\n      group: " + GROUP + "\n      cancel-in-progress: false\n"
        self.assertEqual(check_blocks(source)[0], 1)
        self.assertEqual(len(check_blocks(source)[1]), 1)

    def test_running_job_cancellation_is_rejected(self) -> None:
        source = (
            "    concurrency:\n      group: "
            + GROUP
            + "\n      queue: max\n      cancel-in-progress: true\n"
        )
        self.assertEqual(len(check_blocks(source)[1]), 1)

    def test_fifo_pending_serial_lock_is_accepted(self) -> None:
        source = (
            "    concurrency:\n      group: "
            + GROUP
            + "\n      queue: max\n      cancel-in-progress: false\n"
        )
        self.assertEqual(check_blocks(source), (1, []))


if __name__ == "__main__":
    unittest.main()
