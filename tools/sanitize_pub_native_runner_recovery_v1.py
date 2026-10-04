#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

INPUT_SCHEMA = "chaptera.pub-native-runner-recovery.v1"
OUTPUT_SCHEMA = "chaptera.pub-native-runner-recovery-summary.v1"
EXPECTED_ENVIRONMENT = "publisher-2019"

ALLOWED_VERDICTS = {
    "unknown",
    "runner-service-not-installed-or-not-registered",
    "ambiguous-runner-service",
    "runner-service-stopped",
    "runner-service-start-failed",
    "packet-validation-failed",
    "publisher-environment-validation-failed",
    "runner-service-not-running",
    "runner-listener-not-observed",
    "runner-and-publisher-environment-ready",
    "recovery-error",
}
ALLOWED_RECOVERY_ACTIONS = {"none", "start", "restart"}
ALLOWED_VALIDATION_STATUS = {"not_run", "success", "failure"}
ALLOWED_SERVICE_STATUS = {
    "Stopped",
    "Start Pending",
    "Stop Pending",
    "Running",
    "Continue Pending",
    "Pause Pending",
    "Paused",
    "Unknown",
}
HEX64_RE = re.compile(r"^[0-9a-fA-F]{64}$")
ISO_UTC_RE = re.compile(r"^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d+)?Z$")


def require_dict(value: object, name: str) -> dict:
    if not isinstance(value, dict):
        raise ValueError(f"{name} must be an object")
    return value


def require_allowed(value: object, allowed: set[str], name: str) -> str:
    text = str(value)
    if text not in allowed:
        raise ValueError(f"unexpected {name}: {text!r}")
    return text


def optional_status(value: object, name: str) -> str | None:
    if value is None:
        return None
    text = str(value)
    if text not in ALLOWED_SERVICE_STATUS:
        raise ValueError(f"unexpected {name}: {text!r}")
    return text


def sanitize(payload: dict) -> dict:
    if payload.get("schema") != INPUT_SCHEMA:
        raise ValueError(f"unsupported recovery schema: {payload.get('schema')!r}")
    if payload.get("expected_environment") != EXPECTED_ENVIRONMENT:
        raise ValueError("unexpected recovery environment")

    runner = require_dict(payload.get("runner"), "runner")
    packet = require_dict(payload.get("packet"), "packet")
    packet_validation = require_dict(payload.get("packet_validation"), "packet_validation")
    environment_validation = require_dict(
        payload.get("environment_validation"), "environment_validation"
    )

    verdict = require_allowed(payload.get("verdict"), ALLOWED_VERDICTS, "verdict")
    recovery_action = require_allowed(
        runner.get("recovery_action"), ALLOWED_RECOVERY_ACTIONS, "recovery_action"
    )
    packet_status = require_allowed(
        packet_validation.get("status"),
        ALLOWED_VALIDATION_STATUS,
        "packet_validation.status",
    )
    environment_status = require_allowed(
        environment_validation.get("status"),
        ALLOWED_VALIDATION_STATUS,
        "environment_validation.status",
    )

    packet_sha = packet.get("sha256")
    if not isinstance(packet_sha, str) or HEX64_RE.fullmatch(packet_sha) is None:
        raise ValueError("packet SHA-256 must be exactly 64 hexadecimal characters")

    captured_at_utc = payload.get("captured_at_utc")
    if not isinstance(captured_at_utc, str) or ISO_UTC_RE.fullmatch(captured_at_utc) is None:
        raise ValueError("captured_at_utc must be an ISO-8601 UTC timestamp")

    service_count = runner.get("service_count")
    if not isinstance(service_count, int) or service_count < 0:
        raise ValueError("runner.service_count must be a non-negative integer")

    listener_present = runner.get("listener_present")
    if not isinstance(listener_present, bool):
        raise ValueError("runner.listener_present must be boolean")

    packet_exit_code = packet_validation.get("exit_code")
    if packet_exit_code is not None and not isinstance(packet_exit_code, int):
        raise ValueError("packet_validation.exit_code must be integer or null")

    ready = verdict == "runner-and-publisher-environment-ready"
    if ready:
        if optional_status(runner.get("service_status_after"), "runner.service_status_after") != "Running":
            raise ValueError("ready verdict requires Running service")
        if listener_present is not True:
            raise ValueError("ready verdict requires Runner.Listener")
        if packet_status != "success":
            raise ValueError("ready verdict requires packet validation success")
        if environment_status != "success":
            raise ValueError("ready verdict requires environment validation success")

    return {
        "schema": OUTPUT_SCHEMA,
        "captured_at_utc": captured_at_utc,
        "expected_environment": EXPECTED_ENVIRONMENT,
        "packet_sha256": packet_sha.lower(),
        "runner": {
            "service_count": service_count,
            "service_status_before": optional_status(
                runner.get("service_status_before"), "runner.service_status_before"
            ),
            "service_status_after": optional_status(
                runner.get("service_status_after"), "runner.service_status_after"
            ),
            "recovery_action": recovery_action,
            "listener_present": listener_present,
        },
        "packet_validation": {
            "status": packet_status,
            "exit_code": packet_exit_code,
        },
        "environment_validation": {
            "status": environment_status,
        },
        "ready": ready,
        "verdict": verdict,
        "claims": {
            "local_paths_emitted": False,
            "service_name_emitted": False,
            "agent_name_emitted": False,
            "github_url_emitted": False,
            "runner_registration_data_emitted": False,
            "credentials_emitted": False,
            "publisher_executable_path_emitted": False,
            "private_error_text_emitted": False,
            "packet_hash_emitted": True,
        },
    }


