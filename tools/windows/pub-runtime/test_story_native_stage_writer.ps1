# Source-safe PowerShell 5.1 smoke for the *actual* native stage writer.
# No Microsoft Publisher COM, PUB source bytes, path uploads or self-hosted runner.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sourceScript = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot 'run_story_native_roundtrip.ps1')).Path
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    $sourceScript, [ref]$tokens, [ref]$parseErrors
)
if (@($parseErrors).Count -ne 0) {
    throw 'native_stage_source_parse_failed'
}
$stageFunctions = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -ceq 'Write-PubNativeStage'
}, $true))
if ($stageFunctions.Count -ne 1) {
    throw 'native_stage_writer_function_not_unique'
}

$output = Join-Path ([System.IO.Path]::GetTempPath()) (
    'chaptera-native-stage-smoke-' + [guid]::NewGuid().ToString('N')
)
$manifest = [pscustomobject]@{
    source_sha256 = ('a' * 64)
    candidate_sha256 = ('b' * 64)
}
New-Item -ItemType Directory -Force -Path $output | Out-Null
try {
    # Import exactly the production function from its AST. Do not execute the
    # surrounding native script, even if Publisher happens to be installed.
    . ([scriptblock]::Create($stageFunctions[0].Extent.Text))
    $phases = @(
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
    foreach ($phase in $phases) {
        Write-PubNativeStage -Phase $phase
        $stagePath = Join-Path $output 'native-roundtrip-stage.json'
        if (-not (Test-Path -LiteralPath $stagePath -PathType Leaf)) {
            throw 'native_stage_receipt_missing'
        }
        $stage = Get-Content -LiteralPath $stagePath -Raw | ConvertFrom-Json
        if ([string]$stage.schema -cne 'chaptera.pub-native-story-stage.v1' -or
            [string]$stage.phase -cne $phase -or
            [string]$stage.source_sha256 -cne ('a' * 64) -or
            [string]$stage.candidate_sha256 -cne ('b' * 64) -or
            @(($stage | Get-Member -MemberType NoteProperty)).Count -ne 4) {
            throw "native_stage_transition_mismatch: $phase"
        }
        if (Test-Path -LiteralPath (Join-Path $output 'native-roundtrip-stage.next.json')) {
            throw "native_stage_temporary_file_leaked: $phase"
        }
    }
    # Invalid phases must fail closed and leave the last valid durable stage.
    $invalidRejected = $false
    try {
        Write-PubNativeStage -Phase 'unapproved_phase'
    }
    catch {
        $invalidRejected = $true
    }
    $final = Get-Content -LiteralPath (Join-Path $output 'native-roundtrip-stage.json') -Raw |
        ConvertFrom-Json
    if (-not $invalidRejected -or [string]$final.phase -cne 'pass') {
        throw 'native_stage_invalid_phase_not_rejected'
    }
    Write-Host ("native stage writer Windows PowerShell 5.1 smoke PASS: {0} stages" -f $phases.Count)
}
finally {
    Remove-Item -LiteralPath $output -Recurse -Force -ErrorAction SilentlyContinue
}
