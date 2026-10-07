#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
import pathlib
import subprocess
import sys
from dataclasses import dataclass
from typing import Iterable

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_REGISTRY = ROOT / "tools" / "perf" / "contracts.json"

# Commands are intentionally code-owned. Registry metadata may select only one
# of these IDs; it can never inject an arbitrary shell command.
TRUSTED_COMMANDS: dict[str, tuple[list[str], pathlib.Path, dict[str, str]]] = {
    "layout-projection-work": (
        [
            "cargo",
            "test",
            "-p",
            "pub-layout",
            "--test",
            "projection_index_benchmark",
            "--",
            "--ignored",
            "--nocapture",
        ],
        ROOT / "vendor" / "producer-a",
        {
            "LAYOUT_PROJECTION_INDEX_RECEIPT": str(
                ROOT
                / "vendor"
                / "producer-a"
                / "target"
                / "layout-projection-index"
                / "benchmark.json"
            )
        },
    ),
}


@dataclass(frozen=True)
class Contract:
    contract_id: str
    description: str
    patterns: tuple[str, ...]
    command_id: str


def load_registry(path: pathlib.Path) -> list[Contract]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("version") != 1:
        raise ValueError(f"unsupported perf contract registry version: {payload.get('version')!r}")

    raw_contracts = payload.get("contracts")
    if not isinstance(raw_contracts, dict) or not raw_contracts:
        raise ValueError("perf contract registry must contain at least one contract")

    contracts: list[Contract] = []
    for contract_id, raw in sorted(raw_contracts.items()):
        if not isinstance(raw, dict):
            raise ValueError(f"{contract_id}: contract definition must be an object")
        description = raw.get("description")
        paths = raw.get("paths")
        command_id = raw.get("command_id")
        if not isinstance(description, str) or not description.strip():
            raise ValueError(f"{contract_id}: description is required")
        if not isinstance(paths, list) or not paths or not all(isinstance(x, str) and x for x in paths):
            raise ValueError(f"{contract_id}: non-empty string paths are required")
        if not isinstance(command_id, str) or command_id not in TRUSTED_COMMANDS:
            raise ValueError(f"{contract_id}: unknown trusted command_id {command_id!r}")
        contracts.append(
            Contract(
                contract_id=contract_id,
                description=description,
                patterns=tuple(paths),
                command_id=command_id,
            )
        )
    return contracts


def path_matches(path: str, pattern: str) -> bool:
    path = path.replace("\\", "/")
    pattern = pattern.replace("\\", "/")
    if pattern.endswith("/**"):
        prefix = pattern[:-3].rstrip("/")
        return path == prefix or path.startswith(prefix + "/")
    return fnmatch.fnmatchcase(path, pattern)


def select_contracts(changed_paths: Iterable[str], contracts: Iterable[Contract]) -> list[Contract]:
    normalized = sorted({path.strip().replace("\\", "/") for path in changed_paths if path.strip()})
    selected = []
    for contract in contracts:
        if any(path_matches(path, pattern) for path in normalized for pattern in contract.patterns):
            selected.append(contract)
    return selected


def run_contract(contract: Contract) -> None:
    command, cwd, extra_env = TRUSTED_COMMANDS[contract.command_id]
    env = dict(__import__("os").environ)
    env.update(extra_env)
    print(f"::group::perf contract {contract.contract_id}")
    print(f"description: {contract.description}")
    print(f"cwd: {cwd.relative_to(ROOT)}")
    print("command:", " ".join(command))
    try:
        subprocess.run(command, cwd=cwd, env=env, check=True)
    finally:
        print("::endgroup::")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Select and execute trusted Rust performance contracts.")
    parser.add_argument("--registry", type=pathlib.Path, default=DEFAULT_REGISTRY)
    parser.add_argument("--changed-file", action="append", default=[])
    parser.add_argument("--changed-files-from", type=pathlib.Path)
    parser.add_argument("--run", action="store_true", help="Execute selected trusted contracts.")
    parser.add_argument("--require-selection", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    changed = list(args.changed_file)
    if args.changed_files_from:
        changed.extend(args.changed_files_from.read_text(encoding="utf-8").splitlines())

    contracts = load_registry(args.registry)
    selected = select_contracts(changed, contracts)

    summary = {
        "registry_version": 1,
        "changed_files": sorted({x.strip() for x in changed if x.strip()}),
        "selected_contracts": [contract.contract_id for contract in selected],
    }
    print(json.dumps(summary, indent=2, sort_keys=True))

    if args.require_selection and not selected:
        raise RuntimeError("no performance contract selected for changed paths")

    if args.run:
        for contract in selected:
            run_contract(contract)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
