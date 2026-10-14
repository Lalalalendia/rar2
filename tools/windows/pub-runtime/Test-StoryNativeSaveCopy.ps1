# Research only: Publisher Save() on an exact disposable PUB copy.
# Never use this result as a SaveAs or product Download PUB acceptance.
param(
    [Parameter(Mandatory = $true)][ValidateSet('Original', 'Candidate')][string]$Arm,
    [Parameter(Mandatory = $true)][string]$BundleDir,
    [Parameter(Mandatory = $true)][string]$WorkingPath,
    [Parameter(Mandatory = $true)][string]$StageRoot,
    [Parameter(Mandatory = $true)][string]$ReceiptPath,
    [string]$ExpectedPublisherVersion = '16.0',
    [string]$ExpectedBuildPrefix = '12527'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'PubRuntime.psm1') -Force

$manifest = Get-Content -LiteralPath (Join-Path $BundleDir 'handoff.json') -Raw | ConvertFrom-Json
if ([string]$manifest.schema -cne 'chaptera.pub-native-story-handoff.v1') {
    throw 'save_copy_bad_manifest_schema'
}
$expectedSource = '424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc'
$expectedCandidate = 'a92543b6f2b6ac3a8ae2481e15a2188a338ddc2a92832580f8987079fa4f70f8'
if ([string]$manifest.source_sha256 -cne $expectedSource -or
    [string]$manifest.candidate_sha256 -cne $expectedCandidate) {
    throw 'save_copy_pinned_input_mismatch'
}
$sourcePath = Join-Path $BundleDir 'source.pub'
$candidatePath = Join-Path $BundleDir 'candidate.pub'
$sourceBefore = Get-PubFileRecord $sourcePath
$candidateBefore = Get-PubFileRecord $candidatePath
$workingBefore = Get-PubFileRecord $WorkingPath
$expectedWorking = if ($Arm -eq 'Original') { $expectedSource } else { $expectedCandidate }
if ($sourceBefore.sha256 -cne $expectedSource -or
    $candidateBefore.sha256 -cne $expectedCandidate -or
    $workingBefore.sha256 -cne $expectedWorking) {
    throw 'save_copy_preflight_hash_mismatch'
}

$phases = @(
    'work_copy_verified', 'publisher_identity_begin', 'publisher_identity_ok',
    'application_begin', 'application_ready', 'open_begin', 'open_ok',
    'save_begin', 'save_ok', 'application_closed', 'saved_file_verified',
    'reopen_application_begin', 'reopen_begin', 'reopen_ok', 'reopen_application_closed',
    'reader_begin', 'reader_ok', 'input_immutable', 'pass'
)
function Write-SavePhase {
    param([Parameter(Mandatory = $true)][string]$Phase)
    $index = [array]::IndexOf($phases, $Phase)
    if ($index -lt 0) { throw 'save_copy_unapproved_phase' }
    $path = Join-Path $StageRoot ('phase-{0:D2}-{1}.marker' -f $index, $Phase)
    if (Test-Path -LiteralPath $path) { throw 'save_copy_duplicate_phase' }
    [System.IO.File]::WriteAllText($path, '', [System.Text.Encoding]::ASCII)
}
function Close-SaveDocument {
    param($Document)
    if ($null -eq $Document) { return }
    try { $Document.Close() } catch {}
    try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Document) } catch {}
}
function Get-SaveSnapshot {
    param([Parameter(Mandatory = $true)]$Document)
    $pageCount = [int]$Document.Pages.Count
    $shapeCount = 0
    for ($i = 1; $i -le $pageCount; $i++) {
        $shapeCount += [int]$Document.Pages.Item($i).Shapes.Count
    }
    return [ordered]@{ pages = $pageCount; shapes = $shapeCount }
}

Write-SavePhase 'work_copy_verified'
Write-SavePhase 'publisher_identity_begin'
$identity = Get-PubPublisherIdentity
if (-not $identity.available -or
    $identity.version.state -ne 'value' -or
    [string]$identity.version.value -cne $ExpectedPublisherVersion -or
    $identity.build.state -ne 'value' -or
    -not ([string]$identity.build.value).StartsWith($ExpectedBuildPrefix)) {
    throw 'save_copy_publisher_identity_mismatch'
}
Write-SavePhase 'publisher_identity_ok'

