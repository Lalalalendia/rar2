#!/usr/bin/env python3
"""Physical security receipt for the Product PUB isolation authority.

This does not implement a second sandbox. It drives the shared
migration_pdf_worker_isolation.py wrapper with hostile helper payloads, then
binds those physical syscall results to the production Product wiring in
chaptera-server source.
"""

from __future__ import annotations

import argparse
import errno
import hashlib
import json
import os
import pathlib
import sys
import tempfile
import textwrap

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / "tools"
sys.path.insert(0, str(TOOLS))

from migration_pdf_worker_isolation import WorkerLimits, run_isolated_worker


NETWORK_PROBE = r"""#!/usr/bin/env python3
import errno
import json
import os
import pathlib
import socket

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
results = {}
for label, family in [("ipv4", socket.AF_INET), ("ipv6", socket.AF_INET6)]:
    try:
        sock = socket.socket(family, socket.SOCK_STREAM)
    except OSError as exc:
        results[label] = {"blocked": exc.errno == errno.EPERM, "errno": exc.errno}
    else:
        sock.close()
        results[label] = {"blocked": False, "errno": 0}
(out / "network.json").write_text(json.dumps(results, sort_keys=True), encoding="utf-8")
raise SystemExit(0 if all(v["blocked"] for v in results.values()) else 9)
"""

ENV_PROBE = r"""#!/usr/bin/env python3
import json
import os
import pathlib
import resource

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "env.json").write_text(json.dumps({
    "parent_secret_present": "CHAPTERA_PRODUCT_SECRET_CANARY" in os.environ,
    "network_policy": os.environ.get("CHAPTERA_NETWORK_POLICY"),
    "input_present": "CHAPTERA_WORKER_INPUT" in os.environ,
    "address_space": list(resource.getrlimit(resource.RLIMIT_AS)),
    "cpu": list(resource.getrlimit(resource.RLIMIT_CPU)),
    "open_files": list(resource.getrlimit(resource.RLIMIT_NOFILE)),
    "output_file": list(resource.getrlimit(resource.RLIMIT_FSIZE)),
}, sort_keys=True), encoding="utf-8")
"""

ESCAPE_PROBE = r"""#!/usr/bin/env python3
import errno
import json
import os
import pathlib

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
observed = {}
for name, call in [
    ("setsid", os.setsid),
    ("setpgid", lambda: os.setpgid(0, 0)),
]:
    try:
        call()
    except OSError as exc:
        observed[name] = exc.errno
    else:
        observed[name] = 0
(out / "escape.json").write_text(json.dumps(observed, sort_keys=True), encoding="utf-8")
raise SystemExit(0 if all(v == errno.EPERM for v in observed.values()) else 10)
"""

TIMEOUT_PROBE = r"""#!/usr/bin/env python3
import os
import pathlib
import time

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "started.txt").write_text("started", encoding="utf-8")
time.sleep(10)
(out / "late.txt").write_text("late", encoding="utf-8")
"""


