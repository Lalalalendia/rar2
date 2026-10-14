param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $RepoRoot

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/page-background-auth-read-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/page_background_auth_read_01.ps1"
$ExpectedPacketBlob = "e17d2e199055fa435ed91f2a808f1473da51e565"
$ExpectedOperationBlob = "4af9e4d722f93fec7a4da2f140da48f71277507d"

foreach ($entry in @(
    [pscustomobject]@{ Path = $Packet; Expected = $ExpectedPacketBlob; Label = "packet" },
    [pscustomobject]@{ Path = $Operation; Expected = $ExpectedOperationBlob; Label = "operation" }
)) {
    if (-not (Test-Path -LiteralPath $entry.Path -PathType Leaf)) {
        throw "T842 $($entry.Label) missing: $($entry.Path)"
    }
    $actual = (git hash-object -- $entry.Path).Trim()
    if ($actual -ne $entry.Expected) {
        throw "T842 $($entry.Label) blob mismatch: expected $($entry.Expected) got $actual"
    }
}

python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/page-background-auth-read-01.packet.json" --expected-environment publisher-2019
if ($LASTEXITCODE -ne 0) { throw "T842 packet validation failed." }

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
    $OutputRoot = Join-Path $RepoRoot ("out/pub-research/page-background-auth-read-01-" + $stamp)
} elseif (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot $OutputRoot
}

if (Test-Path -LiteralPath $OutputRoot) {
    $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop)
    if ($existing.Count -ne 0) {
        throw "T842 OutputRoot is not empty; use a fresh path: $OutputRoot"
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T842 prepare_native_run failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T842 Publisher operation failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "T842 finalize_native_run failed." }

Get-Content -LiteralPath (Join-Path $OutputRoot "analysis/page-background-auth-read-01.json") -Raw
Get-Content -LiteralPath (Join-Path $OutputRoot "logs/page-background-auth-read-01.txt") -Raw
