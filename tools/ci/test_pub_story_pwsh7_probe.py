"""Verify the *actual* native PowerShell probe survives WinPS->pwsh transport.

No Publisher COM, PUB file, private paths, or native runner are required.
Hosted Windows executes the pinned wrapper's literal script through pwsh7.
"""
import argparse
import base64
from pathlib import Path
import re
import shutil
import subprocess

SOURCE = (
    Path(__file__).resolve().parents[2]
    / "tools/windows/pub-runtime/Invoke-StoryNativeRoundtripGuarded.ps1"
)
FOUR_FIELDS = re.compile(
    r"^7\.[0-9]+\.[0-9]+(?:\.[0-9]+)?\|Core\|(STA|MTA|Unknown)\|[01]$"
)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--static-only", action="store_true",
        help="validate source/encoding without starting a PowerShell child",
    )
    args = parser.parse_args()

    source = SOURCE.read_text(encoding="utf-8")
    matches = re.findall(
        r"(?m)^[ \t]*\$probeCommand[ \t]*=[ \t]*'([^'\r\n]+)'[ \t]*$",
        source,
    )
    if len(matches) != 1:
        raise SystemExit("pwsh7_literal_probe_source_missing_or_ambiguous")
    probe_script = matches[0]
    if 'Write-Output "$v|$e|$a|$b"' not in probe_script:
        raise SystemExit("pwsh7_probe_literal_pipe_output_missing")
    if (
        "$probeEncoded = [Convert]::ToBase64String("
        "[Text.Encoding]::Unicode.GetBytes($probeCommand))"
    ) not in source:
        raise SystemExit("pwsh7_probe_utf16le_encoding_missing")
    if not re.search(
        r"(?m)^[ \t]*\$probe[ \t]*=[ \t]*@\(&[ \t]*\$powershellExe"
        r"[ \t]+-NoLogo[ \t]+-NoProfile[ \t]+-NonInteractive"
        r"[ \t]+-EncodedCommand[ \t]+\$probeEncoded\)[ \t]*$",
        source,
    ):
        raise SystemExit("pwsh7_probe_encoded_process_invocation_missing")
    if "pwsh7_portable_version_or_arch_mismatch" not in source:
        raise SystemExit("pwsh7_pinned_runtime_gate_lost")

    encoded = base64.b64encode(probe_script.encode("utf-16-le")).decode("ascii")
    assert base64.b64decode(encoded).decode("utf-16-le") == probe_script
    if args.static_only:
        print("PASS pwsh7 literal/UTF-16LE probe source (runtime NOT evaluated)")
        return

    executable = shutil.which("pwsh")
    if executable is None:
        raise SystemExit("pwsh7_hosted_runtime_unavailable")
    command = [executable, "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded]
    result = subprocess.run(command, capture_output=True, text=True, timeout=30, check=False)
    output = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if result.returncode != 0 or len(output) != 1 or FOUR_FIELDS.fullmatch(output[0]) is None:
        # Avoid echoing host, profile or private information in public CI output.
        raise SystemExit("pwsh7_encoded_probe_four_field_roundtrip_failed")

    # Explicitly demonstrate that removing the quotes recreates the unsafe
    # parse shape, without printing the child output or any host metadata.
    invalid_script = probe_script.replace(
        'Write-Output "$v|$e|$a|$b"', "Write-Output $v|$e|$a|$b"
    )
    if invalid_script == probe_script:
        raise SystemExit("pwsh7_negative_control_not_constructed")
    invalid = base64.b64encode(invalid_script.encode("utf-16-le")).decode("ascii")
    bad = subprocess.run(
        [executable, "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", invalid],
        capture_output=True, text=True, timeout=30, check=False,
    )
    if bad.returncode == 0:
        raise SystemExit("pwsh7_unquoted_pipeline_negative_control_not_rejected")
    print("PASS pwsh7 quoted four-field probe and unquoted negative (no COM)")


if __name__ == "__main__":
    main()