def self_test() -> None:
    secret_markers = [
        "SENTINEL_LOCAL_EXECUTABLE",
        "SENTINEL_PRIVATE_PACKET_PATH",
        "SENTINEL_SERVICE_NAME",
        "SENTINEL_AGENT_NAME",
        "SENTINEL_GITHUB_URL",
        "SENTINEL_CREDENTIAL",
        "SENTINEL_PRIVATE_ERROR",
        "SENTINEL_REPOSITORY_ROOT",
        "SENTINEL_RUNNER_ROOT",
        "SENTINEL_RUNNER_FILE",
        "SENTINEL_PREPARE_ROOT",
    ]
    payload = {
        "schema": INPUT_SCHEMA,
        "captured_at_utc": "2026-10-04T10:00:00Z",
        "repository_root": secret_markers[7],
        "expected_environment": EXPECTED_ENVIRONMENT,
        "packet": {
            "path": secret_markers[1],
            "sha256": "a" * 64,
        },
        "runner": {
            "service_count": 1,
            "selected_service": secret_markers[2],
            "service_status_before": "Stopped",
            "service_status_after": "Running",
            "service_start_mode": "Automatic",
            "recovery_action": "start",
            "executable": secret_markers[0],
            "root": secret_markers[8],
            "runner_file": secret_markers[9],
            "agent_name": secret_markers[3],
            "github_url": secret_markers[4],
            "listener_present": True,
        },
        "packet_validation": {
            "status": "success",
            "exit_code": 0,
            "output": [secret_markers[5]],
        },
        "environment_validation": {
            "status": "success",
            "output_root": secret_markers[10],
            "error": secret_markers[6],
        },
        "verdict": "runner-and-publisher-environment-ready",
        "error": secret_markers[6],
    }

    summary = sanitize(payload)
    assert summary["ready"] is True
    assert summary["verdict"] == "runner-and-publisher-environment-ready"
    serialized = json.dumps(summary, sort_keys=True)
    for marker in secret_markers:
        assert marker not in serialized, marker

    blocked = json.loads(json.dumps(payload))
    blocked["verdict"] = "runner-listener-not-observed"
    blocked["runner"]["listener_present"] = False
    blocked_summary = sanitize(blocked)
    assert blocked_summary["ready"] is False

    inconsistent = json.loads(json.dumps(payload))
    inconsistent["runner"]["listener_present"] = False
    try:
        sanitize(inconsistent)
    except ValueError:
        pass
    else:
        raise AssertionError("ready verdict without listener must fail")

    bad_status = json.loads(json.dumps(payload))
    bad_status["runner"]["service_status_after"] = "SENTINEL_PRIVATE_STATUS"
    try:
        sanitize(bad_status)
    except ValueError:
        pass
    else:
        raise AssertionError("unbounded service status must fail")

    bad_timestamp = json.loads(json.dumps(payload))
    bad_timestamp["captured_at_utc"] = "SENTINEL_PRIVATE_TIMESTAMP"
    try:
        sanitize(bad_timestamp)
    except ValueError:
        pass
    else:
        raise AssertionError("unbounded timestamp must fail")

    bad_sha = json.loads(json.dumps(payload))
    bad_sha["packet"]["sha256"] = "x" * 64
    try:
        sanitize(bad_sha)
    except ValueError:
        pass
    else:
        raise AssertionError("non-hex packet SHA must fail")

    print("pub native runner recovery summary self-test: ok")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("receipt", nargs="?", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if args.receipt is None:
        parser.error("receipt is required unless --self-test is used")

    payload = json.loads(args.receipt.read_text(encoding="utf-8-sig"))
    summary = sanitize(require_dict(payload, "receipt"))
    rendered = json.dumps(summary, indent=2, sort_keys=True) + "\n"
    if args.out is not None:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(rendered, encoding="utf-8")
    print(rendered, end="")


if __name__ == "__main__":
    main()
