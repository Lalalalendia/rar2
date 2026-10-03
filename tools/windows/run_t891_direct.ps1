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
$classifier = Join-Path $repoRoot "tools/t891_evidence_classifier.py"

$expectedBlobs = @(
    [pscustomobject]@{ Path = $packet; Sha = "572dd8dcd68f26e553b89ad5a142238ecd8ca25d" }
    [pscustomobject]@{ Path = $operation; Sha = "c14c2cccb28cbe467a44c6d1b39c233f56e78aee" }
    [pscustomobject]@{ Path = $prepare; Sha = "0848e8e147dff5dab68065c37d2d73f72f09eb45" }
    [pscustomobject]@{ Path = $finalize; Sha = "2a97d6f2c8be1265010a744c015ce8d288eb7e75" }
    [pscustomobject]@{ Path = $classifier; Sha = "864e53715ee4e5298f103995ba9c1a6d2314a166" }
)

Push-Location $repoRoot
try {
    foreach ($entry in $expectedBlobs) {
        if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) { throw "Required file missing: $($entry.Path)" }
        $actual = (& git hash-object -- $entry.Path).Trim()
        if ($LASTEXITCODE -ne 0) { throw "git hash-object failed for $($entry.Path)" }
        if ($actual -ne [string]$entry.Sha) { throw "Pinned file drift: $($entry.Path) expected $($entry.Sha) got $actual" }
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

    $analysis = Join-Path $OutputRoot "analysis/tlb-shape-effects-batch01.json"
    $blastDir = Join-Path $OutputRoot "analysis/blast-radius"
    $summary = Join-Path $OutputRoot "analysis/t891-evidence-summary.json"
    if (-not (Test-Path -LiteralPath $analysis -PathType Leaf)) { throw "Required analysis receipt missing: $analysis" }

    Write-Host "T891 direct host: classify evidence"
    & python $classifier --analysis $analysis --blast-dir $blastDir --out $summary
    if ($LASTEXITCODE -ne 0) { throw "t891_evidence_classifier.py failed with exit code $LASTEXITCODE" }

    Write-Host "T891 direct host: finalize"
    & pwsh -NoProfile -File $finalize -PacketPath $packet -OutputRoot $OutputRoot
    if ($LASTEXITCODE -ne 0) { throw "finalize_native_run.ps1 failed with exit code $LASTEXITCODE" }

    $manifest = Join-Path $OutputRoot "evidence-manifest.json"
    if (-not (Test-Path -LiteralPath $summary -PathType Leaf)) { throw "T891 evidence summary missing: $summary" }
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw "Evidence manifest missing: $manifest" }

    Write-Host ""
    Write-Host "T891 direct-host run completed."
    Write-Host "Analysis: $analysis"
    Write-Host "Summary: $summary"
    Write-Host "Manifest: $manifest"
    Get-Content -LiteralPath $summary -Raw
}
finally {
    Pop-Location
}
