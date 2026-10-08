param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "PUB-T-840-TABLE-AUTOFORMAT-PHASE1"
$PbFilePublication = 1
$PbTableAutoFormatCheckbookRegister = 0
$TagName = "PUB_ORACLE_ID"
$TagValue = "PUB_T840_TABLE_AUTOFORMAT_PHASE1"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/table-autoformat-phase1"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Release-Com($Value) {
    if ($null -ne $Value -and [System.Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Close-Document($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Saved = $true } catch {}
    try { $Document.Close() } catch {}
    Release-Com $Document
}

function Find-TaggedTableShape {
    param([Parameter(Mandatory = $true)]$Document)

    $matches = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    for ($tagIndex = 1; $tagIndex -le [int]$shape.Tags.Count; $tagIndex++) {
                        $tag = $null
                        try {
                            $tag = $shape.Tags.Item($tagIndex)
                            if ([string]$tag.Name -eq $TagName -and [string]$tag.Value -eq $TagValue) {
                                $matches += [pscustomobject]@{
                                    page_index = $pageIndex
                                    shape_index = $shapeIndex
                                }
                            }
                        }
                        finally {
                            Release-Com $tag
                        }
                    }
                }
                finally {
                    Release-Com $shape
                }
            }
        }
        finally {
            Release-Com $page
        }
    }

    if ($matches.Count -ne 1) {
        throw "Expected exactly one tagged TABLE shape; found $($matches.Count)."
    }
    return $matches[0]
}

function Add-HistogramValue {
    param(
        [Parameter(Mandatory = $true)][hashtable]$Histogram,
        [Parameter(Mandatory = $true)][string]$Key
    )
    if (-not $Histogram.ContainsKey($Key)) { $Histogram[$Key] = 0 }
    $Histogram[$Key] = [int]$Histogram[$Key] + 1
}

function Get-CellSignature {
    param([Parameter(Mandatory = $true)]$Cell)

    $fill = $null
    $fillColor = $null
    $textRange = $null
    $font = $null
    $paragraph = $null
    $top = $null
    $right = $null
    $bottom = $null
    $left = $null
    $topColor = $null
    $rightColor = $null
    $bottomColor = $null
    $leftColor = $null
    try {
        $fill = $Cell.Fill
        $fillColor = $fill.ForeColor
        $textRange = $Cell.TextRange
        $font = $textRange.Font
        $paragraph = $textRange.ParagraphFormat
        $top = $Cell.BorderTop
        $right = $Cell.BorderRight
        $bottom = $Cell.BorderBottom
        $left = $Cell.BorderLeft
        $topColor = $top.Color
        $rightColor = $right.Color
        $bottomColor = $bottom.Color
        $leftColor = $left.Color

        return [ordered]@{
            fill_visible = [long]$fill.Visible
            fill_type = [long]$fill.Type
            fill_rgb = [long]$fillColor.RGB
            font_name = [string]$font.Name
            font_size = [double]$font.Size
            font_bold = [long]$font.Bold
            paragraph_alignment = [long]$paragraph.Alignment
            border_top_weight = [double]$top.Weight
            border_top_rgb = [long]$topColor.RGB
            border_right_weight = [double]$right.Weight
            border_right_rgb = [long]$rightColor.RGB
            border_bottom_weight = [double]$bottom.Weight
            border_bottom_rgb = [long]$bottomColor.RGB
            border_left_weight = [double]$left.Weight
            border_left_rgb = [long]$leftColor.RGB
        }
    }
    finally {
        Release-Com $leftColor
        Release-Com $bottomColor
        Release-Com $rightColor
        Release-Com $topColor
        Release-Com $left
        Release-Com $bottom
        Release-Com $right
        Release-Com $top
        Release-Com $paragraph
        Release-Com $font
        Release-Com $textRange
        Release-Com $fillColor
        Release-Com $fill
    }
}

