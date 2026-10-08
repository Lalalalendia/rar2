param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $RepoRoot

$Packet = Join-Path $RepoRoot "tools/research-runner/experiments/vba-call-bearing-native-estate-2019-01.packet.json"
$Operation = Join-Path $RepoRoot "tools/research-runner/operations/vba_call_bearing_native_estate_2019_01.ps1"
$ExpectedPacketBlob = "21147bbfe5029883bbfbe2dfe5893cf096e23537"
$ExpectedOperationBlob = "32d0cadb407ee21d8deffb29f8f228ec0f7e921c"

if (-not (Test-Path -LiteralPath $Packet)) { throw "Call-bearing VBA packet missing: $Packet" }
if (-not (Test-Path -LiteralPath $Operation)) { throw "Call-bearing VBA operation missing: $Operation" }

$PacketBlob = (git hash-object -- $Packet).Trim()
$OperationBlob = (git hash-object -- $Operation).Trim()
if ($PacketBlob -ne $ExpectedPacketBlob) {
    throw "Packet blob mismatch: expected $ExpectedPacketBlob got $PacketBlob"
}
if ($OperationBlob -ne $ExpectedOperationBlob) {
    throw "Operation blob mismatch: expected $ExpectedOperationBlob got $OperationBlob"
}

python tools/research-runner/validate_packet.py --packet "tools/research-runner/experiments/vba-call-bearing-native-estate-2019-01.packet.json" --expected-environment publisher-2019
if ($LASTEXITCODE -ne 0) { throw "Packet validation failed." }

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot "out/pub-research/vba-call-bearing-native-estate-2019-01"
}

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "prepare_native_run failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Operation -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "call-bearing VBA estate operation failed." }

powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $OutputRoot
if ($LASTEXITCODE -ne 0) { throw "finalize_native_run failed." }

$result = Join-Path $OutputRoot "analysis/vba-call-bearing-native-estate-2019-01.json"
$scan = Join-Path $OutputRoot "analysis/vba-call-bearing-native-estate-scan.json"
$log = Join-Path $OutputRoot "logs/vba-call-bearing-native-estate-2019-01.txt"
Get-Content -LiteralPath $result -Raw
Get-Content -LiteralPath $scan -Raw
Get-Content -LiteralPath $log -Raw
