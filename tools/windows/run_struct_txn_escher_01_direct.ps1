param(
    [Parameter(Mandatory = $true)][string]$InputPath,
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/struct-txn-escher-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/struct_txn_escher_01.ps1"
$HelperSource = Join-Path $RepoRoot "vendor/producer-a/crates/pub-reader/src/bin/structural_base_manifest.rs"
$StructuralModule = Join-Path $RepoRoot "vendor/producer-a/crates/pub-reader/src/structural_base.rs"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"
$Finalize = Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1"

$ExpectedPacketBlob = "undefined"
$ExpectedOperationBlob = "undefined"
$ExpectedHelperSourceBlob = "59f0cc84f2730fb58249800512abbd13f7f75c79"
$ExpectedStructuralModuleBlob = "50ae3b989446f07e62838f39bef073c4bbea0c33"
$ExpectedRuntimeBlob = "fed4c890a34d39401d3b5848cc16d1087f862a27"
$ExpectedFinalizeBlob = "2a97d6f2c8be1265010a744c015ce8d288eb7e75"

function Assert-GitBlob {
    param([string]$Path,[string]$Expected)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Required file missing: $Path" }
    $actual = (& git hash-object -- $Path).Trim()
    if ($LASTEXITCODE -ne 0) { throw "git hash-object failed for $Path" }
    if ($actual -ne $Expected) { throw "Pinned file drift: $Path expected $Expected got $actual" }
}

function Assert-Publisher2019 {
    Import-Module $Runtime -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher COM is unavailable." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) { throw "Publisher Version mismatch." }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) { throw "Publisher Build mismatch." }
    if ($publisher.path.state -ne "value") { throw "Publisher executable directory is unavailable." }
    $publisherExe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
    if (-not (Test-Path -LiteralPath $publisherExe -PathType Leaf)) { throw "MSPUB.EXE missing." }
    $fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($publisherExe).FileVersion
    if ([string]$fileVersion -ne $ExpectedPublisherFileVersion) { throw "MSPUB.EXE file version mismatch." }
    $exeSha = (Get-FileHash -LiteralPath $publisherExe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($exeSha -ne $ExpectedPublisherExeSha256) { throw "MSPUB.EXE SHA mismatch." }
}

Push-Location $RepoRoot
try {
    Assert-GitBlob $Packet $ExpectedPacketBlob
    Assert-GitBlob $Operation $ExpectedOperationBlob
    Assert-GitBlob $HelperSource $ExpectedHelperSourceBlob
    Assert-GitBlob $StructuralModule $ExpectedStructuralModuleBlob
    Assert-GitBlob $Runtime $ExpectedRuntimeBlob
    Assert-GitBlob $Finalize $ExpectedFinalizeBlob

    $resolvedInput = if ([IO.Path]::IsPathRooted($InputPath)) { $InputPath } else { Join-Path $RepoRoot $InputPath }
    if (-not (Test-Path -LiteralPath $resolvedInput -PathType Leaf)) { throw "InputPath does not exist: $resolvedInput" }
    $resolvedInput = (Resolve-Path -LiteralPath $resolvedInput).Path
    $inputSha = (Get-FileHash -LiteralPath $resolvedInput -Algorithm SHA256).Hash.ToLowerInvariant()

    Assert-Publisher2019

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot "out/pub-research/struct-txn-escher-01-direct"
    } elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot $OutputRoot
    }
    if (Test-Path -LiteralPath $OutputRoot) {
        $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
        if ($existing.Count -ne 0) { throw "OutputRoot is not empty: $OutputRoot" }
    }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

    $env:PUB_RESEARCH_FIXTURE = $resolvedInput

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "STRUCT-TXN-ESCHER operation failed with exit code $LASTEXITCODE" }

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "finalize_native_run.ps1 failed with exit code $LASTEXITCODE" }

    $analysis = Join-Path $OutputRoot "analysis/struct-txn-escher-01.json"
    $result = Get-Content -LiteralPath $analysis -Raw | ConvertFrom-Json
    if ([string]$result.schema -ne "chaptera.struct-txn-escher-01.v1") { throw "Unexpected result schema." }
    if ([string]$result.experiment_id -ne "STRUCT-TXN-ESCHER-01") { throw "Unexpected experiment id." }
    if ([string]$result.source.admitted_base_sha256 -ne $inputSha) { throw "Result base SHA does not match input." }
    if (-not [bool]$result.source.t370_semantic_contract_match) { throw "T370 semantic contract was not admitted." }
    if (-not [bool]$result.source.original_unchanged) { throw "Source input changed during T351." }
    if (-not [bool]$result.claims.one_new_ordinary_non_text_shape) { throw "Missing bounded AddShape lifecycle." }
    if (-not [bool]$result.claims.save_close_fresh_reopen) { throw "Missing save/close/reopen proof." }
    if ([bool]$result.claims.source_original_mutated -or [bool]$result.claims.generated_pub_uploaded) { throw "Privacy/source fence violation." }

    Write-Host ""
    Write-Host "STRUCT-TXN-ESCHER direct run completed."
    Write-Host "Verdict: $($result.verdict)"
    Write-Host "Analysis: $analysis"
}
finally {
    Pop-Location
}
