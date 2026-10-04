#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import shutil
import zipfile
from pathlib import Path

WITNESSES = [
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
]

COPY_MAP = [
    "tools/research-runner/experiments/quill-story-readonly-oracle-02.packet.json",
    "tools/research-runner/operations/quill_story_readonly_oracle_02.ps1",
    "tools/research-runner/prepare_native_run.ps1",
    "tools/research-runner/finalize_native_run.ps1",
    "tools/windows/pub-runtime/PubRuntime.psm1",
]

LAUNCHER = r'''param([string]$OutputRoot = "")

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Root = (Resolve-Path $PSScriptRoot).Path
$WitnessRoot = Join-Path $Root "witnesses"
$Packet = Join-Path $Root "tools\research-runner\experiments\quill-story-readonly-oracle-02.packet.json"
$Operation = Join-Path $Root "tools\research-runner\operations\quill_story_readonly_oracle_02.ps1"
$Prepare = Join-Path $Root "tools\research-runner\prepare_native_run.ps1"
$Finalize = Join-Path $Root "tools\research-runner\finalize_native_run.ps1"

$Witnesses = @(
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff"
)

foreach ($sha in $Witnesses) {
    $path = Join-Path $WitnessRoot "$sha.pub"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Bundled witness missing: $sha.pub"
    }
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $sha) {
        throw "Bundled witness SHA mismatch for $sha"
    }
}

$manifest = Get-Content -LiteralPath (Join-Path $Root "BUNDLE-MANIFEST.json") -Raw | ConvertFrom-Json
foreach ($file in @($manifest.files)) {
    $path = Join-Path $Root ([string]$file.path)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Bundle file missing: $($file.path)"
    }
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne ([string]$file.sha256).ToLowerInvariant()) {
        throw "Bundle file SHA mismatch: $($file.path)"
    }
}

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $OutputRoot = Join-Path $Root "results\$stamp"
} elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $Root $OutputRoot
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

$oldFixture = [string]$env:PUB_RESEARCH_FIXTURE_ROOT
$oldProfile = [string]$env:PUB_RESEARCH_PROFILE_ID
try {
    $env:PUB_RESEARCH_FIXTURE_ROOT = $WitnessRoot
    $env:PUB_RESEARCH_PROFILE_ID = "publisher-2019"

    Write-Host "1/3 Verify exact Publisher2019 environment"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Prepare -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "prepare_native_run.ps1 failed with exit code $LASTEXITCODE" }

    Write-Host "2/3 Run exact-four read-only Publisher Story oracle"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "quill_story_readonly_oracle_02.ps1 failed with exit code $LASTEXITCODE" }

    Write-Host "3/3 Finalize source-safe evidence"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "finalize_native_run.ps1 failed with exit code $LASTEXITCODE" }

    $oracle = Join-Path $OutputRoot "analysis\quill-story-readonly-oracle.json"
    $evidence = Join-Path $OutputRoot "evidence-manifest.json"
    $environment = Join-Path $OutputRoot "environment.json"
    $log = Join-Path $OutputRoot "logs\quill-story-readonly-oracle.txt"
    foreach ($path in @($oracle, $evidence, $environment, $log)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Expected evidence missing: $path"
        }
    }

    $return = Join-Path $OutputRoot "return"
    New-Item -ItemType Directory -Force -Path (Join-Path $return "analysis") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $return "logs") | Out-Null
    Copy-Item -LiteralPath $oracle -Destination (Join-Path $return "analysis\quill-story-readonly-oracle.json")
    Copy-Item -LiteralPath $evidence -Destination (Join-Path $return "evidence-manifest.json")
    Copy-Item -LiteralPath $environment -Destination (Join-Path $return "environment.json")
    Copy-Item -LiteralPath $log -Destination (Join-Path $return "logs\quill-story-readonly-oracle.txt")

    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $zip = Join-Path $Root "RETURN-TO-CHAT-$stamp.zip"
    Compress-Archive -Path (Join-Path $return "*") -DestinationPath $zip -CompressionLevel Optimal -Force

    Write-Host ""
    Write-Host "PASS"
    Write-Host "Upload this file back to ChatGPT:"
    Write-Host $zip
    Write-Host ""
    Get-Content -LiteralPath $oracle -Raw
}
finally {
    if ([string]::IsNullOrWhiteSpace($oldFixture)) {
        Remove-Item Env:PUB_RESEARCH_FIXTURE_ROOT -ErrorAction SilentlyContinue
    } else {
        $env:PUB_RESEARCH_FIXTURE_ROOT = $oldFixture
    }
    if ([string]::IsNullOrWhiteSpace($oldProfile)) {
        Remove-Item Env:PUB_RESEARCH_PROFILE_ID -ErrorAction SilentlyContinue
    } else {
        $env:PUB_RESEARCH_PROFILE_ID = $oldProfile
    }
}
'''

