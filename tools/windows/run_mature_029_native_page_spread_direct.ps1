param(
    [string]$InputPath = "",
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedSourceSha256 = "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e"
$ExpectedSourceBytes = 496640
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/mature_029_native_page_spread_oracle_01.ps1"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"
$ExpectedOperationBlob = "5adbaba0f1bbea56ef2b5a20bbfef3258b3b955c"
$ExpectedRuntimeBlob = "fed4c890a34d39401d3b5848cc16d1087f862a27"

Push-Location $RepoRoot
try {
    foreach ($entry in @(
        [pscustomobject]@{ Path = $Operation; Sha = $ExpectedOperationBlob }
        [pscustomobject]@{ Path = $Runtime; Sha = $ExpectedRuntimeBlob }
    )) {
        if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) {
            throw "Required file missing: $($entry.Path)"
        }
        $actualBlob = (& git hash-object -- $entry.Path).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw "git hash-object failed for $($entry.Path)"
        }
        if ($actualBlob -ne [string]$entry.Sha) {
            throw "Pinned file drift: $($entry.Path) expected $($entry.Sha) got $actualBlob"
        }
    }

    if ([string]::IsNullOrWhiteSpace($InputPath)) {
        $defaultInput = Join-Path $RepoRoot "out/pinned1050/materialized/native/$ExpectedSourceSha256.pub"
        if (-not (Test-Path -LiteralPath $defaultInput -PathType Leaf)) {
            throw "Exact 029 source is not present at the default offline path. Pass -InputPath to a local exact copy; do not download or substitute another PUB."
        }
        $InputPath = $defaultInput
    } elseif (-not [System.IO.Path]::IsPathRooted($InputPath)) {
        $InputPath = Join-Path $RepoRoot $InputPath
    }

    $resolvedInput = (Resolve-Path -LiteralPath $InputPath).Path
    $source = Get-Item -LiteralPath $resolvedInput
    if ([int64]$source.Length -ne $ExpectedSourceBytes) {
        throw "029 source size mismatch: expected $ExpectedSourceBytes got $($source.Length)"
    }
    $sourceSha = (Get-FileHash -LiteralPath $resolvedInput -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sourceSha -ne $ExpectedSourceSha256) {
        throw "029 source SHA mismatch: expected $ExpectedSourceSha256 got $sourceSha"
    }

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

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot "out/pub-research/mature-029-native-page-spread-direct"
    } elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot $OutputRoot
    }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

    Write-Host "MATURE-029 direct oracle: exact source and Publisher2019 identity verified."
    & pwsh -NoProfile -File $Operation -InputPath $resolvedInput -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) {
        throw "mature_029_native_page_spread_oracle_01.ps1 failed with exit code $LASTEXITCODE"
    }

    $analysis = Join-Path $OutputRoot "analysis/mature-029-native-page-spread-oracle-01.json"
    $log = Join-Path $OutputRoot "logs/mature-029-native-page-spread-oracle-01.txt"
    $environment = Join-Path $OutputRoot "environment.json"
    foreach ($path in @($analysis, $log, $environment)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required direct-oracle evidence missing: $path"
        }
    }

    $result = Get-Content -LiteralPath $analysis -Raw | ConvertFrom-Json
    if (-not [bool]$result.source.unchanged_after_probe) {
        throw "Direct oracle did not prove source immutability."
    }
    if ($result.claims.document_mutation_invoked -or
        $result.claims.save_invoked -or
        $result.claims.print_invoked -or
        $result.claims.export_invoked -or
        $result.claims.macro_execution_invoked) {
        throw "Direct oracle receipt violates the read-only contract."
    }

    Write-Host ""
    Write-Host "MATURE-029 direct oracle completed."
    Write-Host "Analysis: $analysis"
    Write-Host "Environment: $environment"
    Write-Host "Log: $log"
    Get-Content -LiteralPath $analysis -Raw
}
finally {
    Pop-Location
}
