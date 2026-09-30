param(
    [Parameter(Mandatory = $true)]
    [string]$WitnessDir,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [string]$ExpectedPublisherVersion = "16.0",
    [string]$ExpectedBuildPrefix = "12527"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
Import-Module (Join-Path $repoRoot "tools\windows\pub-runtime\PubRuntime.psm1") -Force

$ExpectedWitnessSha256 = @(
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff"
)

function Close-PubDocument {
    param($Document)
    if ($null -eq $Document) { return }
    try { $Document.Close() } catch {}
    try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Document) } catch {}
}

function Release-PubComObject {
    param($Object)
    if ($null -eq $Object) { return }
    try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object) } catch {}
}

function Get-Utf16LeDigest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Text
    )

    # Encoding.Unicode is UTF-16LE without a BOM when GetBytes() is used.
    $bytes = [System.Text.Encoding]::Unicode.GetBytes($Text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digest = $sha.ComputeHash($bytes)
    }
    finally {
        $sha.Dispose()
    }
    $hex = ([System.BitConverter]::ToString($digest)).Replace("-", "").ToLowerInvariant()
    return [ordered]@{
        utf16_code_units = [int]($bytes.Length / 2)
        utf16le_sha256 = $hex
    }
}

$publisher = Get-PubPublisherIdentity
if (-not $publisher.available) {
    throw "Microsoft Publisher COM automation is unavailable"
}
if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) {
    throw "Publisher version mismatch: expected $ExpectedPublisherVersion"
}
if ($publisher.build.state -ne "value" -or -not ([string]$publisher.build.value).StartsWith($ExpectedBuildPrefix)) {
    throw "Publisher build mismatch: expected prefix $ExpectedBuildPrefix, got $($publisher.build.value)"
}

$root = (Resolve-Path -LiteralPath $WitnessDir).Path
$paths = @(Get-ChildItem -LiteralPath $root -File -Filter "*.pub" | Sort-Object FullName)
if ($paths.Count -ne 4) {
    throw "Expected exactly four unresolved sentinel PUBs, found $($paths.Count)"
}

$records = @()
foreach ($item in $paths) {
    $records += Get-PubFileRecord $item.FullName
}

$expectedSet = @{}
foreach ($sha in $ExpectedWitnessSha256) { $expectedSet[$sha] = $true }
$observedSet = @{}
foreach ($record in $records) {
    if ($observedSet.ContainsKey($record.sha256)) {
        throw "Duplicate source SHA in witness directory: $($record.sha256)"
    }
    $observedSet[$record.sha256] = $true
    if (-not $expectedSet.ContainsKey($record.sha256)) {
        throw "Unexpected PUB witness SHA: $($record.sha256)"
    }
}
foreach ($sha in $ExpectedWitnessSha256) {
    if (-not $observedSet.ContainsKey($sha)) {
        throw "Missing required unresolved sentinel SHA: $sha"
    }
}

$witnesses = @()
foreach ($record in ($records | Sort-Object sha256)) {
    $application = $null
    $document = $null
    $storiesCollection = $null
    $storyRows = @()

    try {
        $application = New-PubPublisherApplication
        # Existing Chaptera native harness uses Open(path, readOnly, addToRecentFiles).
        # This oracle never calls Save or SaveAs.
        $document = $application.Open($record.path, $true, $false)
        $storiesCollection = $document.Stories
        $storyCount = [int]$storiesCollection.Count

        for ($storyIndex = 1; $storyIndex -le $storyCount; $storyIndex++) {
            $story = $null
            $range = $null
            try {
                $story = $storiesCollection.Item($storyIndex)
                $range = $story.TextRange
                $text = [string]$range.Text
                $digest = Get-Utf16LeDigest -Text $text
                $storyRows += [ordered]@{
                    com_ordinal = $storyIndex - 1
                    utf16_code_units = [int]$digest.utf16_code_units
                    utf16le_sha256 = [string]$digest.utf16le_sha256
                }
                $text = $null
            }
            finally {
                Release-PubComObject $range
                Release-PubComObject $story
            }
        }
    }
    finally {
        Release-PubComObject $storiesCollection
        Close-PubDocument $document
        Close-PubPublisherApplication $application
    }

    $after = Get-PubFileRecord $record.path
    if ($after.sha256 -ne $record.sha256 -or $after.size -ne $record.size) {
        throw "Source PUB changed during read-only oracle: $($record.sha256)"
    }

    $sumUtf16 = 0
    foreach ($storyRow in $storyRows) {
        $sumUtf16 += [int64]$storyRow.utf16_code_units
    }

    $witnesses += [ordered]@{
        source_sha256 = $record.sha256
        story_count = $storyRows.Count
        story_utf16_sum = $sumUtf16
        stories = $storyRows
        source_unchanged = $true
    }
}

$receipt = [ordered]@{
    schema = "chaptera.quill-story-readonly-oracle.v1"
    publisher = [ordered]@{
        version = [string]$publisher.version.value
        build = [string]$publisher.build.value
        name = if ($publisher.name.state -eq "value") { [string]$publisher.name.value } else { $null }
    }
    witness_count = $witnesses.Count
    witnesses = $witnesses
    evidence_boundary = "Exact four SHA-addressed #337 supersets; read-only Publisher Open only; receipt contains Story UTF-16 lengths and SHA-256 digests, never document text; no Save/SaveAs and no 1,050-corpus materialization."
}

Write-PubJson -Value $receipt -Path $OutputPath
Write-Host "PUB read-only Story oracle PASS"
Write-Host "Receipt: $OutputPath"
