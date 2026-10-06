param(
    [Parameter(Mandatory = $true)][string]$InputPath,
    [Parameter(Mandatory = $true)][string]$ExpectedSha256,
    [Parameter(Mandatory = $true)][int64]$ExpectedBytes,
    [Parameter(Mandatory = $true)][string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Get-Safe {
    param([scriptblock]$Getter,[string]$Member)
    try {
        return [ordered]@{ state = "value"; member = $Member; value = & $Getter }
    } catch {
        $hr = $null
        try { $hr = Format-PubHResult ([int]$_.Exception.HResult) } catch {}
        return [ordered]@{
            state = "error"
            member = $Member
            hresult = $hr
            message = $_.Exception.Message
        }
    }
}

$resolved = (Resolve-Path -LiteralPath $InputPath).Path
$file = Get-Item -LiteralPath $resolved
if ([int64]$file.Length -ne $ExpectedBytes) {
    throw "source byte-length mismatch: expected $ExpectedBytes got $($file.Length)"
}
$beforeSha = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
if ($beforeSha -ne $ExpectedSha256.ToLowerInvariant()) {
    throw "source SHA mismatch: expected $ExpectedSha256 got $beforeSha"
}

$publisher = Get-PubPublisherIdentity
if (-not $publisher.available) { throw "Publisher COM unavailable" }
if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne "16.0") {
    throw "Publisher Version mismatch"
}
if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne "12527") {
    throw "Publisher Build mismatch"
}

$app = $null
$doc = $null
$pageSetup = $null
$rows = @()

try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($resolved, $true, $false)
    $pageSetup = $doc.PageSetup

    $document = [ordered]@{
        pages_count = Get-Safe { [int]$doc.Pages.Count } "Document.Pages.Count"
        print_style = Get-Safe { [int]$doc.PrintStyle } "Document.PrintStyle"
        view_two_page_spread = Get-Safe { [bool]$doc.ViewTwoPageSpread } "Document.ViewTwoPageSpread"
        page_width = Get-Safe { [double]$pageSetup.PageWidth } "PageSetup.PageWidth"
        page_height = Get-Safe { [double]$pageSetup.PageHeight } "PageSetup.PageHeight"
        publication_layout = Get-Safe { [int]$pageSetup.PublicationLayout } "PageSetup.PublicationLayout"
    }

    $count = [int]$doc.Pages.Count
    for ($i = 1; $i -le $count; $i++) {
        $page = $null
        $spread = $null
        try {
            $page = $doc.Pages.Item($i)
            $spreadSummary = [ordered]@{ access = "unavailable"; page_count = $null; width = $null; height = $null; left = $null; top = $null; access_error = $null }
            try {
                $spread = $page.ReaderSpread
                if ($null -ne $spread) {
                    $spreadSummary.access = "ok"
                    $spreadSummary.page_count = Get-Safe { [int]$spread.PageCount } "ReaderSpread.PageCount"
                    $spreadSummary.width = Get-Safe { [double]$spread.Width } "ReaderSpread.Width"
                    $spreadSummary.height = Get-Safe { [double]$spread.Height } "ReaderSpread.Height"
                    $spreadSummary.left = Get-Safe { [double]$spread.Left } "ReaderSpread.Left"
                    $spreadSummary.top = Get-Safe { [double]$spread.Top } "ReaderSpread.Top"
                }
            } catch {
                $spreadSummary.access_error = $_.Exception.Message
            } finally {
                Release-Com $spread
            }

            $rows += [ordered]@{
                collection_index = $i
                page_id = Get-Safe { [long]$page.PageID } "Page.PageID"
                page_index = Get-Safe { [int]$page.PageIndex } "Page.PageIndex"
                page_number = Get-Safe { [string]$page.PageNumber } "Page.PageNumber"
                width = Get-Safe { [double]$page.Width } "Page.Width"
                height = Get-Safe { [double]$page.Height } "Page.Height"
                page_type = Get-Safe { [int]$page.PageType } "Page.PageType"
                is_leading = Get-Safe { [bool]$page.IsLeading } "Page.IsLeading"
                is_trailing = Get-Safe { [bool]$page.IsTrailing } "Page.IsTrailing"
                x_offset_within_reader_spread = Get-Safe { [double]$page.XOffsetWithinReaderSpread } "Page.XOffsetWithinReaderSpread"
                y_offset_within_reader_spread = Get-Safe { [double]$page.YOffsetWithinReaderSpread } "Page.YOffsetWithinReaderSpread"
                reader_spread = $spreadSummary
            }
        } finally {
            Release-Com $page
        }
    }
} finally {
    Release-Com $pageSetup
    if ($null -ne $doc) {
        try { $doc.Close() } catch {}
        Release-Com $doc
    }
    Close-PubPublisherApplication $app
}

$afterSha = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
$afterBytes = (Get-Item -LiteralPath $resolved).Length
if ($afterSha -ne $beforeSha -or [int64]$afterBytes -ne $ExpectedBytes) {
    throw "read-only surface probe changed source bytes"
}

$receipt = [ordered]@{
    schema = "chaptera.publisher-supplemental-native-surface.v1"
    source = [ordered]@{
        sha256 = $beforeSha
        bytes = $ExpectedBytes
        unchanged_after_probe = $true
    }
    publisher = [ordered]@{
        version = $publisher.version
        build = $publisher.build
        name = $publisher.name
    }
    document = $document
    pages = @($rows)
    claims = [ordered]@{
        opened_read_only = $true
        add_to_recent_files = $false
        save_invoked = $false
        print_invoked = $false
        export_invoked = $false
        macro_execution_invoked = $false
    }
}

$parent = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null
Write-PubJson -Value $receipt -Path $OutputPath
Get-Content -LiteralPath $OutputPath -Raw