function Get-TableSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$Phase
    )

    $location = Find-TaggedTableShape -Document $Document
    $page = $null
    $shape = $null
    $table = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $table = $shape.Table

        $fillHistogram = @{}
        $textHistogram = @{}
        $borderHistogram = @{}
        $cellCount = 0

        for ($rowIndex = 1; $rowIndex -le [int]$table.Rows.Count; $rowIndex++) {
            $row = $null
            try {
                $row = $table.Rows.Item($rowIndex)
                for ($cellIndex = 1; $cellIndex -le [int]$row.Cells.Count; $cellIndex++) {
                    $cell = $null
                    try {
                        $cell = $row.Cells.Item($cellIndex)
                        $sig = Get-CellSignature -Cell $cell
                        $cellCount++
                        Add-HistogramValue -Histogram $fillHistogram -Key ("v={0};t={1};rgb={2}" -f $sig.fill_visible,$sig.fill_type,$sig.fill_rgb)
                        Add-HistogramValue -Histogram $textHistogram -Key ("font={0};size={1};bold={2};align={3}" -f $sig.font_name,$sig.font_size,$sig.font_bold,$sig.paragraph_alignment)
                        Add-HistogramValue -Histogram $borderHistogram -Key ("t={0}/{1};r={2}/{3};b={4}/{5};l={6}/{7}" -f $sig.border_top_weight,$sig.border_top_rgb,$sig.border_right_weight,$sig.border_right_rgb,$sig.border_bottom_weight,$sig.border_bottom_rgb,$sig.border_left_weight,$sig.border_left_rgb)
                    }
                    finally {
                        Release-Com $cell
                    }
                }
            }
            finally {
                Release-Com $row
            }
        }

        return [ordered]@{
            phase = $Phase
            page_index = [int]$location.page_index
            rows = [int]$table.Rows.Count
            columns = [int]$table.Columns.Count
            cell_count = $cellCount
            fill_histogram = $fillHistogram
            text_histogram = $textHistogram
            border_histogram = $borderHistogram
        }
    }
    finally {
        Release-Com $table
        Release-Com $shape
        Release-Com $page
    }
}

function New-BaselinePublication {
    param([Parameter(Mandatory = $true)][string]$Path)

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $table = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Documents.Add()
        $page = $doc.Pages.Item(1)
        $shape = $page.Shapes.AddTable(4, 4, 72, 144, 432, 144)
        $shape.Tags.Add($TagName, $TagValue) | Out-Null
        $table = $shape.Table

        for ($rowIndex = 1; $rowIndex -le 4; $rowIndex++) {
            $row = $null
            try {
                $row = $table.Rows.Item($rowIndex)
                for ($cellIndex = 1; $cellIndex -le 4; $cellIndex++) {
                    $cell = $null
                    $textRange = $null
                    try {
                        $cell = $row.Cells.Item($cellIndex)
                        $textRange = $cell.TextRange
                        $textRange.Text = "R$($rowIndex)C$($cellIndex)"
                    }
                    finally {
                        Release-Com $textRange
                        Release-Com $cell
                    }
                }
            }
            finally {
                Release-Com $row
            }
        }

        $doc.SaveAs($Path, $PbFilePublication, $false)
    }
    finally {
        Release-Com $table
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$BaselinePath,
        [Parameter(Mandatory = $true)][bool]$ApplyAutoFormat
    )

    $armDir = Join-Path $privateDir $Name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $BaselinePath -Destination $working -Force

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $table = $null
    $before = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $before = Get-TableSnapshot -Document $doc -Phase "before"

        if ($ApplyAutoFormat) {
            $location = Find-TaggedTableShape -Document $doc
            $page = $doc.Pages.Item([int]$location.page_index)
            $shape = $page.Shapes.Item([int]$location.shape_index)
            $table = $shape.Table
            $table.ApplyAutoFormat($PbTableAutoFormatCheckbookRegister, $true, $true, $true, $true)
        }

        $doc.SaveAs($output, $PbFilePublication, $false)
    }
    finally {
        Release-Com $table
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $app2 = $null
    $doc2 = $null
    $after = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        $after = Get-TableSnapshot -Document $doc2 -Phase "fresh_reopen"
    }
    finally {
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    return [ordered]@{
        arm = $Name
        apply_autoformat = $ApplyAutoFormat
        before = $before
        fresh_reopen = $after
        output_sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
        private_output = $true
    }
}