$application = $null
$document = $null
$before = $null
try {
    Write-SavePhase 'application_begin'
    $application = New-PubPublisherApplication
    Write-SavePhase 'application_ready'
    Write-SavePhase 'open_begin'
    $document = $application.Open($WorkingPath, $false, $false)
    if ($null -eq $document) { throw 'save_copy_open_missing_document' }
    Write-SavePhase 'open_ok'
    $before = Get-SaveSnapshot $document
    Write-SavePhase 'save_begin'
    $document.Save()
    Write-SavePhase 'save_ok'
}
finally {
    Close-SaveDocument $document
    Close-PubPublisherApplication $application
}
Write-SavePhase 'application_closed'

# Fingerprint only after Close/Quit: Publisher holds open files locked.
$saved = Get-PubFileRecord $WorkingPath
if ([int64]$saved.size -le 0) { throw 'save_copy_output_empty' }
Write-SavePhase 'saved_file_verified'

$reopenApplication = $null
$reopened = $null
$after = $null
try {
    Write-SavePhase 'reopen_application_begin'
    $reopenApplication = New-PubPublisherApplication
    Write-SavePhase 'reopen_begin'
    $reopened = $reopenApplication.Open($WorkingPath, $true, $false)
    if ($null -eq $reopened) { throw 'save_copy_reopen_missing_document' }
    $after = Get-SaveSnapshot $reopened
    Write-SavePhase 'reopen_ok'
}
finally {
    Close-SaveDocument $reopened
    Close-PubPublisherApplication $reopenApplication
}
Write-SavePhase 'reopen_application_closed'
if ($before.pages -ne $after.pages -or $before.shapes -ne $after.shapes) {
    throw 'save_copy_geometry_count_mismatch'
}

$readerVerified = $false
$semantic = $null
if ($Arm -eq 'Candidate') {
    Write-SavePhase 'reader_begin'
    $repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..')).Path
    Push-Location $repoRoot
    try {
        $verifyOutput = & cargo run --quiet --manifest-path vendor/producer-a/Cargo.toml -p pub-writer --bin pub_story_native_handoff -- verify --pub $WorkingPath --manifest (Join-Path $BundleDir 'handoff.json')
        if ($LASTEXITCODE -ne 0) { throw 'save_copy_current_reader_failed' }
    }
    finally { Pop-Location }
    $semantic = ($verifyOutput -join [Environment]::NewLine) | ConvertFrom-Json
    if ([string]$semantic.status -cne 'valid' -or
        [int]$semantic.story_syid -ne [int]$manifest.story_syid -or
        [int]$semantic.story_utf16_len -ne [int]$manifest.after_utf16_len -or
        [string]$semantic.story_text_sha256 -cne [string]$manifest.after_text_sha256) {
        throw 'save_copy_semantic_survival_failed'
    }
    $readerVerified = $true
    Write-SavePhase 'reader_ok'
}

$sourceAfter = Get-PubFileRecord $sourcePath
$candidateAfter = Get-PubFileRecord $candidatePath
if ($sourceAfter.sha256 -cne $sourceBefore.sha256 -or
    $candidateAfter.sha256 -cne $candidateBefore.sha256) {
    throw 'save_copy_immutable_input_changed'
}
Write-SavePhase 'input_immutable'

$receipt = [ordered]@{
    schema = 'chaptera.pub-native-story-save-copy-research.v1'
    arm = $Arm.ToLowerInvariant()
    publisher_version = [string]$identity.version.value
    publisher_build = [string]$identity.build.value
    source_sha256 = $expectedSource
    candidate_sha256 = $expectedCandidate
    working_before_sha256 = $expectedWorking
    native_saved_sha256 = $saved.sha256
    native_saved_bytes = [int64]$saved.size
    initial_snapshot = $before
    reopen_snapshot = $after
    save_completed = $true
    fresh_reopen = $true
    original_inputs_unchanged = $true
    current_reader_verified = $readerVerified
    semantic_story_sha256 = if ($readerVerified) { [string]$semantic.story_text_sha256 } else { $null }
    saveas_evaluated = $false
    product_save_authorized = $false
}
$receipt | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $ReceiptPath -Encoding utf8
Write-SavePhase 'pass'
Write-Host 'Source-safe Publisher research Save on disposable copy PASS'
