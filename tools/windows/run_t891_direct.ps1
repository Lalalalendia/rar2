param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$packet = Join-Path $repoRoot "tools/research-runner/experiments/pub-tlb-shape-effects-batch01.packet.json"
$operation = Join-Path $repoRoot "tools/research-runner/operations/publisher_tlb_shape_effects_batch01.ps1"
$prepare = Join-Path $repoRoot "tools/research-runner/prepare_native_run.ps1"
$finalize = Join-Path $repoRoot "tools/research-runner/finalize_native_run.ps1"

$expectedBlobs = [ordered]@{
    $packet = "572dd8dcd68f26e553b89ad5a142238ecd8ca25d"
    $operation = "c14c2cccb28cbe467a44c6d1b39c233f56e78aee"
    $prepare = "0848e8e147dff5dab68065c37d2d73f72f09eb45"
    $finalize = "2a97d6f2c8be1265010a744c015ce8d288eb7e75"
}

Push-Location $repoRoot
try {
    foreach ($entry in $expectedBlobs.GetEnumerator()) {
        if (-not (Test-Path -LiteralPath $entry.Key -PathType Leaf)) { throw "Required file missing: $($entry.Key)" }
        $actual = (& git hash-object -- $entry.Key).Trim()
        if ($LASTEXITCODE -ne 0) { throw "git hash-object failed for $($entry.Key)" }
        if ($actual -ne [string]$entry.Value) { throw "Pinned file drift: $($entry.Key) expected $($entry.Value) got $actual" }
    }

    $validation = & python tools/research-runner/validate_packet.py --packet $packet --expected-environment publisher-2019 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Packet validation failed: $($validation -join [Environment]::NewLine)" }

    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $repoRoot "out/pub-research/tlb-shape-effects-batch01-direct"
    } elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $repoRoot $OutputRoot
    }

    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
    $env:PUB_RESEARCH_PROFILE_ID = "publisher-2019"

    Write-Host "T891 direct host: prepare"
    & pwsh -NoProfile -File $prepare -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "prepare_native_run.ps1 failed with exit code $LASTEXITCODE" }

    Write-Host "T891 direct host: execute 27 bounded arms"
    & pwsh -NoProfile -File $operation -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "publisher_tlb_shape_effects_batch01.ps1 failed with exit code $LASTEXITCODE" }

    Write-Host "T891 direct host: finalize"
    & pwsh -NoProfile -File $finalize -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "finalize_native_run.ps1 failed with exit code $LASTEXITCODE" }

    $analysis = Join-Path $OutputRoot "analysis/tlb-shape-effects-batch01.json"
    $manifest = Join-Path $OutputRoot "evidence-manifest.json"
    if (-not (Test-Path -LiteralPath $analysis -PathType Leaf)) { throw "Required analysis receipt missing: $analysis" }
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw "Evidence manifest missing: $manifest" }

    Write-Host ""
    Write-Host "T891 direct-host run completed."
    Write-Host "Analysis: $analysis"
    Write-Host "Manifest: $manifest"
    Get-Content -LiteralPath $analysis -Raw
}
finally {
    Pop-Location
}
