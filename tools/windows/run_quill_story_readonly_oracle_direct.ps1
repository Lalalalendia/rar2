param(
    [string]$OutputRoot = "",
    [string]$WitnessRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$packet = Join-Path $repoRoot "tools/research-runner/experiments/quill-story-readonly-oracle-02.packet.json"
$operation = Join-Path $repoRoot "tools/research-runner/operations/quill_story_readonly_oracle_02.ps1"
$prepare = Join-Path $repoRoot "tools/research-runner/prepare_native_run.ps1"
$finalize = Join-Path $repoRoot "tools/research-runner/finalize_native_run.ps1"
$resolver = Join-Path $repoRoot "vendor/producer-a/crates/pub-reader/src/bin/quill-story-readonly-oracle-resolver.rs"

$expectedBlobs = @(
    [pscustomobject]@{ Path = $packet; Sha = "41ec1b1b34bc277c7f3ac5cb9829569ca189053a" }
    [pscustomobject]@{ Path = $operation; Sha = "8a2b6f28edf86ddf10ec09b865db25549fa7ea4f" }
    [pscustomobject]@{ Path = $prepare; Sha = "0848e8e147dff5dab68065c37d2d73f72f09eb45" }
    [pscustomobject]@{ Path = $finalize; Sha = "2a97d6f2c8be1265010a744c015ce8d288eb7e75" }
    [pscustomobject]@{ Path = $resolver; Sha = "b92f85e133eede4096156202226f6961d15cab3c" }
)

$witnesses = @(
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff"
)
$lalamuCommit = "f78cc6f455f4dc222868f9cc035511a6ca7a91ea"

function Assert-WitnessRoot {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root
    )

    $resolved = (Resolve-Path -LiteralPath $Root).Path
    foreach ($sha in $witnesses) {
        $path = Join-Path $resolved "$sha.pub"
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Exact local witness missing: $path"
        }
        $actualSha = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualSha -ne $sha) {
            throw "Exact local witness SHA mismatch for $sha"
        }
    }
    return $resolved
}

Push-Location $repoRoot
$pwshCommand = Get-Command -Name pwsh -CommandType Application -ErrorAction SilentlyContinue
$researchPowerShell = if ($null -ne $pwshCommand) {
    [string]$pwshCommand.Source
} else {
    Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
}
if (-not (Test-Path -LiteralPath $researchPowerShell -PathType Leaf)) {
    throw "No supported PowerShell host executable found."
}

