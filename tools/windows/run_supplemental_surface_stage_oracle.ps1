param(
    [Parameter(Mandatory = $true)][string]$SourceRoot,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

$Targets = @(
    [pscustomobject]@{ id = "083"; sha256 = "5eb5055bc75918ca8dbc7093fa267a3365f25129fd2c3dc88c7be3540f44e4cd"; bytes = [int64]455680; reference_pdf_pages = 2 }
    [pscustomobject]@{ id = "086"; sha256 = "248596cea801913454aa40073d5ce113a931b11a7283c9efc4533caa86a46125"; bytes = [int64]762368; reference_pdf_pages = 2 }
    [pscustomobject]@{ id = "095"; sha256 = "1e7f38b3ce1d0d956815992b15d361c405fc4bbdced5cdabb3c4581327cc183e"; bytes = [int64]396800; reference_pdf_pages = 1 }
)

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Get-Safe {
    param([Parameter(Mandatory = $true)][scriptblock]$Getter,[Parameter(Mandatory = $true)][string]$Member)
    try {
        return [ordered]@{ state = "value"; member = $Member; value = & $Getter }
    } catch {
        $hr = $null
        try { $hr = Format-PubHResult ([int]$_.Exception.HResult) } catch {}
        return [ordered]@{ state = "error"; member = $Member; hresult = $hr; message = $_.Exception.Message }
    }
}

$resolvedSourceRoot = (Resolve-Path -LiteralPath $SourceRoot).Path
if (-not [System.IO.Path]::IsPathRooted($OutputRoot)) { $OutputRoot = Join-Path $RepoRoot $OutputRoot }
if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force).Count -ne 0) { throw "OutputRoot must be empty: $OutputRoot" }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

$publisher = Get-PubPublisherIdentity
if (-not $publisher.available) { throw "Publisher COM is unavailable." }
if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) { throw "Publisher Version mismatch." }
if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) { throw "Publisher Build mismatch." }
if ($publisher.path.state -ne "value") { throw "Publisher executable directory is unavailable." }

$publisherExe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
if (-not (Test-Path -LiteralPath $publisherExe -PathType Leaf)) { throw "Publisher executable missing." }
$fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($publisherExe).FileVersion
if ([string]$fileVersion -ne $ExpectedPublisherFileVersion) { throw "MSPUB.EXE file version mismatch: $fileVersion" }
$exeSha = (Get-FileHash -LiteralPath $publisherExe -Algorithm SHA256).Hash.ToLowerInvariant()
if ($exeSha -ne $ExpectedPublisherExeSha256) { throw "MSPUB.EXE SHA mismatch: $exeSha" }

$results = @()
foreach ($target in $Targets) {
    $resolved = Join-Path $resolvedSourceRoot ($target.sha256 + ".pub")
    if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) { throw "Exact supplemental source missing for $($target.id): $resolved" }
    $source = Get-Item -LiteralPath $resolved
    if ([int64]$source.Length -ne [int64]$target.bytes) { throw "Source size mismatch for $($target.id)" }
    $beforeSha = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($beforeSha -ne $target.sha256) { throw "Source SHA mismatch for $($target.id): $beforeSha" }

    $app = $null
    $doc = $null
    $pageSetup = $null
    $pages = @()
    $document = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($resolved, $true, $false)
        $pageSetup = $doc.PageSetup
        $document = [ordered]@{
            pages_count = Get-Safe { [int]$doc.Pages.Count } "Document.Pages.Count"
            print_style = Get-Safe { [int]$doc.PrintStyle } "Document.PrintStyle"
            view_two_page_spread = Get-Safe { [bool]$doc.ViewTwoPageSpread } "Document.ViewTwoPageSpread"
            page_setup = [ordered]@{
                page_width = Get-Safe { [double]$pageSetup.PageWidth } "PageSetup.PageWidth"
                page_height = Get-Safe { [double]$pageSetup.PageHeight } "PageSetup.PageHeight"
                publication_layout = Get-Safe { [int]$pageSetup.PublicationLayout } "PageSetup.PublicationLayout"
            }
        }
        $count = [int]$doc.Pages.Count
        for ($index = 1; $index -le $count; $index++) {
            $page = $null
            $spread = $null
            try {
                $page = $doc.Pages.Item($index)
                $spreadSummary = [ordered]@{ page_count = $null; width = $null; height = $null; left = $null; top = $null }
                try {
                    $spread = $page.ReaderSpread
                    if ($null -ne $spread) {
                        $spreadSummary.page_count = Get-Safe { [int]$spread.PageCount } "ReaderSpread.PageCount"
                        $spreadSummary.width = Get-Safe { [double]$spread.Width } "ReaderSpread.Width"
                        $spreadSummary.height = Get-Safe { [double]$spread.Height } "ReaderSpread.Height"
                        $spreadSummary.left = Get-Safe { [double]$spread.Left } "ReaderSpread.Left"
                        $spreadSummary.top = Get-Safe { [double]$spread.Top } "ReaderSpread.Top"
                    }
                } finally { Release-Com $spread }

                $pages += [ordered]@{
                    collection_index = $index
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
            } finally { Release-Com $page }
        }
    } finally {
        Release-Com $pageSetup
        if ($null -ne $doc) { try { $doc.Close() } catch {}; Release-Com $doc }
        Close-PubPublisherApplication $app
    }

    $after = Get-Item -LiteralPath $resolved
    $afterSha = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($afterSha -ne $beforeSha -or [int64]$after.Length -ne [int64]$source.Length) { throw "Read-only probe changed source $($target.id)." }

    $results += [ordered]@{
        id = $target.id
        source_sha256 = $beforeSha
        source_bytes = [int64]$source.Length
        source_unchanged_after_probe = $true
        publisher_pdf_page_count = [int]$target.reference_pdf_pages
        document = $document
        pages = @($pages)
    }
}

$receipt = [ordered]@{
    schema = "chaptera.supplemental-surface-stage-oracle.v1"
    experiment_id = "SUPPLEMENTAL-SURFACE-STAGE-083-086-095-01"
    publisher = [ordered]@{
        version = $publisher.version
        build = $publisher.build
        name = $publisher.name
        executable_file_version = $ExpectedPublisherFileVersion
        executable_sha256 = $exeSha
    }
    targets = @($results)
    claims = [ordered]@{
        opened_read_only = $true
        add_to_recent_files = $false
        document_mutation_invoked = $false
        save_invoked = $false
        print_invoked = $false
        export_invoked = $false
        macro_execution_invoked = $false
        pdf_page_count_is_observation_not_logical_page_authority = $true
    }
}

$receiptPath = Join-Path $OutputRoot "supplemental-surface-stage-oracle-083-086-095.json"
Write-PubJson -Value $receipt -Path $receiptPath
Write-Host "Supplemental surface-stage oracle complete."
Get-Content -LiteralPath $receiptPath -Raw
