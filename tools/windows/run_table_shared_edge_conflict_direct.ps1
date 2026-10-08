param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $RepoRoot

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/table-shared-edge-conflict-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/publisher_table_shared_edge_conflict_01.ps1"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"

$ExpectedPacketBlob = "ae886fd719a51a197fbdb406ba2125c35728f5ee"
$ExpectedOperationBlob = "25eb09c421ca8c91fb3e4e4a7201597daac8eae3"
$ExpectedRuntimeBlob = "fed4c890a34d39401d3b5848cc16d1087f862a27"
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

function Assert-GitBlob {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required file missing: $Path"
    }

    $actual = (& git hash-object -- $Path).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "git hash-object failed for $Path"
    }
    if ($actual -ne $Expected) {
        throw "Pinned file drift: $Path expected $Expected got $actual"
    }
}

Assert-GitBlob -Path $Packet -Expected $ExpectedPacketBlob
Assert-GitBlob -Path $Operation -Expected $ExpectedOperationBlob
Assert-GitBlob -Path $Runtime -Expected $ExpectedRuntimeBlob

Import-Module $Runtime -Force
$publisher = Get-PubPublisherIdentity
if (-not $publisher.available) {
    throw "Publisher COM is unavailable."
}
if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) {
    throw "Publisher Version mismatch: expected $ExpectedPublisherVersion"
}
if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) {
    throw "Publisher Build mismatch: expected $ExpectedPublisherBuild"
}
if ($publisher.path.state -ne "value") {
    throw "Publisher executable directory is unavailable."
}

$publisherExe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
if (-not (Test-Path -LiteralPath $publisherExe -PathType Leaf)) {
    throw "Publisher executable missing at COM-reported path."
}

$fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($publisherExe).FileVersion
if ([string]$fileVersion -ne $ExpectedPublisherFileVersion) {
    throw "MSPUB.EXE file version mismatch: expected $ExpectedPublisherFileVersion got $fileVersion"
}

$exeSha = (Get-FileHash -LiteralPath $publisherExe -Algorithm SHA256).Hash.ToLowerInvariant()
if ($exeSha -ne $ExpectedPublisherExeSha256) {
    throw "MSPUB.EXE SHA mismatch: expected $ExpectedPublisherExeSha256 got $exeSha"
}

python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/table-shared-edge-conflict-01.packet.json" --expected-environment publisher-2019
if ($LASTEXITCODE -ne 0) {
    throw "Packet validation failed."
}

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot "out/pub-research/table-shared-edge-conflict-01-direct"
} elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot $OutputRoot
}

if (Test-Path -LiteralPath $OutputRoot) {
    $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
    if ($existing.Count -ne 0) {
        throw "OutputRoot is not empty; use a fresh directory: $OutputRoot"
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) {
    throw "prepare_native_run failed."
}

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) {
    throw "TABLE shared-edge operation failed."
}

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) {
    throw "finalize_native_run failed."
}

$result = Join-Path $OutputRoot "analysis/table-shared-edge-conflict-01.json"
$log = Join-Path $OutputRoot "logs/table-shared-edge-conflict-01.txt"
foreach ($path in @($result, $log, (Join-Path $OutputRoot "environment.json"), (Join-Path $OutputRoot "evidence-manifest.json"))) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required evidence missing: $path"
    }
}

$receipt = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
$allowedVerdicts = @(
    "single_edge_last_mutation_wins",
    "deterministic_winner_independent_of_order",
    "multiple_carriers_same_edge",
    "separate_or_conflicting_side_state",
    "single_edge_aliasing_conflict_rule_unknown",
    "canonical_edge_identity_not_proven",
    "raw_diff_ambiguous",
    "control_not_clean",
    "not_evaluable"
)
if ($allowedVerdicts -notcontains [string]$receipt.verdict) {
    throw "Unexpected verdict: $($receipt.verdict)"
}

Write-Host ""
Write-Host "TABLE-SHARED-EDGE-CONFLICT-01 completed."
Write-Host "Verdict: $($receipt.verdict)"
Write-Host "Analysis: $result"
Write-Host "Log: $log"
Get-Content -LiteralPath $result -Raw
