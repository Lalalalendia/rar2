param(
    [string]$InputPath = "",
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedSourceSha256 = "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade"
$ExpectedSourceBytes = 72704
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$ExpectedOracleMerge = "2f4d95f5bfdf42315d5b3f3882a2fb5e92fb9877"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/master_projection_stack_auth_01.ps1"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"
$ExpectedOperationBlob = "1c72a0b32219598a2dcc7c603873e1439b02000c"
$ExpectedRuntimeBlob = "fed4c890a34d39401d3b5848cc16d1087f862a27"

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

Push-Location $RepoRoot
try {
    & git merge-base --is-ancestor $ExpectedOracleMerge HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "Checkout does not contain required stack-oracle merge $ExpectedOracleMerge"
    }

    Assert-GitBlob -Path $Operation -Expected $ExpectedOperationBlob
    Assert-GitBlob -Path $Runtime -Expected $ExpectedRuntimeBlob

    if ([string]::IsNullOrWhiteSpace($InputPath)) {
        $candidate = Join-Path $RepoRoot "out/pinned1050/materialized/native/$ExpectedSourceSha256.pub"
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "Exact ordinary stack fixture is absent from the default pinned1050 path. Pass -InputPath to a local exact copy; do not download or substitute another PUB."
        }
        $InputPath = $candidate
    } elseif (-not [System.IO.Path]::IsPathRooted($InputPath)) {
        $InputPath = Join-Path $RepoRoot $InputPath
    }

    $resolvedInput = (Resolve-Path -LiteralPath $InputPath).Path
    $source = Get-Item -LiteralPath $resolvedInput
    if ([int64]$source.Length -ne $ExpectedSourceBytes) {
        throw "Stack fixture size mismatch: expected $ExpectedSourceBytes got $($source.Length)"
    }
    $sourceSha = (Get-FileHash -LiteralPath $resolvedInput -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sourceSha -ne $ExpectedSourceSha256) {
        throw "Stack fixture SHA mismatch: expected $ExpectedSourceSha256 got $sourceSha"
    }

    Assert-Publisher2019

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot "out/pub-research/master-projection-stack-direct"
    } elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot $OutputRoot
    }
    if (Test-Path -LiteralPath $OutputRoot) {
        $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
        if ($existing.Count -ne 0) {
            throw "OutputRoot is not empty; use a fresh directory to avoid mixing evidence: $OutputRoot"
        }
    }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

    Write-Host "MASTER-PROJECTION-STACK direct oracle: exact source and Publisher2019 identity verified."
    & $Operation -InputPath $resolvedInput -OutputRoot $OutputRoot

    $analysis = Join-Path $OutputRoot "analysis/master-projection-stack-auth-01.json"
    $log = Join-Path $OutputRoot "logs/master-projection-stack-auth-01.txt"
    $environment = Join-Path $OutputRoot "environment.json"
    foreach ($path in @($analysis, $log, $environment)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required direct-oracle evidence missing: $path"
        }
    }

    $result = Get-Content -LiteralPath $analysis -Raw | ConvertFrom-Json
    if ([string]$result.source.sha256 -ne $ExpectedSourceSha256) {
        throw "Stack receipt source identity mismatch."
    }
    if (-not [bool]$result.source.unchanged_after_experiment) {
        throw "Stack oracle did not prove original source immutability."
    }
    if ([bool]$result.claims.source_original_mutated) {
        throw "Stack oracle receipt reports original-source mutation."
    }
    if ([bool]$result.claims.generated_pub_uploaded -or [bool]$result.claims.generated_page_picture_uploaded) {
        throw "Stack oracle receipt violates the private-local generated-artifact boundary."
    }
    if (-not [bool]$result.claims.save_close_fresh_reopen_per_arm -or -not [bool]$result.claims.two_page_master_excluded) {
        throw "Stack oracle receipt is missing required bounded experiment fences."
    }

    Write-Host ""
    Write-Host "MASTER-PROJECTION-STACK direct oracle completed."
    Write-Host "Analysis: $analysis"
    Write-Host "Environment: $environment"
    Write-Host "Log: $log"
    Write-Host "Verdict: $($result.verdict)"
    Get-Content -LiteralPath $analysis -Raw
}
finally {
    Pop-Location
}
