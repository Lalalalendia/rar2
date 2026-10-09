Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Exact order, source-safe. Native Publisher COM is never invoked from this module.
$script:NativeStoryPhaseOrder = @(
    'inputs_verified',
    'publisher_identity_begin',
    'publisher_identity_verified',
    'candidate_application_begin',
    'candidate_application_ready',
    'candidate_open_begin',
    'candidate_open_ok',
    'saveas_begin',
    'saveas_ok',
    'candidate_application_closed',
    'reopen_application_begin',
    'reopen_application_ready',
    'native_reopen_begin',
    'native_reopen_ok',
    'native_application_closed',
    'source_immutable',
    'reader_verify_begin',
    'reader_verify_ok',
    'pass'
)

function Write-PubNativePhase {
    param(
        [Parameter(Mandatory = $true)][string]$OutputRoot,
        [Parameter(Mandatory = $true)][string]$Phase,
        [Parameter(Mandatory = $true)][string]$SourceSha256,
        [Parameter(Mandatory = $true)][string]$CandidateSha256
    )
    if ($Phase -cnotin $script:NativeStoryPhaseOrder) {
        throw 'unsupported_native_phase'
    }
    if ($SourceSha256 -cnotmatch '^[0-9a-f]{64}$' -or
        $CandidateSha256 -cnotmatch '^[0-9a-f]{64}$') {
        throw 'native_phase_sha_not_lowercase_sha256'
    }
    if (-not (Test-Path -LiteralPath $OutputRoot -PathType Container)) {
        throw 'native_phase_output_root_missing'
    }
    $path = Join-Path $OutputRoot ("native-roundtrip-stage-{0}.json" -f $Phase)
    if (Test-Path -LiteralPath $path) {
        throw 'native_phase_duplicate_is_forbidden'
    }
    $payload = [ordered]@{
        schema = 'chaptera.pub-native-story-stage.v1'
        phase = $Phase
        source_sha256 = $SourceSha256
        candidate_sha256 = $CandidateSha256
    }
    $payload | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $path -Encoding utf8
}

function Get-PubNativeLastPhase {
    param(
        [Parameter(Mandatory = $true)][string]$OutputRoot
    )
    for ($index = $script:NativeStoryPhaseOrder.Count - 1; $index -ge 0; $index--) {
        $phase = [string]$script:NativeStoryPhaseOrder[$index]
        $path = Join-Path $OutputRoot ("native-roundtrip-stage-{0}.json" -f $phase)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            continue
        }
        $receipt = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
        if ([string]$receipt.schema -ne 'chaptera.pub-native-story-stage.v1' -or
            [string]$receipt.phase -cne $phase -or
            [string]$receipt.source_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
            [string]$receipt.candidate_sha256 -cnotmatch '^[0-9a-f]{64}$') {
            throw 'native_phase_receipt_invalid'
        }
        return [ordered]@{
            phase = $phase
            source_sha256 = [string]$receipt.source_sha256
            candidate_sha256 = [string]$receipt.candidate_sha256
        }
    }
    return $null
}

Export-ModuleMember -Function 'Write-PubNativePhase', 'Get-PubNativeLastPhase'
