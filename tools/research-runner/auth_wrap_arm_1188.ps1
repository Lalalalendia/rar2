param(
    [Parameter(Mandatory = $true)][string]$TrustedCommitSha,
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [Parameter(Mandatory = $true)][ValidateSet('A','B')][string]$Family
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$expectedPacket = Join-Path $repo 'tools/research-runner/experiments/auth-wrap-01.packet.json'
if ([IO.Path]::GetFullPath($PacketPath) -cne [IO.Path]::GetFullPath($expectedPacket)) {
    throw 'Child packet path differs from trusted research packet'
}
if ($TrustedCommitSha -cnotmatch '^[0-9a-f]{40}$') { throw 'Unpinned AUTH-WRAP child commit' }
if (-not $env:PUB_RESEARCH_PACKET_SHA256) { throw 'Missing parent-validated packet digest' }
if (-not $env:PUB_RESEARCH_AUTH_WRAP_ORACLE_EXE -or
    -not (Test-Path -LiteralPath $env:PUB_RESEARCH_AUTH_WRAP_ORACLE_EXE -PathType Leaf)) {
    throw 'Missing locally built source-pinned AUTH-WRAP oracle'
}
Push-Location $repo
try {
    $actualCommit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actualCommit -cne $TrustedCommitSha) {
        throw 'AUTH-WRAP child checkout moved from trusted commit'
    }
    $digest = (Get-FileHash -LiteralPath $expectedPacket -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($digest -cne $env:PUB_RESEARCH_PACKET_SHA256.ToLowerInvariant()) {
        throw 'AUTH-WRAP child packet digest changed after launcher preflight'
    }
    $packet = Get-Content -LiteralPath $expectedPacket -Raw | ConvertFrom-Json
    if ([string]$packet.id -cne 'AUTH-WRAP-01' -or -not [bool]$packet.reset.required) {
        throw 'AUTH-WRAP child packet identity/reset requirement mismatch'
    }
    $env:PUB_RESEARCH_AUTH_WRAP_FAMILY = $Family
    Remove-Item Env:PUB_RESEARCH_FIXTURE -ErrorAction SilentlyContinue
    Remove-Item Env:PUB_RESEARCH_PUBLISHER_EXE -ErrorAction SilentlyContinue
    & (Join-Path $repo 'tools/research-runner/prepare_native_run.ps1') -PacketPath $expectedPacket -OutputRoot $OutputRoot
    # The reset provider may leave MSPUB running or a user may open Publisher
    # while the VM is being restored. Never attach to that ambient COM instance.
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw 'Publisher active after reset provider; refusing ambient COM instance'
    }
    $fixture = [string]$env:PUB_RESEARCH_FIXTURE
    $publisher = [string]$env:PUB_RESEARCH_PUBLISHER_EXE
    if (-not $fixture -or -not (Test-Path -LiteralPath $fixture -PathType Leaf) -or
        (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash.ToLowerInvariant() -cne
            [string]$packet.fixture.sha256) {
        throw 'Prepared fixture binding missing or changed after native preflight'
    }
    if (-not $publisher -or -not (Test-Path -LiteralPath $publisher -PathType Leaf) -or
        (Get-FileHash -LiteralPath $publisher -Algorithm SHA256).Hash.ToLowerInvariant() -cne
            [string]$packet.publisher.exe_sha256) {
        throw 'Prepared Publisher executable binding missing or changed after native preflight'
    }
    & (Join-Path $repo 'tools/research-runner/operations/auth_wrap_01.ps1') -PacketPath $expectedPacket -OutputRoot $OutputRoot
    & (Join-Path $repo 'tools/research-runner/finalize_native_run.ps1') -PacketPath $expectedPacket -OutputRoot $OutputRoot
} finally {
    Pop-Location
}