$baselinePath = Join-Path $privateDir "baseline.pub"
New-BaselinePublication -Path $baselinePath
$baselineHash = (Get-FileHash -LiteralPath $baselinePath -Algorithm SHA256).Hash.ToLowerInvariant()

$control = Invoke-Arm -Name "control" -BaselinePath $baselinePath -ApplyAutoFormat $false
$treatment = Invoke-Arm -Name "treatment" -BaselinePath $baselinePath -ApplyAutoFormat $true

$tableShapeStable = (
    [int]$control.fresh_reopen.rows -eq 4 -and
    [int]$control.fresh_reopen.columns -eq 4 -and
    [int]$control.fresh_reopen.cell_count -eq 16 -and
    [int]$treatment.fresh_reopen.rows -eq 4 -and
    [int]$treatment.fresh_reopen.columns -eq 4 -and
    [int]$treatment.fresh_reopen.cell_count -eq 16
)
$controlStable = (
    ($control.before.fill_histogram | ConvertTo-Json -Compress -Depth 8) -eq
        ($control.fresh_reopen.fill_histogram | ConvertTo-Json -Compress -Depth 8) -and
    ($control.before.text_histogram | ConvertTo-Json -Compress -Depth 8) -eq
        ($control.fresh_reopen.text_histogram | ConvertTo-Json -Compress -Depth 8) -and
    ($control.before.border_histogram | ConvertTo-Json -Compress -Depth 8) -eq
        ($control.fresh_reopen.border_histogram | ConvertTo-Json -Compress -Depth 8)
)
$effectiveDelta = (
    ($control.fresh_reopen.fill_histogram | ConvertTo-Json -Compress -Depth 8) -ne
        ($treatment.fresh_reopen.fill_histogram | ConvertTo-Json -Compress -Depth 8) -or
    ($control.fresh_reopen.text_histogram | ConvertTo-Json -Compress -Depth 8) -ne
        ($treatment.fresh_reopen.text_histogram | ConvertTo-Json -Compress -Depth 8) -or
    ($control.fresh_reopen.border_histogram | ConvertTo-Json -Compress -Depth 8) -ne
        ($treatment.fresh_reopen.border_histogram | ConvertTo-Json -Compress -Depth 8)
)

$verdict = "inconclusive"
if ($tableShapeStable -and $controlStable) {
    $verdict = if ($effectiveDelta) { "autoformat-delta-persists" } else { "autoformat-no-durable-delta" }
}

$result = [ordered]@{
    schema = "chaptera.publisher-table-autoformat-phase1.v1"
    experiment_id = $ExpectedExperiment
    publisher = [ordered]@{
        expected_version_prefix = "16.0.12527."
    }
    generated_fixture = [ordered]@{
        baseline_sha256 = $baselineHash
        rows = 4
        columns = 4
        cell_count = 16
    }
    control = [ordered]@{
        fresh_reopen = $control.fresh_reopen
        output_sha256 = $control.output_sha256
    }
    treatment = [ordered]@{
        autoformat = "CheckbookRegister"
        text_formatting = $true
        text_alignment = $true
        fill = $true
        borders = $true
        fresh_reopen = $treatment.fresh_reopen
        output_sha256 = $treatment.output_sha256
    }
    guards = [ordered]@{
        table_shape_stable = [bool]$tableShapeStable
        noop_effective_snapshot_stable = [bool]$controlStable
        effective_delta_after_fresh_reopen = [bool]$effectiveDelta
    }
    verdict = $verdict
    authority_boundary = "Phase 1 proves only whether Checkbook Register creates a durable effective formatting delta after fresh reopen. Durable-style vs one-shot materialization vs hybrid requires raw A/B carrier inspection and later category/scheme/growth arms."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "table-autoformat-phase1.json")
@(
    "experiment=$ExpectedExperiment",
    "baseline_sha256=$baselineHash",
    "table_shape_stable=$tableShapeStable",
    "noop_snapshot_stable=$controlStable",
    "effective_delta=$effectiveDelta",
    "verdict=$verdict"
) | Set-Content -LiteralPath (Join-Path $logDir "table-autoformat-phase1.txt") -Encoding ASCII

if ($verdict -eq "inconclusive") {
    throw "PUB-T-840 Phase 1 remained inconclusive; inspect private local arm artifacts."
}
