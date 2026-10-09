param(
    [Parameter(Mandatory = $true)][string]$SourcePath,
    [Parameter(Mandatory = $true)][string]$SavePath,
    [Parameter(Mandatory = $true)][string]$StageRoot,
    [string]$ExpectedPublisherVersion = '16.0',
    [string]$ExpectedBuildPrefix = '12527'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'PubRuntime.psm1') -Force

$phaseOrder = @(
    'source_verified', 'publisher_identity_begin', 'publisher_identity_ok',
    'source_application_begin', 'source_application_ready',
    'source_open_begin', 'source_open_ok',
    'source_saveas_begin', 'source_saveas_ok',
    'source_application_closed', 'native_file_verified',
    'native_reopen_application_begin', 'native_reopen_begin', 'native_reopen_ok',
    'source_immutable', 'pass'
)

function Write-ControlPhase {
    param([Parameter(Mandatory = $true)][string]$Phase)
    $index = [array]::IndexOf($phaseOrder, $Phase)
    if ($index -lt 0) { throw 'source_control_unknown_phase' }
    $path = Join-Path $StageRoot ('phase-{0:D2}-{1}.marker' -f $index, $Phase)
    if (Test-Path -LiteralPath $path) { throw 'source_control_duplicate_phase' }
    [System.IO.File]::WriteAllText($path, '', [System.Text.Encoding]::ASCII)
}

function Close-ControlDocument {
    param($Document)
    if ($null -eq $Document) { return }
    try { $Document.Close() } catch {}
    try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Document) } catch {}
}

$sourceBefore = Get-PubFileRecord $SourcePath
Write-ControlPhase 'source_verified'
Write-ControlPhase 'publisher_identity_begin'
$identity = Get-PubPublisherIdentity
if (-not $identity.available -or
    $identity.version.state -ne 'value' -or
    [string]$identity.version.value -ne $ExpectedPublisherVersion -or
    $identity.build.state -ne 'value' -or
    -not ([string]$identity.build.value).StartsWith($ExpectedBuildPrefix)) {
    throw 'source_control_publisher_identity_mismatch'
}
Write-ControlPhase 'publisher_identity_ok'

$application = $null
$document = $null
try {
    Write-ControlPhase 'source_application_begin'
    $application = New-PubPublisherApplication
    Write-ControlPhase 'source_application_ready'
    Write-ControlPhase 'source_open_begin'
    $document = $application.Open($SourcePath, $false, $false)
    Write-ControlPhase 'source_open_ok'
    Write-ControlPhase 'source_saveas_begin'
    $document.SaveAs($SavePath, 1, $false)
    Write-ControlPhase 'source_saveas_ok'
}
finally {
    Close-ControlDocument $document
    Close-PubPublisherApplication $application
}
Write-ControlPhase 'source_application_closed'

if (-not (Test-Path -LiteralPath $SavePath -PathType Leaf)) {
    throw 'source_control_saveas_output_missing'
}
$saved = Get-PubFileRecord $SavePath
if ([int64]$saved.size -le 0) { throw 'source_control_saveas_output_empty' }
Write-ControlPhase 'native_file_verified'

$reopenApplication = $null
$reopened = $null
try {
    Write-ControlPhase 'native_reopen_application_begin'
    $reopenApplication = New-PubPublisherApplication
    Write-ControlPhase 'native_reopen_begin'
    $reopened = $reopenApplication.Open($SavePath, $true, $false)
    if ($null -eq $reopened) { throw 'source_control_reopen_document_missing' }
    Write-ControlPhase 'native_reopen_ok'
}
finally {
    Close-ControlDocument $reopened
    Close-PubPublisherApplication $reopenApplication
}
$sourceAfter = Get-PubFileRecord $SourcePath
if ($sourceAfter.sha256 -ne $sourceBefore.sha256) { throw 'source_control_source_mutated' }
Write-ControlPhase 'source_immutable'
Write-ControlPhase 'pass'
