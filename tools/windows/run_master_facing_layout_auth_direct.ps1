param(
    [string]$FixtureRoot = "",
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $RepoRoot

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/master-facing-layout-auth-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/publisher_master_facing_layout_auth_01.ps1"
$Analyzer = Join-Path $RepoRoot "tools/research-runner/analysis/master_facing_layout_auth_01.py"
$Prepare = Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1"
$Finalize = Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1"

$ExpectedPacketBlob = "fc8eaa28b9c133578d6c08444cf655be273fac20"
$ExpectedOperationBlob = "a291712cb8814c86c54c173df918c03f16abeecf"
$ExpectedAnalyzerBlob = "e36bd9fb52f97b39294797ccd0f50da3c36ba79b"
$ExpectedFixtureSha256 = "5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b"
$ExpectedFixtureRelativePath = "pubgen-create-20260923/minimal-blank-v1-generated.pub"

function Assert-LastExit([string]$Label) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

foreach ($entry in @(
    [pscustomobject]@{ Path = $Packet; Expected = $ExpectedPacketBlob; Label = "packet" },
    [pscustomobject]@{ Path = $Operation; Expected = $ExpectedOperationBlob; Label = "operation" },
    [pscustomobject]@{ Path = $Analyzer; Expected = $ExpectedAnalyzerBlob; Label = "analyzer" }
)) {
    if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) {
        throw "T825 $($entry.Label) missing: $($entry.Path)"
    }
    $actual = (& git hash-object -- $entry.Path).Trim()
    Assert-LastExit "git hash-object $($entry.Label)"
    if ($actual -ne $entry.Expected) {
        throw "T825 $($entry.Label) blob mismatch: expected $($entry.Expected) got $actual"
    }
}

python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/master-facing-layout-auth-01.packet.json" --expected-environment publisher-2019
Assert-LastExit "T825 packet validation"

$fixtureRoots = New-Object System.Collections.Generic.List[string]
foreach ($candidate in @(
    $FixtureRoot,
    [string]$env:PUB_RESEARCH_FIXTURE_ROOT,
    (Join-Path $RepoRoot "realtest"),
    $(if (-not [string]::IsNullOrWhiteSpace([string]$env:USERPROFILE)) { Join-Path $env:USERPROFILE "rar2\realtest" } else { "" })
)) {
    if ([string]::IsNullOrWhiteSpace([string]$candidate)) { continue }
    if (-not [IO.Path]::IsPathRooted([string]$candidate)) {
        $candidate = Join-Path $RepoRoot ([string]$candidate)
    }
    if (-not (Test-Path -LiteralPath $candidate -PathType Container)) { continue }
    $full = (Resolve-Path -LiteralPath $candidate).Path
    $duplicate = $false
    foreach ($existing in $fixtureRoots) {
        if ([string]::Equals($existing,$full,[StringComparison]::OrdinalIgnoreCase)) {
            $duplicate = $true
            break
        }
    }
    if (-not $duplicate) { $fixtureRoots.Add($full) }
}

$resolvedFixtureRoot = $null
$resolvedFixture = $null
foreach ($root in $fixtureRoots) {
    $candidate = Join-Path $root $ExpectedFixtureRelativePath
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
    $sha = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sha -ne $ExpectedFixtureSha256) {
        throw "T825 fixture path exists but SHA mismatches: $candidate expected $ExpectedFixtureSha256 got $sha"
    }
    $resolvedFixtureRoot = $root
    $resolvedFixture = (Resolve-Path -LiteralPath $candidate).Path
    break
}

if ($null -eq $resolvedFixture) {
    throw "T825 exact fixture is absent. Pass -FixtureRoot or set PUB_RESEARCH_FIXTURE_ROOT to the root containing $ExpectedFixtureRelativePath."
}

$env:PUB_RESEARCH_FIXTURE_ROOT = $resolvedFixtureRoot
Write-Host "T825 fixture verified: $resolvedFixture"

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
    $OutputRoot = Join-Path $RepoRoot ("out/pub-research/master-facing-layout-auth-01-" + $stamp)
} elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot $OutputRoot
}

if (Test-Path -LiteralPath $OutputRoot) {
    $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
    if ($existing.Count -ne 0) {
        throw "T825 OutputRoot is not empty; use a fresh path so previous evidence is preserved: $OutputRoot"
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Prepare -PacketPath $Packet -OutputRoot $OutputRoot
Assert-LastExit "T825 prepare_native_run"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
Assert-LastExit "T825 Publisher operation"

python $Analyzer --output-root $OutputRoot
Assert-LastExit "T825 topology analyzer"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Finalize -PacketPath $Packet -OutputRoot $OutputRoot
Assert-LastExit "T825 finalize_native_run"

foreach ($path in @(
    (Join-Path $OutputRoot "analysis/master-facing-layout-auth-01.json"),
    (Join-Path $OutputRoot "analysis/master-facing-layout-auth-01-blast-radius.json"),
    (Join-Path $OutputRoot "logs/master-facing-layout-auth-01.txt"),
    (Join-Path $OutputRoot "environment.json"),
    (Join-Path $OutputRoot "evidence-manifest.json")
)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "T825 required evidence missing: $path"
    }
}

Get-Content -LiteralPath (Join-Path $OutputRoot "analysis/master-facing-layout-auth-01.json") -Raw
Get-Content -LiteralPath (Join-Path $OutputRoot "analysis/master-facing-layout-auth-01-blast-radius.json") -Raw
Get-Content -LiteralPath (Join-Path $OutputRoot "logs/master-facing-layout-auth-01.txt") -Raw
