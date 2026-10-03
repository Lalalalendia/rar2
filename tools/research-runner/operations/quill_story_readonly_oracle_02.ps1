param(
    [Parameter(Mandatory = $true)]
    [string]$PacketPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot
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
$LalamuCommit = "f78cc6f455f4dc222868f9cc035511a6ca7a91ea"
$AnalysisPath = Join-Path $OutputRoot "analysis\quill-story-readonly-oracle.json"
$LogPath = Join-Path $OutputRoot "logs\quill-story-readonly-oracle.txt"
$TempRoot = Join-Path $env:RUNNER_TEMP ("quill-story-readonly-oracle-" + [guid]::NewGuid().ToString("N"))
$WitnessDir = Join-Path $TempRoot "witness"

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

New-Item -ItemType Directory -Force -Path $WitnessDir | Out-Null
try {
    foreach ($sha in $ExpectedWitnessSha256) {
        $url = "https://raw.githubusercontent.com/Lalalalendia/lalamu/$LalamuCommit/pub-corpus/corpus/native/unclassified/$sha.pub"
        $target = Join-Path $WitnessDir "$sha.pub"
        Invoke-WebRequest -Uri $url -OutFile $target -UseBasicParsing
        $record = Get-PubFileRecord $target
        if ($record.sha256 -ne $sha) {
            throw "Downloaded witness SHA mismatch for $sha"
        }
    }

    $paths = @(Get-ChildItem -LiteralPath $WitnessDir -File -Filter "*.pub" | Sort-Object Name)
    if ($paths.Count -ne 4) {
        throw "Expected exactly four unresolved sentinel PUBs, found $($paths.Count)"
    }

    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) {
        throw "Microsoft Publisher COM automation is unavailable"
    }

    $witnesses = @()
    foreach ($item in $paths) {
        $before = Get-PubFileRecord $item.FullName
        $application = $null
        $document = $null
        $storiesCollection = $null
        $storyRows = @()

        try {
            $application = New-PubPublisherApplication
            $document = $application.Open($before.path, $true, $false)
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

        $after = Get-PubFileRecord $item.FullName
        if ($after.sha256 -ne $before.sha256 -or $after.size -ne $before.size) {
            throw "Source PUB changed during read-only oracle for $($before.sha256)"
        }

        $sumUtf16 = 0
        foreach ($storyRow in $storyRows) {
            $sumUtf16 += [int64]$storyRow.utf16_code_units
        }

        $witnesses += [ordered]@{
            source_sha256 = $before.sha256
            source_byte_len = [int64]$before.size
            story_count = $storyRows.Count
            story_utf16_sum = $sumUtf16
            stories = $storyRows
            source_unchanged = $true
        }
    }

    $receipt = [ordered]@{
        schema = "chaptera.quill-story-readonly-oracle.v1"
        publisher = [ordered]@{
            version = if ($publisher.version.state -eq "value") { [string]$publisher.version.value } else { $null }
            build = if ($publisher.build.state -eq "value") { [string]$publisher.build.value } else { $null }
        }
        witness_count = $witnesses.Count
        witnesses = @($witnesses | Sort-Object source_sha256)
        source_commit = $LalamuCommit
        evidence_boundary = "Exact four SHA-addressed #337 supersets; public pinned donor bytes are downloaded into runner temp and deleted after the run; Publisher Open is read-only only; receipt contains Story UTF-16 lengths and SHA-256 digests, never document text; no Save or SaveAs."
    }

    Write-PubJson -Value $receipt -Path $AnalysisPath
    @(
        "QUILL-STORY-READONLY-ORACLE-02 PASS",
        "witness_count=4",
        "publisher_version=$($receipt.publisher.version)",
        "publisher_build=$($receipt.publisher.build)",
        "source_unchanged=4/4",
        "document_text_retained=false"
    ) | Set-Content -LiteralPath $LogPath -Encoding utf8
}
finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
