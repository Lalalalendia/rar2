param(
    [string]$InputPath = "",
    [string]$BaseReceiptPath = "",
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedBaseSha256 = "905bf75b00c0ff8680f61a20d5df4d843d753d3544a233fd237dc5a38a0a0599"
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

$ExpectedPacketBlob = "1fac96af8893f3706746aaf57dd033a82c949c55"
$ExpectedOperationBlob = "f7009dd5712393ce49ad4ba6f42a598d29034462"
$ExpectedHelperSourceBlob = "1afe88d65eed91d2b73c3814e6997b29572ecc4c"
$ExpectedStructuralModuleBlob = "50ae3b989446f07e62838f39bef073c4bbea0c33"
$ExpectedRuntimeBlob = "fed4c890a34d39401d3b5848cc16d1087f862a27"
$ExpectedFinalizeBlob = "2a97d6f2c8be1265010a744c015ce8d288eb7e75"

function Assert-GitBlob {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Expected
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required file missing: $Path"
    }
    $actual = (& git hash-object -- $Path).Trim()
    if ($LASTEXITCODE -ne 0) { throw "git hash-object failed for $Path" }
    if ($actual -ne $Expected) {
        throw "Pinned file drift: $Path expected $Expected got $actual"
    }
}

function Assert-Publisher2019 {
    Import-Module $Runtime -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher COM is unavailable." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) {
        throw "Publisher Version mismatch: expected $ExpectedPublisherVersion"
    }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) {
        throw "Publisher Build mismatch: expected $ExpectedPublisherBuild"
    }
    if ($publisher.path.state -ne "value") { throw "Publisher executable directory is unavailable." }

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
}

function Find-ExactBase {
    if (-not [string]::IsNullOrWhiteSpace($InputPath)) {
        $candidate = if ([IO.Path]::IsPathRooted($InputPath)) { $InputPath } else { Join-Path $RepoRoot $InputPath }
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "InputPath does not exist: $candidate" }
        $sha = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($sha -ne $ExpectedBaseSha256) { throw "InputPath is not the exact T370 base." }
        return (Resolve-Path -LiteralPath $candidate).Path
    }

    $default = Join-Path $RepoRoot "out/pub-research/modern-base-pin-01/private/modern-base-pin-01/working.pub"
    if (Test-Path -LiteralPath $default -PathType Leaf) {
        $sha = (Get-FileHash -LiteralPath $default -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($sha -eq $ExpectedBaseSha256) { return (Resolve-Path -LiteralPath $default).Path }
    }

    $searchRoot = Join-Path $RepoRoot "out/pub-research"
    if (-not (Test-Path -LiteralPath $searchRoot -PathType Container)) {
        throw "Exact T370 base not found. Pass -InputPath to SHA $ExpectedBaseSha256."
    }
    $matches = @()
    foreach ($file in Get-ChildItem -LiteralPath $searchRoot -Recurse -File -Filter "*.pub" -ErrorAction SilentlyContinue) {
        try {
            $sha = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($sha -eq $ExpectedBaseSha256) { $matches += $file.FullName }
        } catch {}
    }
    $matches = @($matches | Sort-Object -Unique)
    if ($matches.Count -ne 1) {
        throw "Expected exactly one exact T370 base under out/pub-research; found $($matches.Count). Pass -InputPath explicitly."
    }
    return (Resolve-Path -LiteralPath $matches[0]).Path
}

function Find-ExactBaseReceipt {
    if (-not [string]::IsNullOrWhiteSpace($BaseReceiptPath)) {
        $candidate = if ([IO.Path]::IsPathRooted($BaseReceiptPath)) { $BaseReceiptPath } else { Join-Path $RepoRoot $BaseReceiptPath }
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "BaseReceiptPath does not exist: $candidate" }
        return (Resolve-Path -LiteralPath $candidate).Path
    }

    $default = Join-Path $RepoRoot "out/pub-research/modern-base-pin-01/analysis/modern-base-pin-01.json"
    if (Test-Path -LiteralPath $default -PathType Leaf) { return (Resolve-Path -LiteralPath $default).Path }

    $searchRoot = Join-Path $RepoRoot "out/pub-research"
    if (-not (Test-Path -LiteralPath $searchRoot -PathType Container)) {
        throw "T370 receipt not found. Pass -BaseReceiptPath."
    }
    $matches = @(
        Get-ChildItem -LiteralPath $searchRoot -Recurse -File -Filter "modern-base-pin-01.json" -ErrorAction SilentlyContinue |
            ForEach-Object {
                try {
                    $receipt = Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
                    if (
                        [string]$receipt.schema -eq "chaptera.modern-base-pin.v1" -and
                        [string]$receipt.native_lineage.post_save_sha256 -eq $ExpectedBaseSha256
                    ) { $_.FullName }
                } catch {}
            }
    )
    $matches = @($matches | Sort-Object -Unique)
    if ($matches.Count -ne 1) {
        throw "Expected exactly one T370 receipt bound to the exact base; found $($matches.Count). Pass -BaseReceiptPath explicitly."
    }
    return (Resolve-Path -LiteralPath $matches[0]).Path
}

