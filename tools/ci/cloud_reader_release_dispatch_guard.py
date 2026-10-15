#!/usr/bin/env python3
"""Reject untrusted or stale Cloud Reader release-dispatch attempts."""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPOSITORY = "Lalalalendia/rar2"
WORKFLOW = "Cloud Reader Linux release packet"
ASSET = "chaptera-cloud-reader-linux-amd64.tar.gz"
SHA = re.compile(r"^[0-9a-f]{40}$")
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")


class ReleaseDispatchRejected(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReleaseDispatchRejected(message)


def valid_asset(release: object) -> bool:
    if not isinstance(release, dict):
        return False
    assets = release.get("assets")
    return isinstance(assets, list) and any(
        isinstance(item, dict)
        and item.get("name") == ASSET
        and isinstance(item.get("size"), int)
        and item["size"] > 0
        and isinstance(item.get("digest"), str)
        and DIGEST.fullmatch(item["digest"]) is not None
        for item in assets
    )


def source_sha(event: dict) -> str:
    run = event.get("workflow_run") or {}
    head_repo = run.get("head_repository") or {}
    require(event.get("action") == "completed", "Not a completed workflow_run event")
    require(run.get("conclusion") == "success", "Upstream release did not succeed")
    require(run.get("event") == "push", "Refusing non-push release completion")
    require(run.get("head_branch") == "main", "Release did not run on main")
    require(run.get("name") == WORKFLOW, "Unexpected upstream workflow owner")
    require(head_repo.get("full_name") == REPOSITORY, "Untrusted release source repo")
    sha = run.get("head_sha", "")
    require(isinstance(sha, str) and SHA.fullmatch(sha) is not None, "Malformed source SHA")
    return sha


def validate_release(
    sha: str, exact_release: dict, latest_releases: list, comparison: dict
) -> str:
    require(SHA.fullmatch(sha) is not None, "Malformed release SHA")
    tag = "cloud-reader-" + sha
    require(exact_release.get("tag_name") == tag, "Release tag mismatches upstream SHA")
    require(exact_release.get("target_commitish") == sha, "Release target mismatches source SHA")
    require(exact_release.get("draft") is False, "Draft is not a published release")
    require(bool(exact_release.get("published_at")), "Release not yet published")
    require(valid_asset(exact_release), "Missing SHA256-digested Linux asset")
    require(isinstance(latest_releases, list), "Missing release collection")
    eligible = [
        item for item in latest_releases
        if isinstance(item, dict)
        and item.get("draft") is False
        and isinstance(item.get("published_at"), str)
        and item["published_at"]
        and isinstance(item.get("tag_name"), str)
        and re.fullmatch(r"cloud-reader-[0-9a-f]{40}", item["tag_name"])
        and valid_asset(item)
    ]
    require(bool(eligible), "No eligible immutable reader releases")
    newest = max(eligible, key=lambda item: item["published_at"])
    require(newest["tag_name"] == tag, "Stale release event would reorder deployment")
    require(
        comparison.get("status") in ("ahead", "identical")
        and comparison.get("behind_by") == 0,
        "Release is not ancestor of current protected main",
    )
    return sha


def validate(event: dict, release: dict, releases: list, comparison: dict) -> str:
    return validate_release(source_sha(event), release, releases, comparison)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--event", type=Path, required=True)
    parser.add_argument("--release", type=Path, required=True)
    parser.add_argument("--releases", type=Path, required=True)
    parser.add_argument("--compare", type=Path, required=True)
    args = parser.parse_args()
    try:
        inputs = [
            json.loads(path.read_text(encoding="utf-8")) for path in
            (args.event, args.release, args.releases, args.compare)
        ]
        sha = validate(*inputs)
    except (OSError, json.JSONDecodeError, ReleaseDispatchRejected) as exc:
        print(f"Release dispatch refused: {exc}", file=sys.stderr)
        return 1
    print(sha)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
