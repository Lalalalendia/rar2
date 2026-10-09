#!/usr/bin/env python3
"""Fail-closed wiring and negative controls for Cloud export's Ubuntu APT mirror fallback."""
import os
import shlex
import subprocess
import tempfile
import textwrap
from pathlib import Path

WORKFLOW = Path(".github/workflows/cloud-export-executor-producer-v1.yml")
START = "      - name: Install independent editable-output consumers\n        run: |\n"
END = "\n      - name: Fetch pinned real Publisher fixture\n"
CONSUMERS = "consumers=(scribus libreoffice-draw xvfb)"
OFFICIAL_MIRROR = "s@http://azure.archive.ubuntu.com/ubuntu@http://archive.ubuntu.com/ubuntu@g"
DOWNLOAD = 'install -y --download-only --no-install-recommends "${consumers[@]}"'
INSTALL = 'sudo apt-get "${apt_opts[@]}" install -y --no-download --no-install-recommends "${consumers[@]}"'


def require(condition: bool, what: str) -> None:
    if not condition:
        raise AssertionError(what)


def enforce(source: str) -> None:
    require(source.count(START) == 1 and source.count(END) == 1, "Consumer installation owner missing/duplicated")
    script = textwrap.dedent(source.split(START, 1)[1].split(END, 1)[0])
    bash = subprocess.run(["bash", "-n"], input=script, text=True, capture_output=True, check=False)
    require(bash.returncode == 0, f"Consumer installer Bash syntax invalid: {bash.stderr}")
    require("set -euo pipefail" in script, "Shell fail-closed removed")
    require(CONSUMERS in script, "Any of the three required real consumers removed")
    require(script.count("sudo timeout --kill-after=10s") == 4, "APT time boundaries removed")
    require("120s apt-get " in script and "180s apt-get " in script and "300s apt-get " in script , "Bounded primary/fallback budgets changed")
    require("-o Acquire::Retries=1 -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15" in script, "Exact short APT retry/timeout policy removed")
    require(script.count(DOWNLOAD) == 2, "Network download-only primary/fallback proof missing")
    require(script.count(INSTALL) == 1, "Offline installation after downloads missing")
    require("if ! sudo timeout" in script and "then" in script and "\nfi" in script, "Real primary failure branch lost")
    require("mirror_file=/etc/apt/apt-mirrors.txt" in script, "Hosted-runner Ubuntu mirror authority changed")
    require('grep -Fq "http://azure.archive.ubuntu.com/ubuntu" "$mirror_file"' in script, "Azure expected-source check removed")
    require(OFFICIAL_MIRROR in script, "Only official Ubuntu archive fallback allowed")
    require('sudo sed -i ' in script and '"$mirror_file"' in script, "Official Ubuntu mirror must actually be selected")
    require("exit 1" in script and "::error::" in script, "Unknown mirror must fail closed")
    require("|| true" not in script and "continue-on-error:" not in source, "Unsafe soft failure accepted")
    for cli in ("command -v scribus", "command -v libreoffice", "command -v xvfb-run"):
        require(cli in script, f"Installed real consumer executable validation missing: {cli}")
    for product in (
        "Prove exact edited real-PUB IDML + ODG producers",
        "Prove IDML survives independent Scribus consumer",
        "xvfb-run -a scribus -g -ns -py",
        "Prove ODG survives independent LibreOffice Draw consumer",
        "libreoffice --headless",
        "Verify schema migration v10",
        "actions/upload-artifact@v4",
    ):
        require(product in source, f"Independent product proof removed: {product}")
    require('    runs-on: ubuntu-latest' in source, "Original Ubuntu host proof replaced")
    require('    timeout-minutes: 45' in source, "Original job execution budget weakened")