CMD = """@echo off\r
setlocal\r
cd /d "%~dp0"\r
echo Quill Story read-only Publisher2019 oracle\r
echo.\r
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0RUN_NATIVE.ps1"\r
set RC=%ERRORLEVEL%\r
echo.\r
if exist "%~dp0RETURN-TO-CHAT-*.zip" echo Upload RETURN-TO-CHAT-*.zip back to ChatGPT.\r
echo Exit code: %RC%\r
pause\r
exit /b %RC%\r
"""

README = """QUILL STORY READ-ONLY NATIVE HANDOFF
=====================================

1. Extract this ZIP to a writable local folder on the Publisher2019 machine.
2. Double-click RUN_NATIVE.cmd.
3. Do not manually open Publisher while it runs.
4. When PASS appears, upload RETURN-TO-CHAT-<timestamp>.zip back to ChatGPT.

No local rar2 repository, Git, Cargo/Rust, Python or network is required.
The four exact PUB witnesses are bundled and SHA-verified.
The operation opens only temporary copies read-only; no Save/SaveAs/Print/Export/macros.
The return ZIP contains source-safe JSON/log receipts only, not PUB files or document text.
"""


def main() -> int:
    root = Path("out/QUILL-STORY-READONLY-NATIVE-HANDOFF")
    if root.exists():
        shutil.rmtree(root)
    (root / "witnesses").mkdir(parents=True)
    for p in [
        "tools/research-runner/experiments",
        "tools/research-runner/operations",
        "tools/research-runner",
        "tools/windows/pub-runtime",
    ]:
        (root / p).mkdir(parents=True, exist_ok=True)

    src_root = Path("_witness/lalamu/pub-corpus/corpus/native/unclassified")
    for sha in WITNESSES:
        src = src_root / f"{sha}.pub"
        data = src.read_bytes()
        if hashlib.sha256(data).hexdigest() != sha:
            raise RuntimeError(f"Witness SHA mismatch for {sha}")
        (root / "witnesses" / f"{sha}.pub").write_bytes(data)

    for rel in COPY_MAP:
        shutil.copy2(rel, root / rel)

    (root / "RUN_NATIVE.ps1").write_text(LAUNCHER, encoding="utf-8-sig")
    (root / "RUN_NATIVE.cmd").write_text(CMD, encoding="ascii")
    (root / "README.txt").write_text(README, encoding="utf-8-sig")

    files = []
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.name != "BUNDLE-MANIFEST.json"):
        data = path.read_bytes()
        files.append({
            "path": path.relative_to(root).as_posix(),
            "size": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })

    manifest = {
        "schema": "chaptera.quill-story-native-handoff.v1",
        "lalamu_commit": "f78cc6f455f4dc222868f9cc035511a6ca7a91ea",
        "witness_count": 4,
        "requires_local_repo": False,
        "requires_network": False,
        "files": files,
    }
    (root / "BUNDLE-MANIFEST.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )

    out_zip = Path("out/QUILL-STORY-READONLY-NATIVE-HANDOFF.zip")
    out_zip.parent.mkdir(parents=True, exist_ok=True)
    if out_zip.exists():
        out_zip.unlink()
    with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for path in sorted(p for p in root.rglob("*") if p.is_file()):
            z.write(path, arcname=(root.name + "/" + path.relative_to(root).as_posix()))

    summary = {
        "file": out_zip.name,
        "size": out_zip.stat().st_size,
        "sha256": hashlib.sha256(out_zip.read_bytes()).hexdigest(),
    }
    Path("out/native-handoff-summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
