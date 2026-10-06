#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import importlib.util
import json
from pathlib import Path
import shlex
import subprocess
import sys
from typing import NamedTuple, Sequence


MIN_CANDIDATES = 2
MAX_CANDIDATES = 8
MAX_FILES_PER_CANDIDATE = 25
MAX_TOTAL_FILES = 80

ALLOWED_PATTERNS = (
    "apps/chaptera-desktop/src/*.rs",
    "apps/chaptera-desktop/src/**/*.rs",
    "crates/*/src/*.rs",
    "crates/*/src/**/*.rs",
    "crates/*/tests/*.rs",
    "crates/*/tests/**/*.rs",
    "vendor/producer-a/crates/*/src/*.rs",
    "vendor/producer-a/crates/*/src/**/*.rs",
    "vendor/producer-a/crates/*/tests/*.rs",
    "vendor/producer-a/crates/*/tests/**/*.rs",
)

FORBIDDEN_PATTERNS = (
    ".github/**",
    "tools/ci/**",
    "installer/**",
    "deploy/**",
    "packages/product/**",
    "crates/chaptera-update-*/**",
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/src/lib.rs",
)

FORBIDDEN_EXACT = {
    "AGENTS.md",
    "Cargo.toml",
    "Cargo.lock",
    "vendor/producer-a/Cargo.toml",
    "vendor/producer-a/Cargo.lock",
    "apps/chaptera-desktop/src/main.rs",
    "apps/chaptera-desktop/src/render_backend.rs",
    "apps/chaptera-desktop/src/source_font.rs",
}

HEAVY_SCOPES = (
    "reader_windows_smoke",
    "reader_windows",
    "editor_windows",
    "visual_oracle",
    "cloud_reference",
    "virginia_page_role",
    "visual_batch01",
    "typography_golden",
    "android",
    "web",
    "local_portable",
    "installer",
    "path_identity",
    "update_accept",
)


class TrainError(RuntimeError):
    pass


class CandidateSpec(NamedTuple):
    label: str
    ref: str


def run_lines(root: Path, args: Sequence[str]) -> list[str]:
    result = subprocess.run(
        list(args),
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def run_ok(root: Path, args: Sequence[str]) -> bool:
    return subprocess.run(
        list(args),
        cwd=root,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    ).returncode == 0


def resolve_commit(root: Path, ref: str) -> str:
    try:
        lines = run_lines(root, ["git", "rev-parse", "--verify", f"{ref}^{{commit}}"])
    except subprocess.CalledProcessError as exc:
        raise TrainError(f"cannot resolve candidate/base ref {ref!r}") from exc
    if len(lines) != 1:
        raise TrainError(f"ref {ref!r} did not resolve to one commit")
    return lines[0]


def parse_candidate(raw: str) -> CandidateSpec:
    if "=" not in raw:
        raise TrainError(f"candidate must be LABEL=REF, got {raw!r}")
    label, ref = raw.split("=", 1)
    label = label.strip()
    ref = ref.strip()
    if not label or not ref:
        raise TrainError(f"candidate must have non-empty LABEL and REF, got {raw!r}")
    return CandidateSpec(label=label, ref=ref)


def is_allowed_path(path: str) -> bool:
    if path in FORBIDDEN_EXACT:
        return False
    if any(fnmatch.fnmatchcase(path, pattern) for pattern in FORBIDDEN_PATTERNS):
        return False
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in ALLOWED_PATTERNS)


