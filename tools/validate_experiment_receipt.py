#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
from collections.abc import Mapping
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "packages" / "protocol" / "experiment-receipt" / "v1" / "receipt.schema.json"

SUPPORTED_SPECIALIZED_RECEIPTS = {
    "StateTransitionReceiptV1": {
        "version": "chaptera.state-transition-receipt.v1",
    },
}

_WINDOWS_ABSOLUTE = re.compile(r"(?i)(?:^|[\s\"'])(?:[a-z]:[\\/]|\\\\)")
_POSIX_ABSOLUTE = re.compile(r"(?:^|[\s\"'])/(?!/)")
_HOME_ALIAS = re.compile(r"(?:^|[\s\"'])~[\\/]")
_DATA_URI = re.compile(r"(?i)^data:[^,]*;base64,")

_FORBIDDEN_FIELD_NAMES = {
    "username",
    "user_name",
    "user_home",
    "home",
    "home_directory",
    "hostname",
    "host_name",
    "machine_name",
    "source_bytes",
    "pub_bytes",
    "binary",
    "binary_payload",
    "source_payload",
    "document_payload",
    "raw_payload",
    "raw_bytes",
    "file_bytes",
    "document_text",
    "customer_text",
    "base64",
    "content_base64",
    "payload_base64",
    "private_content",
    "password",
    "secret",
    "api_key",
    "access_token",
}


class ExperimentReceiptError(ValueError):
    pass


def canonical_json(value: Any) -> bytes:
    try:
        encoded = json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        )
    except (TypeError, ValueError) as exc:
        raise ExperimentReceiptError(f"value is not canonical JSON: {exc}") from exc
    return encoded.encode("utf-8")


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(canonical_json(value)).hexdigest()


def _object(
    value: Any,
    path: str,
    required: set[str],
    optional: set[str] | None = None,
) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ExperimentReceiptError(f"{path} must be an object")
    optional = optional or set()
    missing = sorted(required - value.keys())
    if missing:
        raise ExperimentReceiptError(f"{path} is missing required fields: {', '.join(missing)}")
    extra = sorted(value.keys() - required - optional)
    if extra:
        raise ExperimentReceiptError(f"{path} contains unknown fields: {', '.join(extra)}")
    return value


def _string(value: Any, path: str, *, maximum: int = 512) -> str:
    if not isinstance(value, str) or not value or len(value) > maximum:
        raise ExperimentReceiptError(f"{path} must be a non-empty string of at most {maximum} characters")
    return value


def _identifier(value: Any, path: str) -> str:
    text = _string(value, path, maximum=160)
    if re.fullmatch(r"[0-9A-Za-z._:/+\-]+", text) is None:
        raise ExperimentReceiptError(f"{path} is not a normalized identifier")
    return text


def _sha256(value: Any, path: str) -> str:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise ExperimentReceiptError(f"{path} must be a lowercase SHA-256 digest")
    return value


def _string_list(
    value: Any,
    path: str,
    *,
    identifier: bool = False,
    minimum: int = 0,
    maximum: int = 128,
) -> list[str]:
    if not isinstance(value, list) or not minimum <= len(value) <= maximum:
        raise ExperimentReceiptError(f"{path} must contain {minimum}..{maximum} strings")
    output = []
    for index, item in enumerate(value):
        output.append(
            _identifier(item, f"{path}[{index}]")
            if identifier
            else _string(item, f"{path}[{index}]")
        )
    if len(set(output)) != len(output):
        raise ExperimentReceiptError(f"{path} must not contain duplicates")
    return output


def _artifact_identity(value: Any, path: str) -> None:
    identity = _object(value, path, {"sha256", "size_bytes"}, {"logical_fixture_id"})
    _sha256(identity["sha256"], f"{path}.sha256")
    size = identity["size_bytes"]
    if isinstance(size, bool) or not isinstance(size, int) or size < 0:
        raise ExperimentReceiptError(f"{path}.size_bytes must be a non-negative integer")
    if "logical_fixture_id" in identity:
        _identifier(identity["logical_fixture_id"], f"{path}.logical_fixture_id")


def _evidence_value(value: Any, path: str, *, identity: bool = False) -> str:
    evidence = _object(value, path, {"status"}, {"value", "reason"})
    status = evidence["status"]
    if status == "known":
        if set(evidence) != {"status", "value"}:
            raise ExperimentReceiptError(f"{path} known evidence requires only status and value")
        if identity:
            _artifact_identity(evidence["value"], f"{path}.value")
    elif status in {"unknown", "unavailable"}:
        if set(evidence) != {"status", "reason"}:
            raise ExperimentReceiptError(f"{path} {status} evidence requires only status and reason")
        _string(evidence["reason"], f"{path}.reason")
    else:
        raise ExperimentReceiptError(f"{path}.status must be known, unknown, or unavailable")
    return status


