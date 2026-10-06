#!/usr/bin/env python3
from pathlib import Path

DOCKERFILE = Path("tools/ci/migration-consumer-runtime.Dockerfile")
WORKFLOW = Path(".github/workflows/migration-consumer-runtime.yml")
MIGRATION = Path(".github/workflows/migration-1050-corpus-matrix.yml")

def require(text: str, markers: tuple[str, ...], owner: str) -> None:
    missing = [marker for marker in markers if marker not in text]
    if missing:
        raise SystemExit(f"{owner}: missing marker(s): " + ", ".join(missing))

def main() -> int:
    dockerfile = DOCKERFILE.read_text(encoding="utf-8")
    workflow = WORKFLOW.read_text(encoding="utf-8")
    migration = MIGRATION.read_text(encoding="utf-8")

    require(
        dockerfile,
        (
            "FROM ubuntu:24.04",
            "scribus",
            "libreoffice-draw",
            "xvfb",
            "fonts-montserrat",
            "rm -rf /var/lib/apt/lists/*",
        ),
        "migration consumer Dockerfile",
    )
    require(
        workflow,
        (
            "workflow_dispatch:",
            "runs-on: ubuntu-24.04",
            "docker build -f tools/ci/migration-consumer-runtime.Dockerfile",
            "docker save chaptera-migration-consumers:v1",
            "chaptera.migration-consumer-runtime.v1",
            "archive_sha256",
            "repository_commit_sha",
            "retention-days: 30",
        ),
        "migration consumer producer workflow",
    )
    forbidden = ("pull_request:", "push:", "schedule:")
    present = [marker for marker in forbidden if marker in workflow]
    if present:
        raise SystemExit("producer must remain explicit/manual only: " + ", ".join(present))

    require(
        migration,
        (
            'PINNED_CONSUMER_RUNTIME_RUN_ID: "37531724213"',
            'PINNED_CONSUMER_RUNTIME_ARTIFACT: "chaptera-migration-consumer-runtime-v1-37531724213"',
            'PINNED_CONSUMER_RUNTIME_ARCHIVE_SHA256: "52dfcd8ba3bd49d9d6cb8d08a43e9e9df461002d124ca6db5c7088817492a9f0"',
            'PINNED_CONSUMER_RUNTIME_ARCHIVE_BYTES: "343844646"',
            'PINNED_CONSUMER_RUNTIME_IMAGE_ID: "sha256:8fdb2089485ce40ac9b77b7dba2e357c22d8bd61f7251fa8f1ebf441aff35a74"',
            "Download pinned independent-consumer runtime",
            "Verify and load pinned independent-consumer runtime",
            "zstd -d -c",
            "docker load",
            "docker image inspect",
            "Install independent editable-output consumers through apt fallback",
            "consumer_runtime:",
            "- pinned",
            "- apt",
            "docker run --rm",
        ),
        "Migration 1050 pinned consumer runtime",
    )

    print("Migration consumer runtime wiring guard: ok")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
