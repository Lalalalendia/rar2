param(
    [Parameter(Mandatory = $true)][string]$InputPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedSha256 = "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e"
$ExperimentId = "MATURE-029-NATIVE-PAGE-SPREAD-ORACLE-01"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir | Out-Null

$resultPath = Join-Path $analysisDir "mature-029-native-page-spread-oracle-01.json"
$logPath = Join-Path $logDir "mature-029-native-page-spread-oracle-01.txt"
$environmentPath = Join-Path $OutputRoot "environment.json"

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Get-Safe {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Getter,
        [Parameter(Mandatory = $true)][string]$Member
    )
    try {
        return [ordered]@{
            state = "value"
            member = $Member
            value = & $Getter
        }
    }
    catch {
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

$resolvedInput = (Resolve-Path -LiteralPath $InputPath).Path
$before = Get-PubFileRecord -Path $resolvedInput
if ([string]$before.sha256 -ne $ExpectedSha256) {
    throw "Exact 029 SHA mismatch: expected $ExpectedSha256 got $($before.sha256)"
}

$environment = Get-PubEnvironmentManifest -SnapshotId $ExperimentId -RequirePublisher
Write-PubJson -Value $environment -Path $environmentPath

$app = $null
$doc = $null
$pageSetup = $null
$rows = @()
$docSummary = $null

try {
    $app = New-PubPublisherApplication
    # Application.Open(FileName, ReadOnly, AddToRecentFiles)
    $doc = $app.Open($resolvedInput, $true, $false)

    $pageSetup = $doc.PageSetup
    $pageSetupSummary = [ordered]@{
        page_width = Get-Safe { [double]$pageSetup.PageWidth } "PageSetup.PageWidth"
        page_height = Get-Safe { [double]$pageSetup.PageHeight } "PageSetup.PageHeight"
        publication_layout = Get-Safe { [int]$pageSetup.PublicationLayout } "PageSetup.PublicationLayout"
        horizontal_gap = Get-Safe { [double]$pageSetup.HorizontalGap } "PageSetup.HorizontalGap"
        vertical_gap = Get-Safe { [double]$pageSetup.VerticalGap } "PageSetup.VerticalGap"
        left_margin = Get-Safe { [double]$pageSetup.LeftMargin } "PageSetup.LeftMargin"
        top_margin = Get-Safe { [double]$pageSetup.TopMargin } "PageSetup.TopMargin"
        page_size_name = $null
    }

    $pageSize = $null
    try {
        $pageSize = $pageSetup.PageSize
        $pageSetupSummary.page_size_name = Get-Safe { [string]$pageSize.Name } "PageSetup.PageSize.Name"
    }
    catch {
        $pageSetupSummary.page_size_name = Get-Safe { throw $_.Exception } "PageSetup.PageSize.Name"
    }
    finally {
        Release-Com $pageSize
    }

    $docSummary = [ordered]@{
        pages_count = Get-Safe { [int]$doc.Pages.Count } "Document.Pages.Count"
        print_style = Get-Safe { [int]$doc.PrintStyle } "Document.PrintStyle"
        view_two_page_spread = Get-Safe { [bool]$doc.ViewTwoPageSpread } "Document.ViewTwoPageSpread"
        page_setup = $pageSetupSummary
    }

    $pagesCount = [int]$doc.Pages.Count
    for ($index = 1; $index -le $pagesCount; $index++) {
        $page = $null
        $spread = $null
        $spreadPages = $null
        $master = $null
        try {
            $page = $doc.Pages.Item($index)

            $masterPageId = $null
            try {
                $master = $page.Master
                if ($null -ne $master) {
                    $masterPageId = Get-Safe { [long]$master.PageID } "Page.Master.PageID"
                }
            }
            catch {
                $masterPageId = Get-Safe { throw $_.Exception } "Page.Master.PageID"
            }
            finally {
                Release-Com $master
                $master = $null
            }

            $spreadSummary = [ordered]@{
                access = "unavailable"
                page_count = $null
                width = $null
                height = $null
                left = $null
                top = $null
                page_ids = @()
                access_error = $null
            }

            try {
                $spread = $page.ReaderSpread
                if ($null -ne $spread) {
                    $spreadSummary.access = "ok"
                    $spreadSummary.page_count = Get-Safe { [int]$spread.PageCount } "ReaderSpread.PageCount"
                    $spreadSummary.width = Get-Safe { [double]$spread.Width } "ReaderSpread.Width"
                    $spreadSummary.height = Get-Safe { [double]$spread.Height } "ReaderSpread.Height"
                    $spreadSummary.left = Get-Safe { [double]$spread.Left } "ReaderSpread.Left"
                    $spreadSummary.top = Get-Safe { [double]$spread.Top } "ReaderSpread.Top"

                    try {
                        $spreadPages = $spread.Pages
                        $spreadPageCount = [int]$spreadPages.Count
                        $ids = @()
                        for ($spreadIndex = 1; $spreadIndex -le $spreadPageCount; $spreadIndex++) {
                            $spreadPage = $null
                            try {
                                $spreadPage = $spreadPages.Item($spreadIndex)
                                $ids += [ordered]@{
                                    spread_ordinal = $spreadIndex
                                    page_id = [long]$spreadPage.PageID
                                    page_index = [int]$spreadPage.PageIndex
                                }
                            }
                            finally {
                                Release-Com $spreadPage
                            }
                        }
                        $spreadSummary.page_ids = @($ids)
                    }
                    catch {
                        $spreadSummary.access_error = "ReaderSpread.Pages: $($_.Exception.Message)"
                    }
                }
            }
            catch {
                $spreadSummary.access_error = $_.Exception.Message
            }
            finally {
                Release-Com $spreadPages
                Release-Com $spread
            }

            $rows += [ordered]@{
                collection_index = $index
                page_id = Get-Safe { [long]$page.PageID } "Page.PageID"
                page_index = Get-Safe { [int]$page.PageIndex } "Page.PageIndex"
                page_number = Get-Safe { [string]$page.PageNumber } "Page.PageNumber"
                name = Get-Safe { [string]$page.Name } "Page.Name"
                width = Get-Safe { [double]$page.Width } "Page.Width"
                height = Get-Safe { [double]$page.Height } "Page.Height"
                page_type = Get-Safe { [int]$page.PageType } "Page.PageType"
                is_leading = Get-Safe { [bool]$page.IsLeading } "Page.IsLeading"
                is_trailing = Get-Safe { [bool]$page.IsTrailing } "Page.IsTrailing"
                is_two_page_master = Get-Safe { [bool]$page.IsTwoPageMaster } "Page.IsTwoPageMaster"
                ignore_master = Get-Safe { [bool]$page.IgnoreMaster } "Page.IgnoreMaster"
                x_offset_within_reader_spread = Get-Safe { [double]$page.XOffsetWithinReaderSpread } "Page.XOffsetWithinReaderSpread"
                y_offset_within_reader_spread = Get-Safe { [double]$page.YOffsetWithinReaderSpread } "Page.YOffsetWithinReaderSpread"
                master_page_id = $masterPageId
                reader_spread = $spreadSummary
            }
        }
        finally {
            Release-Com $page
        }
    }
}
finally {
    Release-Com $pageSetup
    if ($null -ne $doc) {
        try { $doc.Close() } catch {}
        Release-Com $doc
    }
    Close-PubPublisherApplication $app
}

$after = Get-PubFileRecord -Path $resolvedInput
if ([string]$after.sha256 -ne [string]$before.sha256 -or [int64]$after.size -ne [int64]$before.size) {
    throw "Read-only native probe changed the exact source file."
}

$publisher = $environment.publisher
$result = [ordered]@{
    schema = "chaptera.mature-029-native-page-spread-oracle.v1"
    experiment_id = $ExperimentId
    source = [ordered]@{
        sha256 = [string]$before.sha256
        size = [int64]$before.size
        unchanged_after_probe = $true
    }
    publisher = [ordered]@{
        version = $publisher.version
        build = $publisher.build
        name = $publisher.name
    }
    document = $docSummary
    pages = @($rows)
    claims = [ordered]@{
        opened_read_only = $true
        add_to_recent_files = $false
        document_mutation_invoked = $false
        save_invoked = $false
        print_invoked = $false
        export_invoked = $false
        macro_execution_invoked = $false
        external_link_update_invoked = $false
    }
}

Write-PubJson -Value $result -Path $resultPath

$lines = @(
    "experiment=$ExperimentId",
    "source_sha256=$($before.sha256)",
    "source_unchanged=true",
    "pages_count=$($docSummary.pages_count.value)",
    "page_width=$($docSummary.page_setup.page_width.value)",
    "page_height=$($docSummary.page_setup.page_height.value)",
    "publication_layout=$($docSummary.page_setup.publication_layout.value)",
    "print_style=$($docSummary.print_style.value)",
    "view_two_page_spread=$($docSummary.view_two_page_spread.value)",
    "mutation_invoked=false",
    "save_invoked=false",
    "print_invoked=false",
    "export_invoked=false"
)
foreach ($row in $rows) {
    $spreadCount = if ($row.reader_spread.page_count.state -eq "value") { $row.reader_spread.page_count.value } else { "error" }
    $lines += "page[$($row.collection_index)]:id=$($row.page_id.value),index=$($row.page_index.value),number=$($row.page_number.value),width=$($row.width.value),height=$($row.height.value),leading=$($row.is_leading.value),trailing=$($row.is_trailing.value),xoff=$($row.x_offset_within_reader_spread.value),yoff=$($row.y_offset_within_reader_spread.value),spread_count=$spreadCount"
}
$lines | Set-Content -LiteralPath $logPath -Encoding ASCII