def _validate_experiment_shape(receipt: Any) -> None:
    required = {
        "receipt_version", "task_id", "run_id", "producer", "system", "build",
        "environment", "source_identity", "output_identity", "operation",
        "semantic_observations", "changed_artifact_summary", "evidence_refs",
        "lifecycle_disposition", "evidence_scope", "authority_ceiling", "non_claims",
        "privacy",
    }
    root = _object(receipt, "receipt", required)
    if root["receipt_version"] != "chaptera.experiment-receipt.v1":
        raise ExperimentReceiptError("receipt.receipt_version is incompatible")
    _identifier(root["task_id"], "receipt.task_id")
    _identifier(root["run_id"], "receipt.run_id")

    for name in ("producer", "system"):
        identity = _object(root[name], f"receipt.{name}", {"name", "version"})
        _string(identity["name"], f"receipt.{name}.name")
        _string(identity["version"], f"receipt.{name}.version")

    build = _object(root["build"], "receipt.build", {"repository", "revision"})
    _string(build["repository"], "receipt.build.repository")
    if not isinstance(build["revision"], str) or re.fullmatch(r"[0-9a-f]{40}", build["revision"]) is None:
        raise ExperimentReceiptError("receipt.build.revision must be a lowercase 40-character Git SHA")

    environment = _object(
        root["environment"],
        "receipt.environment",
        {"os", "architecture", "runtime", "fingerprint_sha256"},
    )
    for name in ("os", "architecture", "runtime"):
        _string(environment[name], f"receipt.environment.{name}")
    _sha256(environment["fingerprint_sha256"], "receipt.environment.fingerprint_sha256")

    _artifact_identity(root["source_identity"], "receipt.source_identity")
    _evidence_value(root["output_identity"], "receipt.output_identity", identity=True)

    operation = _object(root["operation"], "receipt.operation", {"kind", "parameters"})
    _identifier(operation["kind"], "receipt.operation.kind")
    if not isinstance(operation["parameters"], dict):
        raise ExperimentReceiptError("receipt.operation.parameters must be an object")

    observations = _object(
        root["semantic_observations"],
        "receipt.semantic_observations",
        {"before", "after", "reopen"},
    )
    for phase in ("before", "after", "reopen"):
        _evidence_value(observations[phase], f"receipt.semantic_observations.{phase}")
    _evidence_value(root["changed_artifact_summary"], "receipt.changed_artifact_summary")

    references = root["evidence_refs"]
    if not isinstance(references, list) or len(references) > 128:
        raise ExperimentReceiptError("receipt.evidence_refs must be an array of at most 128 entries")
    for index, value in enumerate(references):
        reference = _object(
            value,
            f"receipt.evidence_refs[{index}]",
            {"type", "version", "sha256"},
            {"relative_artifact_name"},
        )
        _identifier(reference["type"], f"receipt.evidence_refs[{index}].type")
        _identifier(reference["version"], f"receipt.evidence_refs[{index}].version")
        _sha256(reference["sha256"], f"receipt.evidence_refs[{index}].sha256")
        if "relative_artifact_name" in reference:
            name = _string(reference["relative_artifact_name"], f"receipt.evidence_refs[{index}].relative_artifact_name", maximum=256)
            if re.fullmatch(r"[0-9A-Za-z._/\-]+", name) is None:
                raise ExperimentReceiptError("evidence relative_artifact_name contains unsupported characters")

    lifecycle = _object(root["lifecycle_disposition"], "receipt.lifecycle_disposition", {"status", "reason"})
    if lifecycle["status"] not in {"completed", "partial", "failed", "aborted"}:
        raise ExperimentReceiptError("receipt.lifecycle_disposition.status is invalid")
    _string(lifecycle["reason"], "receipt.lifecycle_disposition.reason")
    _string_list(root["evidence_scope"], "receipt.evidence_scope", identifier=True, minimum=1, maximum=64)
    if root["authority_ceiling"] not in {
        "synthetic_contract", "hosted_observation", "native_observation", "product_acceptance"
    }:
        raise ExperimentReceiptError("receipt.authority_ceiling is invalid")
    _string_list(root["non_claims"], "receipt.non_claims", minimum=1, maximum=64)
    privacy = _object(root["privacy"], "receipt.privacy", {"source_free", "private_content_serialized"})
    if privacy["source_free"] is not True or privacy["private_content_serialized"] is not False:
        raise ExperimentReceiptError("receipt.privacy must assert the source-free public boundary")


