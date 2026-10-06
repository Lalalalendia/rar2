param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $RepoRoot

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/ignore-master-auth-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/publisher_ignore_master_auth_01.ps1"
$Analyzer = Join-Path $RepoRoot "tools/research-runner/analysis/ignore_master_auth_01_blast_radius.py"

$ExpectedPacketBlob = "5bd3790f430e6d95947142d57cd4b1ea9d0bbcae"
$ExpectedOperationBlob = "fb519d47a4ae85b68ce891895baec45a9123a01b"
$ExpectedAnalyzerBlob = "ae2ce9de0cc30d301cd394490f8737a3f2f2d331"

foreach ($entry in @(
    [pscustomobject]@{ Path = $Packet; Expected = $ExpectedPacketBlob; Label = "packet" },
    [pscustomobject]@{ Path = $Operation; Expected = $ExpectedOperationBlob; Label = "operation" },
    [pscustomobject]@{ Path = $Analyzer; Expected = $ExpectedAnalyzerBlob; Label = "analyzer" }
)) {
    if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) {
        throw "T828 $($entry.Label) missing: $($entry.Path)"
    }
    $actual = (git hash-object -- $entry.Path).Trim()
    if ($actual -ne $entry.Expected) {
        throw "T828 $($entry.Label) blob mismatch: expected $($entry.Expected) got $actual"
    }
}

python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/ignore-master-auth-01.packet.json" --expected-environment publisher-2019
if ($LASTEXITCODE -ne 0) { throw "T828 packet validation failed." }

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
    $OutputRoot = Join-Path $RepoRoot ("out/pub-research/ignore-master-auth-01-" + $stamp)
} elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot $OutputRoot
}

if (Test-Path -LiteralPath $OutputRoot) {
    $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
    if ($existing.Count -ne 0) {
        throw "T828 OutputRoot is not empty; use a fresh path so previous evidence is preserved: $OutputRoot"
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T828 prepare_native_run failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T828 Publisher operation failed." }

python $Analyzer --output-root $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T828 analysis failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T828 finalize_native_run failed." }

Get-Content -LiteralPath (Join-Path $OutputRoot "analysis/ignore-master-auth-01.json") -Raw
Get-Content -LiteralPath (Join-Path $OutputRoot "analysis/ignore-master-auth-01-blast-radius.json") -Raw
Get-Content -LiteralPath (Join-Path $OutputRoot "logs/ignore-master-auth-01.txt") -Raw
