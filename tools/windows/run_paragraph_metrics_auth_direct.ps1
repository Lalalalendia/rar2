param(
    [string]$FixtureRoot = "",
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedFixtureSha256 = "5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b"
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$ExpectedPreparationMerge = "edc03eac3c6ad58214d8a69388100748412ec88f"
$ExpectedStructuralRevision = "1d8b8b4c04b2670dbfdbe03b2cdbc89fb4b615c0"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/paragraph-metrics-auth-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/paragraph_metrics_auth_01.ps1"
$Analyzer = Join-Path $RepoRoot "tools/research-runner/analysis/paragraph_metrics_auth_01_blast_radius.py"
$Prepare = Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1"
$Finalize = Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"
$Blast = Join-Path $RepoRoot "tools/operation_blast_radius_v1.py"
$Structural = Join-Path $RepoRoot "tools/research-runner/analysis/paragraph_metrics_auth_01_structural.py"
$ProbeManifest = Join-Path $RepoRoot "tools/research-runner/paragraph-metrics-probe/Cargo.toml"
$ProbeLock = Join-Path $RepoRoot "tools/research-runner/paragraph-metrics-probe/Cargo.lock"
$ProbeMain = Join-Path $RepoRoot "tools/research-runner/paragraph-metrics-probe/src/main.rs"

$PinnedFiles = @(
    [pscustomobject]@{ Path = $Packet; Sha = "c8532d9f2ccc4ca11ec7771e749c8a3ab5244ded" }
    [pscustomobject]@{ Path = $Operation; Sha = "6ee22938e5c0aad7de054b3c2243ce54e056450c" }
    [pscustomobject]@{ Path = $Analyzer; Sha = "bb728c54f250f43c9e8c2f096ede89cd94436860" }
    [pscustomobject]@{ Path = $Prepare; Sha = "0848e8e147dff5dab68065c37d2d73f72f09eb45" }
    [pscustomobject]@{ Path = $Finalize; Sha = "2a97d6f2c8be1265010a744c015ce8d288eb7e75" }
    [pscustomobject]@{ Path = $Runtime; Sha = "fed4c890a34d39401d3b5848cc16d1087f862a27" }
    [pscustomobject]@{ Path = $Blast; Sha = "c458539ec7b2f5a5bb35844e9ca3e35df16ff56f" }
    [pscustomobject]@{ Path = $Structural; Sha = "7dee55a3e2c870cbb3981995789329cd74e85a65" }
    [pscustomobject]@{ Path = $ProbeManifest; Sha = "40d209d6e193d477635d72fb34e7bd9f640d7270" }
    [pscustomobject]@{ Path = $ProbeLock; Sha = "c21bbf53e4c64931c6396810589809ed5f51c16d" }
    [pscustomobject]@{ Path = $ProbeMain; Sha = "cf108b7e8cb8d38828b5e91973f26e85918ad86d" }
)

function Assert-LastExit([string]$Label) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

Push-Location $RepoRoot
try {
    & git merge-base --is-ancestor $ExpectedPreparationMerge HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "Checkout does not contain required causal matrix merge $ExpectedPreparationMerge"
    }
    & git merge-base --is-ancestor $ExpectedStructuralRevision HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "Checkout does not contain required structural receipt revision $ExpectedStructuralRevision"
    }
    & git diff --quiet $ExpectedStructuralRevision -- `
        tools/research-runner/paragraph-metrics-probe `
        tools/research-runner/analysis/paragraph_metrics_auth_01_structural.py `
        vendor/producer-a/crates/pub-quill
    if ($LASTEXITCODE -ne 0) {
        throw "Structural paragraph-metrics implementation drifted after $ExpectedStructuralRevision; refresh direct-launcher pins before collecting authority."
    }

    foreach ($entry in $PinnedFiles) {
        if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) {
            throw "Required file missing: $($entry.Path)"
        }
        $actualBlob = (& git hash-object -- $entry.Path).Trim()
        Assert-LastExit "git hash-object $($entry.Path)"
        if ($actualBlob -ne [string]$entry.Sha) {
            throw "Pinned file drift: $($entry.Path) expected $($entry.Sha) got $actualBlob"
        }
    }

    if ([string]::IsNullOrWhiteSpace($FixtureRoot)) {
        $FixtureRoot = [string]$env:PUB_RESEARCH_FIXTURE_ROOT
    }
    if ([string]::IsNullOrWhiteSpace($FixtureRoot)) {
        throw "Fixture root is required. Pass -FixtureRoot or set PUB_RESEARCH_FIXTURE_ROOT; no download fallback is allowed."
    }
    if (-not [System.IO.Path]::IsPathRooted($FixtureRoot)) {
        $FixtureRoot = Join-Path $RepoRoot $FixtureRoot
    }
    $FixtureRoot = (Resolve-Path -LiteralPath $FixtureRoot).Path
    $Fixture = Join-Path $FixtureRoot "pubgen-create-20260923/minimal-blank-v1-generated.pub"
    if (-not (Test-Path -LiteralPath $Fixture -PathType Leaf)) {
        throw "Exact PUB-T-823 runner-root fixture is absent: $Fixture"
    }
    $fixtureSha = (Get-FileHash -LiteralPath $Fixture -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($fixtureSha -ne $ExpectedFixtureSha256) {
        throw "Fixture SHA mismatch: expected $ExpectedFixtureSha256 got $fixtureSha"
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

    $python = Get-Command python -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot "out/pub-research/paragraph-metrics-auth-01-direct"
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

    $env:PUB_RESEARCH_FIXTURE_ROOT = $FixtureRoot
    $env:PUB_RESEARCH_FIXTURE = $Fixture

    Write-Host "PARAGRAPH-METRICS-AUTH-01 direct run: pinned code, fixture and Publisher2019 identity verified."

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Prepare -PacketPath $Packet -OutputRoot $OutputRoot
    Assert-LastExit "prepare_native_run.ps1"

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
    Assert-LastExit "paragraph_metrics_auth_01.ps1"

    & $python.Source $Analyzer --output-root $OutputRoot
    Assert-LastExit "paragraph_metrics_auth_01_blast_radius.py"

    $cargo = Get-Command cargo -ErrorAction Stop
    & $cargo.Source build --offline --locked --release --manifest-path $ProbeManifest
    Assert-LastExit "paragraph-metrics-probe offline build"
    $ProbeRoot = Split-Path -Parent $ProbeManifest
    $Probe = Join-Path $ProbeRoot "target/release/paragraph-metrics-probe.exe"
    if (-not (Test-Path -LiteralPath $Probe -PathType Leaf)) {
        throw "Structural snapshot tool missing after offline build: $Probe"
    }

    & $python.Source $Structural --output-root $OutputRoot --snapshot-tool $Probe
    Assert-LastExit "paragraph_metrics_auth_01_structural.py"

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
    Assert-LastExit "finalize_native_run.ps1"

    $NativePath = Join-Path $OutputRoot "analysis/paragraph-metrics-auth-01.json"
    $BlastPath = Join-Path $OutputRoot "analysis/paragraph-metrics-auth-01-blast-radius.json"
    $StructuralPath = Join-Path $OutputRoot "analysis/paragraph-metrics-auth-01-structural.json"
    $EnvironmentPath = Join-Path $OutputRoot "environment.json"
    $LogPath = Join-Path $OutputRoot "logs/paragraph-metrics-auth-01.txt"
    $ManifestPath = Join-Path $OutputRoot "evidence-manifest.json"
    foreach ($path in @($NativePath, $BlastPath, $StructuralPath, $EnvironmentPath, $LogPath, $ManifestPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required evidence missing: $path"
        }
    }

    $native = Get-Content -LiteralPath $NativePath -Raw | ConvertFrom-Json
    $blastReceipt = Get-Content -LiteralPath $BlastPath -Raw | ConvertFrom-Json
    if ([string]$native.verdict -ne "native-semantic-arms-captured-with-common-seed") {
        throw "Unexpected native verdict: $($native.verdict)"
    }
    if ($null -eq $native.seed -or [string]::IsNullOrWhiteSpace([string]$native.seed.sha256)) {
        throw "Native receipt did not record the common seed SHA."
    }
    if (@($native.arms).Count -ne 12) {
        throw "Expected 12 native arms including matched control, got $(@($native.arms).Count)"
    }
    if (@($blastReceipt.arms).Count -ne 11) {
        throw "Expected 11 mutation blast-radius arms, got $(@($blastReceipt.arms).Count)"
    }

    $baseline = $blastReceipt.causal_baseline
    foreach ($name in @(
        "common_seed_used",
        "matched_noop_control_used",
        "paragraph_before_mutation_identical_across_arms",
        "frame_before_mutation_identical_across_arms",
        "fresh_reopen_text_length_invariant_across_arms"
    )) {
        if (-not [bool]$baseline.$name) {
            throw "Causal baseline invariant failed: $name"
        }
    }
    if ([string]$blastReceipt.remaining_structural_gap.raw_fdpp_property_decode -ne "required") {
        throw "Unexpected FDPP structural-gap status."
    }
    if ([string]$blastReceipt.remaining_structural_gap.quill_text_byte_invariance -ne "required") {
        throw "Unexpected Quill TEXT structural-gap status."
    }

    $structuralReceipt = Get-Content -LiteralPath $StructuralPath -Raw | ConvertFrom-Json
    foreach ($name in @(
        "complete_eleven_arm_matrix",
        "complete_twelve_arm_matrix",
        "exact_artifact_identity_join",
        "common_pre_mutation_snapshots",
        "quill_text_byte_invariance_all_arms",
        "confirmed_story_partition_invariant",
        "raw_fdpp_framing_known"
    )) {
        if (-not [bool]$structuralReceipt.invariants.$name) {
            throw "Structural receipt invariant failed: $name"
        }
    }
    if ([bool]$structuralReceipt.invariants.paragraph_metric_semantics_granted) {
        throw "Structural receipt must not grant paragraph metric semantics."
    }
    if ([string]$structuralReceipt.remaining_authority.native_rule_to_persisted_carrier_law -ne "requires_semantic_review_of_raw_matrix") {
        throw "Unexpected structural carrier-law status."
    }
    if ([string]$structuralReceipt.remaining_authority.effective_line_origins_and_heights -ne "not_proven_by_frame_bounds_or_raw_values") {
        throw "Unexpected effective-line authority status."
    }

    $manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
    $manifestPaths = @($manifest.files | ForEach-Object { [string]$_.path })
    foreach ($requiredPath in @(
        "analysis/paragraph-metrics-auth-01.json",
        "analysis/paragraph-metrics-auth-01-blast-radius.json",
        "analysis/paragraph-metrics-auth-01-structural.json",
        "environment.json",
        "logs/paragraph-metrics-auth-01.txt"
    )) {
        if ($manifestPaths -notcontains $requiredPath) {
            throw "Evidence manifest does not bind required file: $requiredPath"
        }
    }

    $fixtureShaAfter = (Get-FileHash -LiteralPath $Fixture -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($fixtureShaAfter -ne $ExpectedFixtureSha256) {
        throw "Original fixture changed during the run."
    }

    Write-Host ""
    Write-Host "PARAGRAPH-METRICS-AUTH-01 causal matrix completed."
    Write-Host "Native receipt: $NativePath"
    Write-Host "Blast-radius receipt: $BlastPath"
    Write-Host "Structural receipt: $StructuralPath"
    Write-Host "Evidence manifest: $ManifestPath"
    Write-Host "Environment: $EnvironmentPath"
    Write-Host "Log: $LogPath"
    Write-Host "Remaining authority gap: semantic review of raw FDPP matrix + native effective line origins/heights."
    Get-Content -LiteralPath $StructuralPath -Raw
}
finally {
    Pop-Location
}