Push-Location $RepoRoot
try {
    Assert-GitBlob -Path $Packet -Expected $ExpectedPacketBlob
    Assert-GitBlob -Path $Operation -Expected $ExpectedOperationBlob
    Assert-GitBlob -Path $HelperSource -Expected $ExpectedHelperSourceBlob
    Assert-GitBlob -Path $StructuralModule -Expected $ExpectedStructuralModuleBlob
    Assert-GitBlob -Path $Runtime -Expected $ExpectedRuntimeBlob
    Assert-GitBlob -Path $Finalize -Expected $ExpectedFinalizeBlob

    $resolvedBase = Find-ExactBase
    $resolvedBaseReceipt = Find-ExactBaseReceipt
    Assert-Publisher2019

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot "out/pub-research/struct-txn-escher-01-direct"
    } elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot $OutputRoot
    }
    if (Test-Path -LiteralPath $OutputRoot) {
        $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
        if ($existing.Count -ne 0) {
            throw "OutputRoot is not empty; use a fresh directory to avoid mixing evidence: $OutputRoot"
        }
    }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

    $env:PUB_RESEARCH_FIXTURE = $resolvedBase
    $env:PUB_RESEARCH_MODERN_BASE_RECEIPT = $resolvedBaseReceipt

    Write-Host "STRUCT-TXN-ESCHER direct run: exact T370 base/receipt and Publisher2019 identity verified."
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "struct_txn_escher_01.ps1 failed with exit code $LASTEXITCODE" }

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "finalize_native_run.ps1 failed with exit code $LASTEXITCODE" }

    $analysis = Join-Path $OutputRoot "analysis/struct-txn-escher-01.json"
    $log = Join-Path $OutputRoot "logs/struct-txn-escher-01.txt"
    $environment = Join-Path $OutputRoot "environment.json"
    $manifest = Join-Path $OutputRoot "evidence-manifest.json"
    foreach ($path in @($analysis,$log,$environment,$manifest)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required T351 evidence missing: $path"
        }
    }

    $result = Get-Content -LiteralPath $analysis -Raw | ConvertFrom-Json
    if ([string]$result.schema -ne "chaptera.struct-txn-escher-01.v1") {
        throw "Unexpected T351 receipt schema: $($result.schema)"
    }
    if ([string]$result.experiment_id -ne "STRUCT-TXN-ESCHER-01") {
        throw "Unexpected T351 experiment identity: $($result.experiment_id)"
    }
    if ([string]$result.source.admitted_base_sha256 -ne $ExpectedBaseSha256 -or -not [bool]$result.source.original_unchanged) {
        throw "T351 receipt does not preserve exact source identity."
    }
    if (-not [bool]$result.claims.exact_t370_base_bound -or -not [bool]$result.claims.exact_t370_receipt_bound) {
        throw "T351 receipt did not bind both T370 base and receipt."
    }
    if (-not [bool]$result.claims.one_new_ordinary_non_text_shape -or -not [bool]$result.claims.save_close_fresh_reopen) {
        throw "T351 receipt is missing the bounded AddShape lifecycle."
    }
    if ([bool]$result.claims.source_original_mutated -or [bool]$result.claims.generated_pub_uploaded) {
        throw "T351 receipt violates source/private-artifact fences."
    }

    $allowed = @(
        "bounded_creation_materialization_captured",
        "structural_candidate_found_but_shape_id_join_differs",
        "structural_target_not_uniquely_joined"
    )
    if ($allowed -notcontains [string]$result.verdict) {
        throw "Unexpected T351 verdict: $($result.verdict)"
    }

    Write-Host ""
    Write-Host "STRUCT-TXN-ESCHER direct run completed."
    Write-Host "Analysis: $analysis"
    Write-Host "Manifest: $manifest"
    Write-Host "Verdict: $($result.verdict)"
    Get-Content -LiteralPath $analysis -Raw
}
finally {
    Pop-Location
}