def write_worker(root: pathlib.Path, name: str, source: str) -> pathlib.Path:
    path = root / name
    path.write_text(textwrap.dedent(source), encoding="utf-8")
    return path


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256_file(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require_product_wiring() -> dict[str, bool]:
    main = (ROOT / "apps/chaptera-server/src/main.rs").read_text(encoding="utf-8")
    ingress = (ROOT / "apps/chaptera-server/src/source_ingress_http.rs").read_text(
        encoding="utf-8"
    )
    api = (ROOT / "apps/chaptera-server/src/product_api_http.rs").read_text(
        encoding="utf-8"
    )
    worker = (ROOT / "apps/chaptera-server/src/product_replay_worker.rs").read_text(
        encoding="utf-8"
    )
    materializer = (
        ROOT / "apps/chaptera-server/src/revision_materializer.rs"
    ).read_text(encoding="utf-8")

    checks = {
        "serve_configures_isolated_product": (
            ".with_isolated_replay(" in main
            and "source_ingress_http::baseline_config(source_config)" in main
        ),
        "product_reuses_baseline_isolation_config": (
            "isolation_harness: config.baseline.isolation_harness.clone()" in ingress
            and "worker_binary: config.baseline.worker_binary.clone()" in ingress
            and "worker_wall_timeout:" in ingress
            and "worker_address_space_mb:" in ingress
            and "worker_cpu_seconds:" in ingress
            and "worker_open_files:" in ingress
            and "worker_output_file_mb:" in ingress
        ),
        "product_invokes_shared_harness": (
            ".arg(&self.config.isolation_harness)" in worker
            and '.arg("--clear-environment")' in worker
            and '.arg("--timeout")' in worker
            and '.arg("--address-space-mb")' in worker
            and '.arg("--cpu-seconds")' in worker
            and '.arg("--open-files")' in worker
            and '.arg("--output-file-mb")' in worker
        ),
        "materializer_receives_isolated_worker": (
            "materializer.set_isolated_replay(worker.clone())" in api
            and "set_isolated_replay" in materializer
        ),
        "current_document_uses_worker": ".project_authoring_graph(" in api,
        "move_node_uses_worker": ".move_node_to(" in api,
        "reader_scene_uses_worker": ".project_reader_scene(" in api,
    }
    failed = [name for name, passed in checks.items() if not passed]
    if failed:
        raise RuntimeError("production Product isolation wiring missing: " + ", ".join(failed))
    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        type=pathlib.Path,
        default=ROOT / "target/product-security-isolation/receipt.json",
    )
    args = parser.parse_args()

    limits = WorkerLimits(
        address_space_bytes=384 * 1024 * 1024,
        cpu_seconds=5,
        open_files=48,
        output_file_bytes=8 * 1024 * 1024,
    )

    with tempfile.TemporaryDirectory(prefix="chaptera-product-security-") as raw:
        temp = pathlib.Path(raw)
        source = temp / "source.pub"
        source.write_bytes(b"synthetic-product-security-probe")

        network_worker = write_worker(temp, "network_probe.py", NETWORK_PROBE)
        network_out = temp / "network-result"
        network_result = run_isolated_worker(
            [sys.executable, str(network_worker)],
            final_output_dir=network_out,
            timeout_seconds=5,
            limits=limits,
            input_path=source,
            inherit_environment=False,
        )
        if not network_result.succeeded:
            raise RuntimeError(f"network probe failed: {network_result.stderr_tail}")
        network = read_json(network_out / "network.json")
        if not all(
            row.get("blocked") is True and row.get("errno") == errno.EPERM
            for row in network.values()
        ):
            raise RuntimeError(f"network syscall was not fail-closed: {network}")

        env_worker = write_worker(temp, "env_probe.py", ENV_PROBE)
        env_out = temp / "env-result"
        previous = os.environ.get("CHAPTERA_PRODUCT_SECRET_CANARY")
        os.environ["CHAPTERA_PRODUCT_SECRET_CANARY"] = "must-not-cross-worker-boundary"
        try:
            env_result = run_isolated_worker(
                [sys.executable, str(env_worker)],
                final_output_dir=env_out,
                timeout_seconds=5,
                limits=limits,
                input_path=source,
                inherit_environment=False,
            )
        finally:
            if previous is None:
                os.environ.pop("CHAPTERA_PRODUCT_SECRET_CANARY", None)
            else:
                os.environ["CHAPTERA_PRODUCT_SECRET_CANARY"] = previous
        if not env_result.succeeded:
            raise RuntimeError(f"environment probe failed: {env_result.stderr_tail}")
        environment = read_json(env_out / "env.json")
        if environment["parent_secret_present"]:
            raise RuntimeError("parent secret crossed the worker environment boundary")
        if environment["network_policy"] != "seccomp_default_deny":
            raise RuntimeError("worker did not receive the explicit network policy")
        if not environment["input_present"]:
            raise RuntimeError("worker input contract missing after environment clear")

        escape_worker = write_worker(temp, "escape_probe.py", ESCAPE_PROBE)
        escape_out = temp / "escape-result"
        escape_result = run_isolated_worker(
            [sys.executable, str(escape_worker)],
            final_output_dir=escape_out,
            timeout_seconds=5,
            limits=limits,
            input_path=source,
            inherit_environment=False,
        )
        if not escape_result.succeeded:
            raise RuntimeError(f"process escape probe failed: {escape_result.stderr_tail}")
        escape = read_json(escape_out / "escape.json")
        if any(value != errno.EPERM for value in escape.values()):
            raise RuntimeError(f"process-group escape syscall was admitted: {escape}")

        timeout_worker = write_worker(temp, "timeout_probe.py", TIMEOUT_PROBE)
        timeout_out = temp / "timeout-result"
        timeout_result = run_isolated_worker(
            [sys.executable, str(timeout_worker)],
            final_output_dir=timeout_out,
            timeout_seconds=0.15,
            limits=limits,
            input_path=source,
            inherit_environment=False,
        )
        if (
            timeout_result.status != "timeout"
            or not timeout_result.timed_out
            or not timeout_result.staging_cleaned
            or timeout_out.exists()
        ):
            raise RuntimeError(f"wall-timeout did not fail closed: {timeout_result}")

    wiring = require_product_wiring()
    harness = ROOT / "tools/migration_pdf_worker_isolation.py"
    receipt = {
        "protocol_version": "chaptera.product-isolation-security-receipt.v1",
        "shared_harness_sha256": sha256_file(harness),
        "production_wiring": wiring,
        "network": network,
        "parent_secret_stripped": not environment["parent_secret_present"],
        "network_policy": environment["network_policy"],
        "worker_input_contract": environment["input_present"],
        "rlimits": {
            "address_space": environment["address_space"],
            "cpu": environment["cpu"],
            "open_files": environment["open_files"],
            "output_file": environment["output_file"],
        },
        "process_group_escape_errno": escape,
        "wall_timeout": {
            "status": timeout_result.status,
            "timed_out": timeout_result.timed_out,
            "staging_cleaned": timeout_result.staging_cleaned,
            "published_output_exists": timeout_out.exists(),
        },
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
