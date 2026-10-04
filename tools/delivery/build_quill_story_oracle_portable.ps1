param(
    [Parameter(Mandatory = $true)]
    [string]$WitnessRoot,
    [Parameter(Mandatory = $true)]
    [string]$ResolverExe,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$bundleRoot = Join-Path $OutputRoot "QUILL-STORY-READONLY-ORACLE-PORTABLE"
if (Test-Path -LiteralPath $bundleRoot) {
    Remove-Item -LiteralPath $bundleRoot -Recurse -Force
}

$dirs = @(
    "witnesses",
    "bin",
    "tools/research-runner/experiments",
    "tools/research-runner/operations",
    "tools/research-runner",
    "tools/windows/pub-runtime"
)
foreach ($relative in $dirs) {
    New-Item -ItemType Directory -Force -Path (Join-Path $bundleRoot $relative) | Out-Null
}

$witnesses = @(
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff"
)

foreach ($sha in $witnesses) {
    $source = Join-Path $WitnessRoot "$sha.pub"
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Witness missing: $source"
    }
    $actual = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $sha) {
        throw "Witness SHA mismatch for $sha"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $bundleRoot "witnesses/$sha.pub")
}

if (-not (Test-Path -LiteralPath $ResolverExe -PathType Leaf)) {
    throw "Resolver EXE missing: $ResolverExe"
}
Copy-Item -LiteralPath $ResolverExe -Destination (Join-Path $bundleRoot "bin/quill-story-readonly-oracle-resolver.exe")

$copyMap = @{
    "tools/research-runner/experiments/quill-story-readonly-oracle-02.packet.json" = "tools/research-runner/experiments/quill-story-readonly-oracle-02.packet.json"
    "tools/research-runner/operations/quill_story_readonly_oracle_02.ps1" = "tools/research-runner/operations/quill_story_readonly_oracle_02.ps1"
    "tools/research-runner/prepare_native_run.ps1" = "tools/research-runner/prepare_native_run.ps1"
    "tools/research-runner/finalize_native_run.ps1" = "tools/research-runner/finalize_native_run.ps1"
    "tools/windows/pub-runtime/PubRuntime.psm1" = "tools/windows/pub-runtime/PubRuntime.psm1"
}
foreach ($entry in $copyMap.GetEnumerator()) {
    $source = Join-Path $repoRoot $entry.Key
    $target = Join-Path $bundleRoot $entry.Value
    Copy-Item -LiteralPath $source -Destination $target -Force
}

$launcher = @'
param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = (Resolve-Path $PSScriptRoot).Path
$witnessRoot = Join-Path $root "witnesses"
$packet = Join-Path $root "tools\research-runner\experiments\quill-story-readonly-oracle-02.packet.json"
$operation = Join-Path $root "tools\research-runner\operations\quill_story_readonly_oracle_02.ps1"
$prepare = Join-Path $root "tools\research-runner\prepare_native_run.ps1"
$finalize = Join-Path $root "tools\research-runner\finalize_native_run.ps1"
$resolver = Join-Path $root "bin\quill-story-readonly-oracle-resolver.exe"

$witnesses = @(
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff"
)

foreach ($sha in $witnesses) {
    $path = Join-Path $witnessRoot "$sha.pub"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Exact bundled witness missing: $sha.pub"
    }
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $sha) {
        throw "Exact bundled witness SHA mismatch for $sha"
    }
}

$manifestPath = Join-Path $root "BUNDLE-MANIFEST.json"
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
foreach ($file in @($manifest.files)) {
    $path = Join-Path $root ([string]$file.path)
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
    $OutputRoot = Join-Path $root "results\$stamp"
} elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $root $OutputRoot
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

