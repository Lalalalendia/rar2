#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import tempfile

MODULE = Path(__file__).with_name("integration_train.py")
spec = importlib.util.spec_from_file_location("integration_train", MODULE)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


def git(root: Path, *args: str, capture: bool = False) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    return result.stdout.strip() if capture else ""


def write(root: Path, path: str, text: str) -> None:
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")


def commit_file(root: Path, path: str, text: str, message: str) -> str:
    write(root, path, text)
    git(root, "add", path)
    git(root, "commit", "-m", message)
    return git(root, "rev-parse", "HEAD", capture=True)


def init_repo() -> tuple[tempfile.TemporaryDirectory, Path, str]:
    temp = tempfile.TemporaryDirectory()
    root = Path(temp.name)
    git(root, "init", "-b", "main")
    git(root, "config", "user.email", "train@example.invalid")
    git(root, "config", "user.name", "Integration Train Test")
    commit_file(root, "README.md", "base\n", "base")
    base = git(root, "rev-parse", "HEAD", capture=True)
    return temp, root, base


def branch_commit(root: Path, base: str, branch: str, path: str, text: str) -> str:
    git(root, "switch", "-C", branch, base)
    return commit_file(root, path, text, branch)


def expect_rejected(fn, needle: str) -> None:
    try:
        fn()
    except mod.TrainError as exc:
        assert needle in str(exc), (needle, str(exc))
    else:
        raise AssertionError(f"expected TrainError containing {needle!r}")


def test_heavy_family_union() -> None:
    paths = ["vendor/producer-a/crates/pub-viewer/src/feature.rs"]
    families = mod.heavy_families_for_paths(paths)
    jobs = mod.effective_heavy_jobs_for_paths(paths)

    assert "visual_oracle" in families
    assert "reader_windows_smoke" in families
    assert "typography_golden" in families
    assert "android_core" in families

    assert "visual_oracle" in jobs
    assert "android_core" in jobs
    assert "reader_windows_smoke" not in jobs
    assert "typography_golden" not in jobs


def test_unrelated_feature_has_no_heavy_family() -> None:
    families = mod.heavy_families_for_paths([
        "vendor/producer-a/crates/pub-editor/src/paragraph_alignment_v1.rs",
    ])
    assert families == []


def test_positive_disjoint_plan() -> None:
    temp, root, base = init_repo()
    try:
        a = branch_commit(root, base, "task-a", "crates/a/src/feature_a.rs", "pub fn a() {}\n")
        b = branch_commit(root, base, "task-b", "vendor/producer-a/crates/pub-editor/src/feature_b.rs", "pub fn b() {}\n")
        plan = mod.plan_train(
            root,
            base_ref=base,
            candidates=[
                mod.CandidateSpec("TASK-A", "task-a"),
                mod.CandidateSpec("TASK-B", "task-b"),
            ],
            branch_name="integration/test",
        )
        assert plan["composition_commits"] == [a, b]
        assert plan["candidate_count"] == 2
        assert plan["total_changed_files"] == 2
        assert plan["mutated_repository"] is False
        assert plan["heavy_families"] == []
        assert plan["heavy_family_members"] == {}
        assert plan["effective_heavy_jobs"] == []
        assert plan["effective_heavy_job_members"] == {}
        assert plan["suggested_commands"][-1].startswith("git cherry-pick ")
    finally:
        temp.cleanup()


def test_verify_composed_head() -> None:
    temp, root, base = init_repo()
    try:
        a = branch_commit(root, base, "task-a", "crates/a/src/feature_a.rs", "pub fn a() {}\n")
        b = branch_commit(root, base, "task-b", "vendor/producer-a/crates/pub-editor/src/feature_b.rs", "pub fn b() {}\n")
        plan = mod.plan_train(
            root,
            base_ref=base,
            candidates=[
                mod.CandidateSpec("TASK-A", a),
                mod.CandidateSpec("TASK-B", b),
            ],
        )
        git(root, "switch", "-C", "integration/test", base)
        git(root, "cherry-pick", *plan["composition_commits"])
        verification = mod.verify_composed_head(root, plan, "integration/test")
        assert verification["verified"] is True
        assert verification["changed_paths"] == [
            "crates/a/src/feature_a.rs",
            "vendor/producer-a/crates/pub-editor/src/feature_b.rs",
        ]
        assert verification["heavy_families"] == plan["heavy_families"]
        assert verification["effective_heavy_jobs"] == plan["effective_heavy_jobs"]
    finally:
        temp.cleanup()


def test_verify_composed_head_rejects_extra_path() -> None:
    temp, root, base = init_repo()
    try:
        a = branch_commit(root, base, "task-a", "crates/a/src/feature_a.rs", "pub fn a() {}\n")
        b = branch_commit(root, base, "task-b", "crates/b/src/feature_b.rs", "pub fn b() {}\n")
        plan = mod.plan_train(
            root,
            base_ref=base,
            candidates=[
                mod.CandidateSpec("TASK-A", a),
                mod.CandidateSpec("TASK-B", b),
            ],
        )
        git(root, "switch", "-C", "integration/test", base)
        git(root, "cherry-pick", *plan["composition_commits"])
        commit_file(root, "crates/extra/src/unplanned.rs", "pub fn extra() {}\n", "unplanned")
        expect_rejected(
            lambda: mod.verify_composed_head(root, plan, "integration/test"),
            "path set does not match plan",
        )
    finally:
        temp.cleanup()