def prove_mocked_fallback(source: str) -> None:
    """Exercise Bash branches with fake sudo/apt and a disposable mirrorlist.

    This test never runs real apt, sudo or system sed against the runner image.
    Only the in-memory script under test has its mirrorlist redirected to tmp.
    """
    extracted = textwrap.dedent(source.split(START, 1)[1].split(END, 1)[0])
    shim = r"""
sudo() {
  printf '%s\n' "$*" >> "$FAKE_APT_TRACE"
  if [[ "$1" == sed ]]; then
    shift
    command sed "$@"
    return
  fi
  if [[ "$1" == timeout ]]; then
    if [[ "$FAKE_APT_MODE" == all-fail ]]; then
      return 124
    fi
    if [[ "$FAKE_APT_MODE" == primary-fail ]] && grep -Fq "azure.archive.ubuntu.com" "$FAKE_MIRROR_FILE"; then
      return 124
    fi
    return 0
  fi
  if [[ "$1" == apt-get ]]; then
    [[ "$*" == *--no-download* ]] || return 94
    return 0
  fi
  return 95
}
"""
    for mode, initial_mirror, expected_success, expected_fallback in (
        ("primary-success", "http://azure.archive.ubuntu.com/ubuntu/\n", True, False),
        ("primary-fail", "http://azure.archive.ubuntu.com/ubuntu/\n", True, True),
        ("all-fail", "http://azure.archive.ubuntu.com/ubuntu/\n", False, True),
        ("primary-fail", "http://unknown.example/ubuntu/\n", False, False),
    ):
        with tempfile.TemporaryDirectory(prefix="cloud-apt-mirror-test-") as directory:
            root = Path(directory)
            mirror = root / "apt-mirrors.txt"
            trace = root / "calls.txt"
            mock_bin = root / "bin"
            mock_bin.mkdir()
            mirror.write_text(initial_mirror, encoding="utf-8")
            for executable in ("scribus", "libreoffice", "xvfb-run"):
                stub = mock_bin / executable
                stub.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
                stub.chmod(0o755)
            mocked = extracted.replace(
                "mirror_file=/etc/apt/apt-mirrors.txt",
                "mirror_file=" + shlex.quote(str(mirror)),
                1,
            )
            env = os.environ.copy()
            env.update({
                "FAKE_APT_MODE": mode,
                "FAKE_APT_TRACE": str(trace),
                "FAKE_MIRROR_FILE": str(mirror),
                "PATH": str(mock_bin) + os.pathsep + env.get("PATH", ""),
            })
            execution = subprocess.run(
                ["bash", "-c", shim + "\n" + mocked],
                env=env,
                text=True,
                capture_output=True,
                timeout=15,
                check=False,
            )
            label = f"{mode}/{initial_mirror.split('//')[1].split('/')[0]}"
            require((execution.returncode == 0) == expected_success,
                    f"{label} status={execution.returncode}: {execution.stderr}")
            history = trace.read_text(encoding="utf-8") if trace.exists() else ""
            require(("sed -i " in history) == expected_fallback,
                    f"{label}: invalid mirror-fallback choice")
            require(("--no-download" in history) == expected_success,
                    f"{label}: offline verified-install proof mismatch")
            if expected_fallback:
                require("http://archive.ubuntu.com/ubuntu/" in mirror.read_text(encoding="utf-8"),
                        f"{label}: official mirror not selected")


def main() -> int:
    raw = WORKFLOW.read_text(encoding="utf-8")
    enforce(raw)
    negative = [
        raw.replace(CONSUMERS, "consumers=(libreoffice-draw xvfb)", 1),
        raw.replace(CONSUMERS, "consumers=(scribus xvfb)", 1),
        raw.replace(CONSUMERS, "consumers=(scribus libreoffice-draw)", 1),
        raw.replace(OFFICIAL_MIRROR, "s@http://azure.archive.ubuntu.com/ubuntu@http://untrusted.example/ubuntu@g", 1),
        raw.replace(DOWNLOAD, 'install -y --no-install-recommends "${consumers[@]}"', 1),
        raw.replace(INSTALL, DOWNLOAD, 1),
        raw.replace("sudo timeout --kill-after=10s 180s", "sudo timeout 1800s", 1),
        raw.replace("Acquire::Retries=1", "Acquire::Retries=10", 1),
        raw.replace('grep -Fq "http://azure.archive.ubuntu.com/ubuntu" "$mirror_file"', 'echo "trust any host"', 1),
        raw.replace("exit 1", "exit 0", 1),
        raw.replace("set -euo pipefail", "set +e", 1),
        raw.replace("Prove IDML survives independent Scribus consumer", "skip IDML consumer", 1),
        raw.replace("Prove ODG survives independent LibreOffice Draw consumer", "skip ODG consumer", 1),
        raw.replace('command -v xvfb-run', 'echo skip-xvfb', 1),
    ]
    for index, mutation in enumerate(negative, 1):
        require(mutation != raw, f"Negative control {index} not applied")
        try:
            enforce(mutation)
        except AssertionError:
            continue
        raise AssertionError(f"Unsafe change escaped guard: negative control {index}")
    prove_mocked_fallback(raw)
    print(f"Cloud export Ubuntu mirror fallback: PASS, {len(negative)} negative controls, 4 mocked Bash paths")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