$previousFixtureRoot = [string]$env:PUB_RESEARCH_FIXTURE_ROOT
$tempRoot = $null
try {
    foreach ($entry in $expectedBlobs) {
        if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) {
            throw "Required file missing: $($entry.Path)"
        }
        $actual = (& git hash-object -- $entry.Path).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw "git hash-object failed for $($entry.Path)"
        }
        if ($actual -ne [string]$entry.Sha) {
            throw "Pinned file drift: $($entry.Path) expected $($entry.Sha) got $actual"
        }
    }

    $validation = & python tools/research-runner/validate_packet.py --packet $packet --expected-environment publisher-2019 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Packet validation failed: $($validation -join [Environment]::NewLine)"
    }

    $resolvedWitnessRoot = $null
    if (-not [string]::IsNullOrWhiteSpace($WitnessRoot)) {
        $resolvedWitnessRoot = Assert-WitnessRoot -Root $WitnessRoot
        $env:PUB_RESEARCH_FIXTURE_ROOT = $resolvedWitnessRoot
        Write-Host "Quill Story oracle direct host: exact local witness mode"
    } else {
        Remove-Item Env:PUB_RESEARCH_FIXTURE_ROOT -ErrorAction SilentlyContinue
        Write-Host "Quill Story oracle direct host: pinned public witness download mode"
    }

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $repoRoot "out/pub-research/quill-story-readonly-oracle-direct"
    } elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $repoRoot $OutputRoot
    }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

    $env:PUB_RESEARCH_PROFILE_ID = "publisher-2019"

    Write-Host "Quill Story oracle direct host: prepare"
    & $researchPowerShell -NoProfile -ExecutionPolicy Bypass -File $prepare -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) {
        throw "prepare_native_run.ps1 failed with exit code $LASTEXITCODE"
    }

    Write-Host "Quill Story oracle direct host: execute read-only Publisher observation"
    & $researchPowerShell -NoProfile -ExecutionPolicy Bypass -File $operation -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) {
        throw "quill_story_readonly_oracle_02.ps1 failed with exit code $LASTEXITCODE"
    }

    Write-Host "Quill Story oracle direct host: finalize"
    & $researchPowerShell -NoProfile -ExecutionPolicy Bypass -File $finalize -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) {
        throw "finalize_native_run.ps1 failed with exit code $LASTEXITCODE"
    }

    $oracle = Join-Path $OutputRoot "analysis/quill-story-readonly-oracle.json"
    $manifest = Join-Path $OutputRoot "evidence-manifest.json"
    if (-not (Test-Path -LiteralPath $oracle -PathType Leaf)) {
        throw "Oracle receipt missing: $oracle"
    }
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        throw "Evidence manifest missing: $manifest"
    }

    $resolverWitnessRoot = $resolvedWitnessRoot
    if ($null -eq $resolverWitnessRoot) {
        $tempBase = if (-not [string]::IsNullOrWhiteSpace($env:TEMP)) {
            $env:TEMP
        } else {
            throw "TEMP is unavailable for resolver witness staging"
        }
        $tempRoot = Join-Path $tempBase ("quill-story-readonly-resolve-" + [guid]::NewGuid().ToString("N"))
        $resolverWitnessRoot = Join-Path $tempRoot "witness"
        New-Item -ItemType Directory -Force -Path $resolverWitnessRoot | Out-Null

        foreach ($sha in $witnesses) {
            $url = "https://raw.githubusercontent.com/Lalalalendia/lalamu/$lalamuCommit/pub-corpus/corpus/native/unclassified/$sha.pub"
            $target = Join-Path $resolverWitnessRoot "$sha.pub"
            Invoke-WebRequest -Uri $url -OutFile $target -UseBasicParsing
            $actualSha = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($actualSha -ne $sha) {
                throw "Witness SHA mismatch for $sha"
            }
        }
    }

    $resolution = Join-Path $OutputRoot "analysis/quill-story-readonly-oracle-resolution.json"
    Write-Host "Quill Story oracle direct host: resolve against source TEXT + FDPP"
    & cargo run --manifest-path vendor/producer-a/Cargo.toml -p pub-reader --bin quill-story-readonly-oracle-resolver -- $resolverWitnessRoot $oracle $resolution
    if ($LASTEXITCODE -ne 0) {
        throw "quill-story-readonly-oracle-resolver failed with exit code $LASTEXITCODE"
    }

    $report = Get-Content -LiteralPath $resolution -Raw | ConvertFrom-Json
    if ([int]$report.witness_count -ne 4) {
        throw "Resolver witness_count mismatch: $($report.witness_count)"
    }

    $crossChecks = [ordered]@{
        "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a" = @(11, 12, 13, 25)
        "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569" = @(0, 1, 45)
    }
    foreach ($sourceSha in $crossChecks.Keys) {
        $rows = @($report.rows | Where-Object { $_.source_sha256 -eq $sourceSha })
        if ($rows.Count -ne 1) {
            throw "Expected exactly one historical-donor cross-check row for $sourceSha"
        }
        if ([bool]$rows[0].unique_hash_partition) {
            $actualOrdinals = @($rows[0].terminal_fdpp_ordinals_zero_based | ForEach-Object { [int]$_ })
            $expectedOrdinals = @($crossChecks[$sourceSha])
            if (($actualOrdinals -join ",") -ne ($expectedOrdinals -join ",")) {
                throw "Historical donor cross-check mismatch for ${sourceSha}: $($actualOrdinals -join ',')"
            }
        }
    }

    Write-Host ""
    Write-Host "Quill Story read-only direct-host run completed."
    Write-Host "Oracle: $oracle"
    Write-Host "Resolution: $resolution"
    Write-Host "Manifest: $manifest"
    Get-Content -LiteralPath $resolution -Raw
}
finally {
    if ($null -ne $tempRoot -and (Test-Path -LiteralPath $tempRoot)) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }

    if ([string]::IsNullOrWhiteSpace($previousFixtureRoot)) {
        Remove-Item Env:PUB_RESEARCH_FIXTURE_ROOT -ErrorAction SilentlyContinue
    } else {
        $env:PUB_RESEARCH_FIXTURE_ROOT = $previousFixtureRoot
    }

    Pop-Location
}
