param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $RepoRoot

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/borderart-auth-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/publisher_borderart_auth_01.ps1"
$ExpectedPacketBlob = "234d200ccadeedbc1ed12b2d43cbc15e853ca1fc"
$ExpectedOperationBlob = "8e105d054085793853676c0ad7c19e8586b3769e"

if (-not (Test-Path -LiteralPath $Packet)) { throw "BorderArt packet missing: $Packet" }
if (-not (Test-Path -LiteralPath $Operation)) { throw "BorderArt operation missing: $Operation" }

$PacketBlob = (git hash-object -- $Packet).Trim()
$OperationBlob = (git hash-object -- $Operation).Trim()
if ($PacketBlob -ne $ExpectedPacketBlob) {
    throw "Packet blob mismatch: expected $ExpectedPacketBlob got $PacketBlob"
}
if ($OperationBlob -ne $ExpectedOperationBlob) {
    throw "Operation blob mismatch: expected $ExpectedOperationBlob got $OperationBlob"
}

python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/borderart-auth-01.packet.json" --expected-environment publisher-2019
if ($LASTEXITCODE -ne 0) { throw "Packet validation failed." }

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot "out/pub-research/borderart-auth-01"
}

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "prepare_native_run failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "BorderArt operation failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "finalize_native_run failed." }

$result = Join-Path $OutputRoot "analysis/borderart-auth-01.json"
$log = Join-Path $OutputRoot "logs/borderart-auth-01.txt"
Get-Content -LiteralPath $result -Raw
Get-Content -LiteralPath $log -Raw
