param(
    [string]$FixtureRoot = "",
    [string]$OutputRoot = "",
    [string]$ReturnZip = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Push-Location $RepoRoot
$fixture = $null

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/paragraph-line-spacing-0p75-native-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/paragraph_line_spacing_0p75_native_01.ps1"
$Prepare = Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1"
$Finalize = Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1"
$Runtime = Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1"
$Analyzer = Join-Path $RepoRoot "tools/research-runner/analysis/paragraph_line_spacing_0p75_structural.py"
$StructuralBase = Join-Path $RepoRoot "tools/research-runner/analysis/paragraph_metrics_auth_01_structural.py"
$ProbeManifest = Join-Path $RepoRoot "tools/research-runner/paragraph-metrics-probe/Cargo.toml"
$ProbeLock = Join-Path $RepoRoot "tools/research-runner/paragraph-metrics-probe/Cargo.lock"
$ProbeMain = Join-Path $RepoRoot "tools/research-runner/paragraph-metrics-probe/src/main.rs"

$ExpectedFixtureSha256 = "5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b"
$FixtureRelativePath = "pubgen-create-20260923\minimal-blank-v1-generated.pub"
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

$PinnedFiles = @(
    [pscustomobject]@{ Path = $Packet; Sha = "2f5e641a3c18215db7e2bd4df303cd55ddcf1f46" }
    [pscustomobject]@{ Path = $Operation; Sha = "b15eaabff83fe31a14a5c83c8d743689ad2e6269" }
    [pscustomobject]@{ Path = $Prepare; Sha = "0848e8e147dff5dab68065c37d2d73f72f09eb45" }
    [pscustomobject]@{ Path = $Finalize; Sha = "2a97d6f2c8be1265010a744c015ce8d288eb7e75" }
    [pscustomobject]@{ Path = $Runtime; Sha = "fed4c890a34d39401d3b5848cc16d1087f862a27" }
    [pscustomobject]@{ Path = $Analyzer; Sha = "064aa8a41231fe52b727393b4775df1d3f0d4a49" }
    [pscustomobject]@{ Path = $StructuralBase; Sha = "1d12b34de10ad74ea49a4fa2cabbe01b44198331" }
    [pscustomobject]@{ Path = $ProbeManifest; Sha = "40d209d6e193d477635d72fb34e7bd9f640d7270" }
    [pscustomobject]@{ Path = $ProbeLock; Sha = "828e76d2f85c1238ced8081796a58c309d5fbade" }
    [pscustomobject]@{ Path = $ProbeMain; Sha = "cf108b7e8cb8d38828b5e91973f26e85918ad86d" }
)

function Assert-LastExit([string]$Label) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Resolve-CanonicalFixture {
    param([string]$RequestedRoot)

    $rootCandidates = @()
    if (-not [string]::IsNullOrWhiteSpace($RequestedRoot)) { $rootCandidates += $RequestedRoot }
    if (-not [string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE_ROOT)) {
        $rootCandidates += [string]$env:PUB_RESEARCH_FIXTURE_ROOT
    }
    if (-not [string]::IsNullOrWhiteSpace([string]$env:USERPROFILE)) {
        $rootCandidates += (Join-Path ([string]$env:USERPROFILE) "rar2\realtest")
    }

    # Historical paragraph-metrics receipts prove the exact pinned fixture existed
    # under a bounded D-drive Publisher lab root on the qualified Windows host.
    # Construct that exact root without embedding a machine-specific absolute
    # path. No drive-wide discovery is allowed, and every candidate still has
    # to match ExpectedFixtureSha256.
    $legacyDrive = Get-PSDrive -Name D -PSProvider FileSystem -ErrorAction SilentlyContinue
    if ($null -ne $legacyDrive) {
        $rootCandidates += (Join-Path $legacyDrive.Root "Downloads\Downloads\pubtool-0.2.0\realtest")
    }

    $rootCandidates += $RepoRoot
    $rootCandidates += (Split-Path -Parent $RepoRoot)
    $seen = @{}
    foreach ($root in $rootCandidates) {
        if ([string]::IsNullOrWhiteSpace([string]$root)) { continue }
        try { $resolvedRoot = (Resolve-Path -LiteralPath $root -ErrorAction Stop).Path } catch { continue }
        $key = $resolvedRoot.ToLowerInvariant()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true

        $direct = Join-Path $resolvedRoot $FixtureRelativePath
        if (Test-Path -LiteralPath $direct -PathType Leaf) {
            if ((Get-Sha256 $direct) -eq $ExpectedFixtureSha256) {
                return [pscustomobject]@{ Root = $resolvedRoot; Path = $direct; Staged = $false }
            }
        }

        $matches = @(Get-ChildItem -LiteralPath $resolvedRoot -Filter "minimal-blank-v1-generated.pub" -File -Recurse -ErrorAction SilentlyContinue)
        foreach ($match in $matches) {
            if ((Get-Sha256 $match.FullName) -eq $ExpectedFixtureSha256) {
                $stagingRoot = Join-Path $env:TEMP ("chaptera-pub-research-fixture-" + [guid]::NewGuid().ToString("N"))
                $stagedPath = Join-Path $stagingRoot $FixtureRelativePath
                New-Item -ItemType Directory -Force -Path (Split-Path -Parent $stagedPath) | Out-Null
                Copy-Item -LiteralPath $match.FullName -Destination $stagedPath -Force
                return [pscustomobject]@{ Root = $stagingRoot; Path = $stagedPath; Staged = $true }
            }
        }
    }

    throw "Exact canonical blank fixture was not found. Pass -FixtureRoot or set PUB_RESEARCH_FIXTURE_ROOT."
}

try {
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

    python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/paragraph-line-spacing-0p75-native-01.packet.json" --expected-environment publisher-2019
    Assert-LastExit "packet validation"

    Import-Module $Runtime -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher.Application / MSPUB.EXE not found." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) {
        throw "Publisher Version mismatch: expected $ExpectedPublisherVersion"
    }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) {
        throw "Publisher Build mismatch: expected $ExpectedPublisherBuild"
    }
    if ($publisher.path.state -ne "value") { throw "Publisher executable directory is unavailable." }
    $publisherExe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
    if (-not (Test-Path -LiteralPath $publisherExe -PathType Leaf)) { throw "MSPUB.EXE missing at COM-reported path." }
    $fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($publisherExe).FileVersion
    if ([string]$fileVersion -ne $ExpectedPublisherFileVersion) {
        throw "MSPUB.EXE file version mismatch: expected $ExpectedPublisherFileVersion got $fileVersion"
    }
    $exeSha = Get-Sha256 $publisherExe
    if ($exeSha -ne $ExpectedPublisherExeSha256) {
        throw "MSPUB.EXE SHA mismatch: expected $ExpectedPublisherExeSha256 got $exeSha"
    }

    $fixture = Resolve-CanonicalFixture -RequestedRoot $FixtureRoot
    if ((Get-Sha256 $fixture.Path) -ne $ExpectedFixtureSha256) {
        throw "Fixture SHA mismatch after staging."
    }

    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot ("out/pub-research/paragraph-line-spacing-0p75-native-01-" + $stamp)
    } elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
        $OutputRoot = Join-Path $RepoRoot $OutputRoot
    }
    if (Test-Path -LiteralPath $OutputRoot) {
        $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
        if ($existing.Count -ne 0) {
            throw "OutputRoot is not empty; use a fresh directory to avoid mixing evidence: $OutputRoot"
        }
    }

    if ([string]::IsNullOrWhiteSpace($ReturnZip)) {
        $ReturnZip = Join-Path (Split-Path -Parent $OutputRoot) ("RETURN-TO-CHAT-PARAGRAPH-0P75-" + $stamp + ".zip")
    } elseif (-not [IO.Path]::IsPathRooted($ReturnZip)) {
        $ReturnZip = Join-Path $RepoRoot $ReturnZip
    }
    if (Test-Path -LiteralPath $ReturnZip) {
        throw "Return ZIP already exists: $ReturnZip"
    }

    $env:PUB_RESEARCH_FIXTURE_ROOT = $fixture.Root
    $env:PUB_RESEARCH_PACKET_SHA256 = Get-Sha256 $Packet
    $env:PUB_RESEARCH_PROFILE_ID = "publisher-2019-direct-0p75"
    $gitSha = (& git rev-parse HEAD).Trim()
    Assert-LastExit "git rev-parse HEAD"
    $env:GITHUB_SHA = $gitSha

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Prepare -PacketPath $Packet -OutputRoot $OutputRoot
    Assert-LastExit "prepare_native_run.ps1"

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
    Assert-LastExit "paragraph_line_spacing_0p75_native_01.ps1"

    $cargo = Get-Command cargo -ErrorAction Stop
    & $cargo.Source build --offline --locked --release --manifest-path $ProbeManifest
    Assert-LastExit "paragraph-metrics-probe offline build"
    $ProbeRoot = Split-Path -Parent $ProbeManifest
    $Probe = Join-Path $ProbeRoot "target/release/paragraph-metrics-probe.exe"
    if (-not (Test-Path -LiteralPath $Probe -PathType Leaf)) {
        throw "Structural snapshot tool missing after offline build: $Probe"
    }

    $python = Get-Command python -ErrorAction Stop
    & $python.Source $Analyzer --output-root $OutputRoot --snapshot-tool $Probe
    Assert-LastExit "paragraph_line_spacing_0p75_structural.py"

    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
    Assert-LastExit "finalize_native_run.ps1"

    $resultPath = Join-Path $OutputRoot "analysis/paragraph-line-spacing-0p75-native-01.json"
    $structuralPath = Join-Path $OutputRoot "analysis/paragraph-line-spacing-0p75-structural.json"
    $manifestPath = Join-Path $OutputRoot "evidence-manifest.json"
    foreach ($required in @($resultPath, $structuralPath, $manifestPath, (Join-Path $OutputRoot "environment.json"))) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "Required evidence missing after run: $required"
        }
    }

    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    if (@($result.arms).Count -ne 3) { throw "Expected exactly 3 native arms." }
    foreach ($arm in @($result.arms)) {
        if ($null -eq $arm.line_geometry_fresh_reopen -or [int]$arm.line_geometry_fresh_reopen.line_count -lt 1) {
            throw "Fresh-reopen line geometry missing for arm $($arm.arm)"
        }
    }

    Compress-Archive -Path (Join-Path $OutputRoot "*") -DestinationPath $ReturnZip -CompressionLevel Optimal
    if (-not (Test-Path -LiteralPath $ReturnZip -PathType Leaf)) { throw "Return ZIP was not created." }
    $zipSha = Get-Sha256 $ReturnZip

    Write-Host ""
    Write-Host "PARAGRAPH-LINE-SPACING-0P75-NATIVE-01 completed."
    Write-Host "Return ZIP: $ReturnZip"
    $structural = Get-Content -LiteralPath $structuralPath -Raw | ConvertFrom-Json
    Write-Host "Return ZIP SHA-256: $zipSha"
    Write-Host "Native 114300 authority candidate: $($structural.native_114300_authority_candidate)"
    Write-Host "Attach this ZIP directly to the Chaptera/ChatGPT project; do not commit the private PUB outputs."
    Write-Host ""
    Get-Content -LiteralPath $structuralPath -Raw
}
finally {
    if ($null -ne $fixture -and [bool]$fixture.Staged -and
        -not [string]::IsNullOrWhiteSpace([string]$fixture.Root) -and
        (Test-Path -LiteralPath ([string]$fixture.Root))) {
        Remove-Item -LiteralPath ([string]$fixture.Root) -Recurse -Force -ErrorAction SilentlyContinue
    }
    Pop-Location
}
