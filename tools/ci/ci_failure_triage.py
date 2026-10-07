#!/usr/bin/env python3
"""Classify GitHub Actions failures before an agent edits product code.

The tool is deliberately read-only. Given a workflow run id it reads the run,
failed jobs, failed job logs, and PR changed files, then emits a source-safe JSON
receipt with normalized root-cause groups and the next bounded action.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import HTTPRedirectHandler, Request, build_opener, urlopen

API_VERSION = "2022-11-28"
SCHEMA = "chaptera.ci-failure-triage.v1"
ANSI_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\s+")
STEP_PREFIX_RE = re.compile(r"^##\[(?:group|endgroup|error|warning|debug|notice)\](?:[^ ]+ )?")
REPO_PATH_RE = re.compile(
    r"(?P<path>(?:\.github|apps|crates|deploy|docs|installer|packages|research|scripts|services|tools|vendor)/[^\s:'\"<>]+)"
)
RUST_ARROW_RE = re.compile(r"-->\s+(?P<path>[^:\s]+):\d+(?::\d+)?")
DIFF_RE = re.compile(r"\bDiff in (?P<path>.+?):\d+(?::\d+)?(?: at line \d+)?$")
HEX_RE = re.compile(r"\b[0-9a-f]{12,64}\b", re.IGNORECASE)
LINECOL_RE = re.compile(r":\d+(?::\d+)?\b")
WHITESPACE_RE = re.compile(r"\s+")


@dataclass(frozen=True)
class Rule:
    failure_class: str
    pattern: re.Pattern[str]
    action: str
    safe_rerun_now: bool = False


@dataclass(frozen=True)
class Finding:
    failure_class: str
    signature: str
    causal_line: str
    causal_path: str | None
    causal_path_in_pr_diff: bool | None
    failed_step: str | None
    recommended_action: str
    safe_rerun_failed_jobs_now: bool


RULES: tuple[Rule, ...] = (
    Rule(
        "FORMAT",
        re.compile(r"\bDiff in .+?:\d+", re.IGNORECASE),
        "Format only the owned Rust surface, inspect the diff, then rerun failed jobs.",
    ),
    Rule(
        "TOOLCHAIN_COMPONENT_MISSING",
        re.compile(r"(?:cargo-fmt|cargo-clippy).+is not installed|component ['\"]?(?:rustfmt|clippy)['\"]?.+unavailable", re.IGNORECASE),
        "Fix the workflow toolchain components; do not edit product Rust for this failure.",
    ),
    Rule(
        "WRONG_CRATE",
        re.compile(r"package ID specification .+ did not match any packages", re.IGNORECASE),
        "Check the workflow package name, workspace, manifest path, and command before changing product code.",
    ),
    Rule(
        "DUPLICATE_PACKAGE",
        re.compile(r"There are multiple .+ packages in your project|package specification .+ is ambiguous", re.IGNORECASE),
        "Version-qualify or otherwise disambiguate the workflow package selector; do not change product semantics.",
    ),
    Rule(
        "PRODUCER_DEPENDENCY",
        re.compile(r"artifact (?:was )?not found|Unable to find any artifacts|No artifacts found", re.IGNORECASE),
        "Check whether the producer ran, the artifact name matches, and the dependency edge is valid.",
    ),
    Rule(
        "PERMISSIONS",
        re.compile(r"Resource not accessible by integration|permission denied|HTTP 403\b|403 Forbidden|insufficient permission", re.IGNORECASE),
        "Check token/job permissions and the event trust boundary before changing source code.",
    ),
    Rule(
        "PATH_ARTIFACT",
        re.compile(r"No such file or directory|cannot find the path specified|The system cannot find the file specified", re.IGNORECASE),
        "Check producer step, working directory, checkout/ref, and artifact/output path.",
    ),
    Rule(
        "INFRA_NETWORK",
        re.compile(
            r"(?:\b429\b|\b502\b|\b503\b|rate limit|connection reset|connection refused|temporary failure|timed out|timeout while|download failed|network error|TLS handshake timeout)",
            re.IGNORECASE,
        ),
        "Treat as transient until reproduced; rerun failed jobs without pushing a new SHA.",
        safe_rerun_now=True,
    ),
    Rule(
        "COMPILE",
        re.compile(r"\berror\[E\d{4}\]|^error(?:\[[A-Za-z0-9_-]+\])?:\s|could not compile", re.IGNORECASE),
        "Inspect the first compiler error. If its path is outside the PR diff, compare against main before editing the branch.",
    ),
    Rule(
        "TEST",
        re.compile(r"\btest .+ FAILED\b|panicked at|assertion (?:left|right|failed)|failures:\s*$", re.IGNORECASE),
        "Isolate the first failing assertion/test. If unrelated to changed files, compare against main before editing the branch.",
    ),
)

NOISE_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(r"Node\.js .*deprecated", re.IGNORECASE),
    re.compile(r"DeprecationWarning", re.IGNORECASE),
    re.compile(r"##\[warning\]", re.IGNORECASE),
    re.compile(r"Post Run actions/", re.IGNORECASE),
    re.compile(r"Process completed with exit code", re.IGNORECASE),
    re.compile(r"process didn.t exit successfully", re.IGNORECASE),
)


def strip_log_prefix(line: str) -> str:
    line = ANSI_RE.sub("", line.rstrip("\n"))
    line = TIMESTAMP_RE.sub("", line)
    return STEP_PREFIX_RE.sub("", line).strip()


def is_noise(line: str) -> bool:
    return any(pattern.search(line) for pattern in NOISE_PATTERNS)


def normalize_signature_line(line: str) -> str:
    value = strip_log_prefix(line)
    value = re.sub(r"/home/runner/work/[^/]+/[^/]+/", "", value)
    value = re.sub(r"[A-Za-z]:\\[^\s]+", "<windows-path>", value)
    value = LINECOL_RE.sub(":<line>", value)
    value = HEX_RE.sub("<hex>", value)
    value = re.sub(r"\b\d{3,}\b", "<n>", value)
    return WHITESPACE_RE.sub(" ", value).strip()[:500]


def extract_causal_path(line: str) -> str | None:
    clean = strip_log_prefix(line)
    match = DIFF_RE.search(clean)
    if match:
        raw = match.group("path")
        repo_match = REPO_PATH_RE.search(raw.replace("\\", "/"))