$previousFixtureRoot = [string]$env:PUB_RESEARCH_FIXTURE_ROOT
$previousProfileId = [string]$env:PUB_RESEARCH_PROFILE_ID
try {
    $env:PUB_RESEARCH_FIXTURE_ROOT = $witnessRoot
    $env:PUB_RESEARCH_PROFILE_ID = "publisher-2019"

    Write-Host "1/4 Validate Publisher2019 environment"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $prepare -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "prepare_native_run.ps1 failed: $LASTEXITCODE" }

    Write-Host "2/4 Run read-only Publisher oracle"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $operation -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "quill_story_readonly_oracle_02.ps1 failed: $LASTEXITCODE" }

    Write-Host "3/4 Finalize source-safe evidence"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $finalize -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "finalize_native_run.ps1 failed: $LASTEXITCODE" }

    $oracle = Join-Path $OutputRoot "analysis\quill-story-readonly-oracle.json"
    $resolution = Join-Path $OutputRoot "analysis\quill-story-readonly-oracle-resolution.json"
    Write-Host "4/4 Resolve Story partitions"
    & $resolver $witnessRoot $oracle $resolution
    if ($LASTEXITCODE -ne 0) { throw "resolver failed: $LASTEXITCODE" }

    $report = Get-Content -LiteralPath $resolution -Raw | ConvertFrom-Json
    if ([int]$report.witness_count -ne 4) {
        throw "Resolver witness_count mismatch: $($report.witness_count)"
    }

    $crossChecks = [ordered]@{
        "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a" = @(11, 12, 13, 25)
        "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569" = @(0, 1, 45)
    }
    foreach ($sha in $crossChecks.Keys) {
        $rows = @($report.rows | Where-Object { $_.source_sha256 -eq $sha })
        if ($rows.Count -ne 1) {
            throw "Missing historical cross-check row for $sha"
        }
        if ([bool]$rows[0].unique_hash_partition) {
            $actual = @($rows[0].terminal_fdpp_ordinals_zero_based | ForEach-Object { [int]$_ })
            $expected = @($crossChecks[$sha])
            if (($actual -join ",") -ne ($expected -join ",")) {
                throw "Historical donor cross-check mismatch for $sha"
            }
        }
    }

    $returnDir = Join-Path $OutputRoot "return"
    New-Item -ItemType Directory -Force -Path $returnDir | Out-Null
    foreach ($relative in @(
        "environment.json",
        "evidence-manifest.json",
        "analysis\quill-story-readonly-oracle.json",
        "analysis\quill-story-readonly-oracle-resolution.json",
        "logs\quill-story-readonly-oracle.txt"
    )) {
        $source = Join-Path $OutputRoot $relative
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Expected return evidence missing: $relative"
        }
        $target = Join-Path $returnDir $relative
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
        Copy-Item -LiteralPath $source -Destination $target -Force
    }

    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $returnZip = Join-Path $root "RETURN-TO-CHAT-$stamp.zip"
    if (Test-Path -LiteralPath $returnZip) { Remove-Item -LiteralPath $returnZip -Force }
    Compress-Archive -Path (Join-Path $returnDir "*") -DestinationPath $returnZip -CompressionLevel Optimal

    Write-Host ""
    Write-Host "PASS"
    Write-Host "Return this file to chat:"
    Write-Host $returnZip
    Write-Host ""
    Get-Content -LiteralPath $resolution -Raw
}
finally {
    if ([string]::IsNullOrWhiteSpace($previousFixtureRoot)) {
        Remove-Item Env:PUB_RESEARCH_FIXTURE_ROOT -ErrorAction SilentlyContinue
    } else {
        $env:PUB_RESEARCH_FIXTURE_ROOT = $previousFixtureRoot
    }
    if ([string]::IsNullOrWhiteSpace($previousProfileId)) {
        Remove-Item Env:PUB_RESEARCH_PROFILE_ID -ErrorAction SilentlyContinue
    } else {
        $env:PUB_RESEARCH_PROFILE_ID = $previousProfileId
    }
}
'@
Set-Content -LiteralPath (Join-Path $bundleRoot "RUN_NATIVE.ps1") -Value $launcher -Encoding UTF8

$cmd = @'
@echo off
setlocal
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0RUN_NATIVE.ps1"
set EXITCODE=%ERRORLEVEL%
echo.
if not "%EXITCODE%"=="0" (
  echo FAILED with exit code %EXITCODE%
) else (
  echo Finished successfully.
)
echo.
pause
exit /b %EXITCODE%
'@
Set-Content -LiteralPath (Join-Path $bundleRoot "RUN_NATIVE.cmd") -Value $cmd -Encoding ASCII

$readme = @'
QUILL STORY READ-ONLY ORACLE — PORTABLE

Purpose
-------
Run the exact #337 read-only Publisher2019 oracle on the four bundled SHA-pinned PUB witnesses.

Requirements
------------
- Windows with Microsoft Publisher 2019 exact build already installed.
- No Git checkout.
- No Cargo/Rust.
- No Python.
- No network access is required.

Run
---
1. Extract this ZIP to a normal writable folder.
2. Double-click RUN_NATIVE.cmd.
3. Do not open/edit the bundled PUB files manually.
4. When the script says PASS, return the generated RETURN-TO-CHAT-<timestamp>.zip.

Safety / evidence boundary
--------------------------
- Publisher opens only temporary copies of the four bundled PUB witnesses.
- No Save, SaveAs, Print, Export, macro execution or Trust Center changes.
- Every witness SHA-256 is checked before execution.
- Installed MSPUB.EXE version prefix and SHA-256 are checked by the existing merged prepare_native_run.ps1.
- The returned ZIP contains source-safe receipts only. It does not contain PUB files or document text.
'@
Set-Content -LiteralPath (Join-Path $bundleRoot "README.txt") -Value $readme -Encoding UTF8

$manifestFiles = @()
Get-ChildItem -LiteralPath $bundleRoot -File -Recurse |
    Where-Object { $_.Name -ne "BUNDLE-MANIFEST.json" } |
    Sort-Object FullName |
    ForEach-Object {
        $relative = $_.FullName.Substring($bundleRoot.Length).TrimStart("\","/").Replace("\","/")
        $manifestFiles += [ordered]@{
            path = $relative
            size = [int64]$_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }

$manifest = [ordered]@{
    schema = "chaptera.quill-story-portable-bundle.v1"
    source_main_sha = $env:GITHUB_SHA
    lalamu_commit = "f78cc6f455f4dc222868f9cc035511a6ca7a91ea"
    witness_count = 4
    files = $manifestFiles
    evidence_boundary = "Delivery-only bundle assembled from already-merged Quill read-only authority. It changes no semantic law and requires no local Git repository."
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $bundleRoot "BUNDLE-MANIFEST.json") -Encoding UTF8

$zipPath = Join-Path $OutputRoot "QUILL-STORY-READONLY-ORACLE-PORTABLE.zip"
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }
Compress-Archive -Path (Join-Path $bundleRoot "*") -DestinationPath $zipPath -CompressionLevel Optimal

$zipHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
$summary = [ordered]@{
    schema = "chaptera.quill-story-portable-bundle-build.v1"
    bundle = [IO.Path]::GetFileName($zipPath)
    sha256 = $zipHash
    size = (Get-Item -LiteralPath $zipPath).Length
    witness_count = 4
}
$summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputRoot "bundle-summary.json") -Encoding UTF8
Get-Content -LiteralPath (Join-Path $OutputRoot "bundle-summary.json") -Raw
