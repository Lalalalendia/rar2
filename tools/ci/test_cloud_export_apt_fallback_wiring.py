#!/usr/bin/env python3
"""Fail-closed wiring and negative controls for Cloud export's Ubuntu APT mirror fallback."""
import subprocess
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
    require('Acquire::Retries=1' in script and 'Acquire::http::Timeout=15' in script and 'Acquire::https::Timeout=15' in script, "Short APT network retry/timeout policy removed")
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
    print(f"Cloud export Ubuntu mirror fallback: PASS, {len(negative)} negative controls")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