def _specialized_evidence_field(value: Any, path: str) -> None:
    evidence = _object(value, path, {"status"}, {"value", "reason"})
    if evidence["status"] == "known":
        if set(evidence) != {"status", "value"}:
            raise ExperimentReceiptError(f"{path} known evidence is malformed")
    elif evidence["status"] in {"unknown", "unavailable"}:
        if set(evidence) != {"status", "reason"}:
            raise ExperimentReceiptError(f"{path} unavailable evidence is malformed")
        _string(evidence["reason"], f"{path}.reason")
    else:
        raise ExperimentReceiptError(f"{path}.status is invalid")


def _validate_state_transition_receipt(artifact: Any) -> None:
    root = _object(
        artifact,
        "StateTransitionReceiptV1",
        {"receipt_version", "contract", "system", "fixture", "comparison", "before", "operation", "after", "invariants"},
    )
    if root["receipt_version"] != "chaptera.state-transition-receipt.v1":
        raise ExperimentReceiptError("referenced StateTransitionReceiptV1 artifact declares incompatible receipt_version")
    contract = _object(root["contract"], "StateTransitionReceiptV1.contract", {"id", "version"})
    _identifier(contract["id"], "StateTransitionReceiptV1.contract.id")
    _identifier(contract["version"], "StateTransitionReceiptV1.contract.version")
    system = _object(root["system"], "StateTransitionReceiptV1.system", {"kind", "runtime", "build"})
    for name in system:
        _string(system[name], f"StateTransitionReceiptV1.system.{name}", maximum=128)
    fixture = _object(root["fixture"], "StateTransitionReceiptV1.fixture", {"kind", "identity", "provenance"})
    if fixture["kind"] not in {"synthetic", "public", "sanitized_private", "native_oracle"}:
        raise ExperimentReceiptError("StateTransitionReceiptV1.fixture.kind is invalid")
    _string(fixture["identity"], "StateTransitionReceiptV1.fixture.identity", maximum=160)
    _string(fixture["provenance"], "StateTransitionReceiptV1.fixture.provenance")
    comparison = _object(root["comparison"], "StateTransitionReceiptV1.comparison", {"required_state_fields", "required_invariants"})
    _string_list(comparison["required_state_fields"], "StateTransitionReceiptV1.comparison.required_state_fields", identifier=True, minimum=1)
    _string_list(comparison["required_invariants"], "StateTransitionReceiptV1.comparison.required_invariants", identifier=True)
    for phase in ("before", "after"):
        state = _object(root[phase], f"StateTransitionReceiptV1.{phase}", {"fields"})
        if not isinstance(state["fields"], dict):
            raise ExperimentReceiptError(f"StateTransitionReceiptV1.{phase}.fields must be an object")
        for name, evidence in state["fields"].items():
            _specialized_evidence_field(evidence, f"StateTransitionReceiptV1.{phase}.fields.{name}")
    operation = _object(root["operation"], "StateTransitionReceiptV1.operation", {"kind", "inputs"})
    _string(operation["kind"], "StateTransitionReceiptV1.operation.kind", maximum=128)
    if not isinstance(operation["inputs"], dict):
        raise ExperimentReceiptError("StateTransitionReceiptV1.operation.inputs must be an object")
    if not isinstance(root["invariants"], dict):
        raise ExperimentReceiptError("StateTransitionReceiptV1.invariants must be an object")
    for name, evidence in root["invariants"].items():
        _specialized_evidence_field(evidence, f"StateTransitionReceiptV1.invariants.{name}")


def _normalized_field_name(name: str) -> str:
    return re.sub(r"[^0-9a-z]+", "_", name.casefold()).strip("_")


def _privacy_scan(value: Any, path: tuple[str, ...] = ()) -> None:
    if isinstance(value, Mapping):
        for key, child in value.items():
            key_text = str(key)
            normalized = _normalized_field_name(key_text)
            if normalized in _FORBIDDEN_FIELD_NAMES or "payload" in normalized:
                location = ".".join((*path, key_text))
                raise ExperimentReceiptError(f"source-free receipt contains forbidden field {location!r}")
            _privacy_scan(child, (*path, key_text))
        return
    if isinstance(value, list):
        for index, child in enumerate(value):
            _privacy_scan(child, (*path, str(index)))
        return
    if not isinstance(value, str):
        return

    if _WINDOWS_ABSOLUTE.search(value) or _POSIX_ABSOLUTE.search(value) or _HOME_ALIAS.search(value):
        location = ".".join(path)
        raise ExperimentReceiptError(f"source-free receipt contains an absolute local path at {location!r}")
    if _DATA_URI.match(value):
        location = ".".join(path)
        raise ExperimentReceiptError(f"source-free receipt contains an embedded binary data URI at {location!r}")


def _artifact_for_ref(
    reference: dict[str, Any],
    evidence_artifacts: Mapping[str, Any],
) -> Any:
    key = reference.get("relative_artifact_name", reference["sha256"])
    if key not in evidence_artifacts:
        raise ExperimentReceiptError(f"referenced evidence artifact {key!r} was not supplied")
    return evidence_artifacts[key]