def test_overlapping_path_rejected() -> None:
    temp, root, base = init_repo()
    try:
        branch_commit(root, base, "task-a", "crates/shared/src/feature.rs", "pub fn v() { let _ = 1; }\n")
        branch_commit(root, base, "task-b", "crates/shared/src/feature.rs", "pub fn v() { let _ = 2; }\n")
        expect_rejected(
            lambda: mod.plan_train(
                root,
                base_ref=base,
                candidates=[
                    mod.CandidateSpec("A", "task-a"),
                    mod.CandidateSpec("B", "task-b"),
                ],
            ),
            "overlapping changed paths",
        )
    finally:
        temp.cleanup()


def test_forbidden_path_rejected() -> None:
    temp, root, base = init_repo()
    try:
        branch_commit(root, base, "task-a", ".github/workflows/x.yml", "name: x\n")
        branch_commit(root, base, "task-b", "crates/b/src/feature.rs", "pub fn b() {}\n")
        expect_rejected(
            lambda: mod.plan_train(
                root,
                base_ref=base,
                candidates=[
                    mod.CandidateSpec("A", "task-a"),
                    mod.CandidateSpec("B", "task-b"),
                ],
            ),
            "train-forbidden path",
        )
    finally:
        temp.cleanup()


def test_non_descendant_rejected() -> None:
    temp, root, base = init_repo()
    try:
        a = branch_commit(root, base, "task-a", "crates/a/src/feature.rs", "pub fn a() {}\n")
        branch_commit(root, base, "task-b", "crates/b/src/feature.rs", "pub fn b() {}\n")
        branch_commit(root, base, "task-c", "crates/c/src/feature.rs", "pub fn c() {}\n")
        expect_rejected(
            lambda: mod.plan_train(
                root,
                base_ref=a,
                candidates=[
                    mod.CandidateSpec("B", "task-b"),
                    mod.CandidateSpec("C", "task-c"),
                ],
            ),
            "does not descend from train base",
        )
    finally:
        temp.cleanup()


def test_overlapping_commit_ancestry_rejected() -> None:
    temp, root, base = init_repo()
    try:
        a = branch_commit(root, base, "task-a", "crates/a/src/feature.rs", "pub fn a() {}\n")
        git(root, "switch", "-C", "task-b", a)
        commit_file(root, "crates/b/src/feature.rs", "pub fn b() {}\n", "task-b")
        expect_rejected(
            lambda: mod.plan_train(
                root,
                base_ref=base,
                candidates=[
                    mod.CandidateSpec("A", "task-a"),
                    mod.CandidateSpec("B", "task-b"),
                ],
            ),
            "overlapping commit ancestry",
        )
    finally:
        temp.cleanup()


def test_merge_commit_rejected() -> None:
    temp, root, base = init_repo()
    try:
        branch_commit(root, base, "task-a", "crates/a/src/feature.rs", "pub fn a() {}\n")
        git(root, "switch", "-C", "side", base)
        commit_file(root, "crates/side/src/feature.rs", "pub fn side() {}\n", "side")
        git(root, "switch", "task-a")
        git(root, "merge", "--no-ff", "side", "-m", "merge side")
        branch_commit(root, base, "task-b", "crates/b/src/feature.rs", "pub fn b() {}\n")
        expect_rejected(
            lambda: mod.plan_train(
                root,
                base_ref=base,
                candidates=[
                    mod.CandidateSpec("A", "task-a"),
                    mod.CandidateSpec("B", "task-b"),
                ],
            ),
            "merge commits are not allowed",
        )
    finally:
        temp.cleanup()


def test_file_limits_rejected() -> None:
    temp, root, base = init_repo()
    try:
        git(root, "switch", "-C", "task-a", base)
        for index in range(mod.MAX_FILES_PER_CANDIDATE + 1):
            write(root, f"crates/a/src/f{index}.rs", f"pub fn f{index}() {{}}\n")
        git(root, "add", ".")
        git(root, "commit", "-m", "too many")
        branch_commit(root, base, "task-b", "crates/b/src/feature.rs", "pub fn b() {}\n")
        expect_rejected(
            lambda: mod.plan_train(
                root,
                base_ref=base,
                candidates=[
                    mod.CandidateSpec("A", "task-a"),
                    mod.CandidateSpec("B", "task-b"),
                ],
            ),
            "per-candidate limit",
        )
    finally:
        temp.cleanup()


def test_total_file_limit_rejected() -> None:
    temp, root, base = init_repo()
    try:
        specs = []
        for candidate_index in range(4):
            branch = f"task-{candidate_index}"
            git(root, "switch", "-C", branch, base)
            for file_index in range(21):
                write(
                    root,
                    f"crates/c{candidate_index}/src/f{file_index}.rs",
                    f"pub fn f{candidate_index}_{file_index}() {{}}\n",
                )
            git(root, "add", ".")
            git(root, "commit", "-m", branch)
            specs.append(mod.CandidateSpec(branch, branch))
        expect_rejected(
            lambda: mod.plan_train(root, base_ref=base, candidates=specs),
            "changed files; limit is",
        )
    finally:
        temp.cleanup()


def main() -> None:
    test_heavy_family_union()
    test_unrelated_feature_has_no_heavy_family()
    test_positive_disjoint_plan()
    test_verify_composed_head()
    test_verify_composed_head_rejects_extra_path()
    test_overlapping_path_rejected()
    test_forbidden_path_rejected()
    test_non_descendant_rejected()
    test_overlapping_commit_ancestry_rejected()
    test_merge_commit_rejected()
    test_file_limits_rejected()
    test_total_file_limit_rejected()
    print("integration train tests: ok")


if __name__ == "__main__":
    main()