def load_reader_fanout_module():
    module_path = Path(__file__).with_name("ci") / "reader_pr_fanout.py"
    spec = importlib.util.spec_from_file_location("chaptera_reader_pr_fanout", module_path)
    if spec is None or spec.loader is None:
        raise TrainError(f"cannot load canonical Reader fanout classifier from {module_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def heavy_families_for_paths(paths: Sequence[str]) -> list[str]:
    classifier = load_reader_fanout_module()
    scopes = classifier.classify(
        list(paths),
        set(),
        visual_neutral_paths=set(),
    )
    return sorted(scope for scope in HEAVY_SCOPES if scopes.get(scope, False))


def changed_paths(root: Path, base_sha: str, head_sha: str) -> list[str]:
    return sorted(set(run_lines(root, ["git", "diff", "--name-only", f"{base_sha}...{head_sha}"])))


def candidate_commits(root: Path, base_sha: str, head_sha: str) -> list[str]:
    return run_lines(
        root,
        ["git", "rev-list", "--reverse", "--topo-order", f"{base_sha}..{head_sha}"],
    )


def merge_commits(root: Path, base_sha: str, head_sha: str) -> list[str]:
    merge_shas: list[str] = []
    for line in run_lines(root, ["git", "rev-list", "--parents", f"{base_sha}..{head_sha}"]):
        parts = line.split()
        if len(parts) > 2:
            merge_shas.append(parts[0])
    return merge_shas


def plan_train(
    root: Path,
    *,
    base_ref: str,
    candidates: Sequence[CandidateSpec],
    branch_name: str | None = None,
) -> dict:
    root = root.resolve()
    if not MIN_CANDIDATES <= len(candidates) <= MAX_CANDIDATES:
        raise TrainError(
            f"integration train requires {MIN_CANDIDATES}..{MAX_CANDIDATES} candidates; "
            f"got {len(candidates)}"
        )

    labels = [candidate.label for candidate in candidates]
    if len(labels) != len(set(labels)):
        raise TrainError("candidate labels must be unique")

    base_sha = resolve_commit(root, base_ref)
    seen_paths: dict[str, str] = {}
    seen_commits: dict[str, str] = {}
    total_paths: set[str] = set()
    planned_candidates: list[dict] = []
    composition_commits: list[str] = []

    for candidate in candidates:
        head_sha = resolve_commit(root, candidate.ref)
        if head_sha == base_sha:
            raise TrainError(f"{candidate.label}: candidate is identical to base")
        if not run_ok(root, ["git", "merge-base", "--is-ancestor", base_sha, head_sha]):
            raise TrainError(
                f"{candidate.label}: {head_sha} does not descend from train base {base_sha}"
            )

        merges = merge_commits(root, base_sha, head_sha)
        if merges:
            raise TrainError(
                f"{candidate.label}: merge commits are not allowed in V0 train: "
                + ", ".join(merges)
            )

        commits = candidate_commits(root, base_sha, head_sha)
        if not commits:
            raise TrainError(f"{candidate.label}: no unique commits above train base")

        paths = changed_paths(root, base_sha, head_sha)
        if not paths:
            raise TrainError(f"{candidate.label}: no changed paths above train base")
        if len(paths) > MAX_FILES_PER_CANDIDATE:
            raise TrainError(
                f"{candidate.label}: {len(paths)} changed files exceeds "
                f"{MAX_FILES_PER_CANDIDATE} per-candidate limit"
            )

        forbidden = [path for path in paths if not is_allowed_path(path)]
        if forbidden:
            raise TrainError(
                f"{candidate.label}: train-forbidden path(s): " + ", ".join(forbidden)
            )

        commit_overlap = [
            (commit, seen_commits[commit])
            for commit in commits
            if commit in seen_commits
        ]
        if commit_overlap:
            detail = ", ".join(
                f"{commit} already owned by {owner}" for commit, owner in commit_overlap
            )
            raise TrainError(f"{candidate.label}: overlapping commit ancestry: {detail}")

        path_overlap = [
            (path, seen_paths[path])
            for path in paths
            if path in seen_paths
        ]
        if path_overlap:
            detail = ", ".join(
                f"{path} already owned by {owner}" for path, owner in path_overlap
            )
            raise TrainError(f"{candidate.label}: overlapping changed paths: {detail}")

        for commit in commits:
            seen_commits[commit] = candidate.label
            composition_commits.append(commit)
        for path in paths:
            seen_paths[path] = candidate.label
            total_paths.add(path)

        planned_candidates.append(
            {
                "label": candidate.label,
                "ref": candidate.ref,
                "head_sha": head_sha,
                "commits": commits,
                "changed_paths": paths,
                "heavy_families": heavy_families_for_paths(paths),
            }
        )

    if len(total_paths) > MAX_TOTAL_FILES:
        raise TrainError(
            f"train has {len(total_paths)} changed files; limit is {MAX_TOTAL_FILES}"
        )

    heavy_family_members: dict[str, list[str]] = {}
    for candidate in planned_candidates:
        for family in candidate["heavy_families"]:
            heavy_family_members.setdefault(family, []).append(candidate["label"])
    heavy_families = sorted(heavy_family_members)

    train_branch = branch_name or f"integration/train-{base_sha[:10]}"
    commands = [
        shlex.join(["git", "switch", "--detach", base_sha]),
        shlex.join(["git", "switch", "-c", train_branch]),
        shlex.join(["git", "cherry-pick", *composition_commits]),
    ]

    return {
        "schema": "chaptera.integration-train-plan.v1",
        "base_ref": base_ref,
        "base_sha": base_sha,
        "candidate_count": len(planned_candidates),
        "total_changed_files": len(total_paths),
        "candidates": planned_candidates,
        "composition_commits": composition_commits,
        "heavy_families": heavy_families,
        "heavy_family_members": heavy_family_members,
        "train_branch": train_branch,
        "suggested_commands": commands,
        "mutated_repository": False,
    }


def repo_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return Path(result.stdout.strip()).resolve()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate a bounded Chaptera integration train and print a deterministic composition plan."
    )
    parser.add_argument("--base", required=True, help="exact common train base ref/SHA")
    parser.add_argument(
        "--candidate",
        action="append",
        default=[],
        help="candidate in LABEL=REF form; repeat 2..8 times",
    )
    parser.add_argument("--branch-name", help="suggested integration branch name")
    parser.add_argument("--receipt", type=Path, help="optional JSON receipt path")
    args = parser.parse_args()

    try:
        specs = [parse_candidate(raw) for raw in args.candidate]
        plan = plan_train(
            repo_root(),
            base_ref=args.base,
            candidates=specs,
            branch_name=args.branch_name,
        )
    except TrainError as exc:
        print(f"integration train rejected: {exc}", file=sys.stderr)
        return 2

    payload = json.dumps(plan, indent=2, sort_keys=True) + "\n"
    if args.receipt:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(payload, encoding="utf-8")
    print(payload, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