def _validate_reference(reference: dict[str, Any], artifact: Any) -> None:
    receipt_type = reference["type"]
    supported = SUPPORTED_SPECIALIZED_RECEIPTS.get(receipt_type)
    if supported is None:
        raise ExperimentReceiptError(f"unsupported specialized receipt type {receipt_type!r}")
    if reference["version"] != supported["version"]:
        raise ExperimentReceiptError(
            f"specialized receipt type/version mismatch for {receipt_type!r}: "
            f"expected {supported['version']!r}, got {reference['version']!r}"
        )
    if not isinstance(artifact, dict):
        raise ExperimentReceiptError(f"referenced {receipt_type!r} artifact must be a JSON object")
    if artifact.get("receipt_version") != reference["version"]:
        raise ExperimentReceiptError(
            f"referenced {receipt_type!r} artifact declares incompatible receipt_version"
        )
    if receipt_type == "StateTransitionReceiptV1":
        _validate_state_transition_receipt(artifact)
    _privacy_scan(artifact, ("referenced_evidence", receipt_type))
    actual_digest = canonical_sha256(artifact)
    if actual_digest != reference["sha256"]:
        raise ExperimentReceiptError(
            f"referenced {receipt_type!r} digest mismatch: expected {reference['sha256']}, "
            f"got {actual_digest}"
        )


def _validate_reference_names(receipt: dict[str, Any]) -> None:
    seen: set[tuple[str, str, str, str | None]] = set()
    for reference in receipt["evidence_refs"]:
        name = reference.get("relative_artifact_name")
        if name is not None:
            candidate = pathlib.PurePosixPath(name)
            if candidate.is_absolute() or ".." in candidate.parts:
                raise ExperimentReceiptError(
                    f"evidence reference must use a normalized relative artifact name: {name!r}"
                )
        identity = (reference["type"], reference["version"], reference["sha256"], name)
        if identity in seen:
            raise ExperimentReceiptError("duplicate evidence reference")
        seen.add(identity)


def _comparison_status(receipt: dict[str, Any]) -> str:
    observations = receipt["semantic_observations"]
    statuses = [observations[phase]["status"] for phase in ("before", "after", "reopen")]
    return "comparable" if all(status == "known" for status in statuses) else "not_comparable"


def validate_receipt(
    receipt: dict[str, Any],
    evidence_artifacts: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    # Keep the declarative schema shipped with the protocol parseable while the
    # runtime validator remains dependency-free for hosted and local execution.
    json.loads(SCHEMA.read_text(encoding="utf-8"))
    canonical_json(receipt)
    _validate_experiment_shape(receipt)
    _privacy_scan(receipt)
    _validate_reference_names(receipt)

    artifacts = evidence_artifacts or {}
    for reference in receipt["evidence_refs"]:
        artifact = _artifact_for_ref(reference, artifacts)
        _validate_reference(reference, artifact)

    comparison_status = _comparison_status(receipt)
    if receipt["lifecycle_disposition"]["status"] == "completed" and comparison_status != "comparable":
        raise ExperimentReceiptError(
            "completed experiment receipt requires known before/after/reopen observations"
        )

    return {
        "receipt_version": receipt["receipt_version"],
        "task_id": receipt["task_id"],
        "run_id": receipt["run_id"],
        "canonical_sha256": canonical_sha256(receipt),
        "comparison_status": comparison_status,
        "referenced_evidence_count": len(receipt["evidence_refs"]),
        "authority_ceiling": receipt["authority_ceiling"],
        "source_free": True,
    }


def _load_evidence_arguments(values: list[str]) -> dict[str, Any]:
    artifacts: dict[str, Any] = {}
    for value in values:
        if "=" not in value:
            raise ExperimentReceiptError("--evidence must use NAME=PATH")
        name, raw_path = value.split("=", 1)
        if not name or name in artifacts:
            raise ExperimentReceiptError(f"invalid or duplicate evidence name {name!r}")
        path = pathlib.Path(raw_path)
        artifacts[name] = json.loads(path.read_text(encoding="utf-8"))
    return artifacts


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate a Chaptera ExperimentReceiptV1 envelope")
    parser.add_argument("receipt", type=pathlib.Path)
    parser.add_argument(
        "--evidence",
        action="append",
        default=[],
        metavar="NAME=PATH",
        help="supply a referenced specialized receipt (repeatable)",
    )
    args = parser.parse_args()

    try:
        receipt = json.loads(args.receipt.read_text(encoding="utf-8"))
        artifacts = _load_evidence_arguments(args.evidence)
        result = validate_receipt(receipt, artifacts)
    except (OSError, json.JSONDecodeError, ExperimentReceiptError) as exc:
        print(f"experiment receipt validation failed: {exc}")
        return 1

    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
