#!/usr/bin/env python3
from pathlib import Path

DOCKERFILE = Path("tools/ci/migration-consumer-runtime.Dockerfile")
WORKFLOW = Path(".github/workflows/migration-consumer-runtime.yml")

def require(text: str, markers: tuple[str, ...], owner: str) -> None:
    missing = [marker for marker in markers if marker not in text]
    if missing:
        raise SystemExit(f"{owner}: missing marker(s): " + ", ".join(missing))

def main() -> int:
    dockerfile = DOCKERFILE.read_text(encoding="utf-8")
    workflow = WORKFLOW.read_text(encoding="utf-8")

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

    print("Migration consumer runtime wiring guard: ok")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
