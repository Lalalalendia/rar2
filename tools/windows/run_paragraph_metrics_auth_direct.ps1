param(
    [string]$FixtureRoot = "",
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedFixtureSha256 = "5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b"
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/paragraph-metrics-auth-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/paragraph_metrics_auth_01.ps1"
$Analysis = Join-Path $RepoRoot "tools/research-runner/analysis/paragraph_metrics_auth_01_blast_radius.py"
$Prepare = Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1"
$Finalize = Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1"
$Validate = Join-Path $RepoRoot "tools/research-runner/validate_packet.py"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"

$ExpectedFiles = @(
    [pscustomobject]@{ Path = $Packet; Sha = "c8532d9f2ccc4ca11ec7771e749c8a3ab5244ded" }
    [pscustomobject]@{ Path = $Operation; Sha = "8339eb6ac719127a64c987e257c45050dfa54245" }
    [pscustomobject]@{ Path = $Analysis; Sha = "bb728c54f250f43c9e8c2f096ede89cd94436860" }
    [pscustomobject]@{ Path = $Prepare; Sha = "0848e8e147dff5dab68065c37d2d73f72f09eb45" }
    [pscustomobject]@{ Path = $Finalize; Sha = "2a97d6f2c8be1265010a744c015ce8d288eb7e75" }
    [pscustomobject]@{ Path = $Validate; Sha = "47109ad5ce1c90309f5be1f16fafeed745f862fa" }
    [pscustomobject]@{ Path = $Runtime; Sha = "fed4c890a34d39401d3b5848cc16d1087f862a27" }
)

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

Push-Location $RepoRoot
try {
    foreach ($entry in $ExpectedFiles) {
        Assert-GitBlob -Path $entry.Path -Expected $entry.Sha
    }

    $validation = & python $Validate --packet $Packet --expected-environment publisher-2019 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Paragraph-metrics packet validation failed: $($validation -join [Environment]::NewLine)"
    }

    if (-not [string]::IsNullOrWhiteSpace($FixtureRoot)) {
        if (-not [System.IO.Path]::IsPathRooted($FixtureRoot)) {
            $FixtureRoot = Join-Path $RepoRoot $FixtureRoot
        }
        $env:PUB_RESEARCH_FIXTURE_ROOT = (Resolve-Path -LiteralPath $FixtureRoot).Path
    }
    if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE_ROOT)) {
        throw "PUB_RESEARCH_FIXTURE_ROOT is required for the exact runner-root blank fixture. Pass -FixtureRoot or set the environment variable; this launcher never downloads or synthesizes the fixture."
    }

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot "out/pub-research/paragraph-metrics-auth-01-direct"
    } elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot $OutputRoot
    }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

    $env:PUB_RESEARCH_PROFILE_ID = "publisher-2019"
    $env:GITHUB_SHA = (& git rev-parse HEAD).Trim()

    # Run prepare in this PowerShell process so PUB_RESEARCH_FIXTURE survives
    # into paragraph_metrics_auth_01.ps1.
    & $Prepare -PacketPath $Packet -OutputRoot $OutputRoot
    if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) {
        throw "prepare_native_run.ps1 did not bind PUB_RESEARCH_FIXTURE."
    }
    $fixtureSha = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_FIXTURE -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($fixtureSha -ne $ExpectedFixtureSha256) {
        throw "Prepared paragraph-metrics fixture SHA mismatch: expected $ExpectedFixtureSha256 got $fixtureSha"
    }

    Import-Module $Runtime -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher COM is unavailable after prepare." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) {
        throw "Publisher Version mismatch: expected $ExpectedPublisherVersion"
    }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) {
        throw "Publisher Build mismatch: expected $ExpectedPublisherBuild"
    }

    Write-Host "PARAGRAPH-METRICS direct oracle: exact packet, fixture and Publisher2019 identity verified."
    & $Operation -PacketPath $Packet -OutputRoot $OutputRoot

    $native = Join-Path $OutputRoot "analysis/paragraph-metrics-auth-01.json"
    $log = Join-Path $OutputRoot "logs/paragraph-metrics-auth-01.txt"
    $environment = Join-Path $OutputRoot "environment.json"
    foreach ($path in @($native, $log, $environment)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required paragraph-metrics native evidence missing: $path"
        }
    }

    & python $Analysis --output-root $OutputRoot
    if ($LASTEXITCODE -ne 0) {
        throw "paragraph_metrics_auth_01_blast_radius.py failed with exit code $LASTEXITCODE"
    }

    $blast = Join-Path $OutputRoot "analysis/paragraph-metrics-auth-01-blast-radius.json"
    if (-not (Test-Path -LiteralPath $blast -PathType Leaf)) {
        throw "Paragraph-metrics blast-radius summary missing: $blast"
    }

    & $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
    $manifest = Join-Path $OutputRoot "evidence-manifest.json"
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        throw "Paragraph-metrics evidence manifest missing: $manifest"
    }

    $nativeResult = Get-Content -LiteralPath $native -Raw | ConvertFrom-Json
    if ([string]$nativeResult.experiment_id -ne "PARAGRAPH-METRICS-AUTH-01") {
        throw "Unexpected paragraph-metrics experiment id in native receipt."
    }
    if ([string]$nativeResult.verdict -ne "native-semantic-arms-captured-with-common-seed") {
        throw "Unexpected paragraph-metrics native verdict: $($nativeResult.verdict)"
    }

    $blastResult = Get-Content -LiteralPath $blast -Raw | ConvertFrom-Json
    if (-not [bool]$blastResult.causal_baseline.common_seed_used -or
        -not [bool]$blastResult.causal_baseline.matched_noop_control_used -or
        -not [bool]$blastResult.causal_baseline.paragraph_before_mutation_identical_across_arms -or
        -not [bool]$blastResult.causal_baseline.frame_before_mutation_identical_across_arms) {
        throw "Paragraph-metrics causal baseline did not pass."
    }

    Write-Host ""
    Write-Host "PARAGRAPH-METRICS direct oracle completed."
    Write-Host "Native: $native"
    Write-Host "Blast radius: $blast"
    Write-Host "Manifest: $manifest"
    Get-Content -LiteralPath $blast -Raw
}
finally {
    Pop-Location
}
