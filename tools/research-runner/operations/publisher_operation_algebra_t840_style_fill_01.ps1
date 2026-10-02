param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "PUB-OPALG-T840-STYLE-FILL-01"
$PbFilePublication = 1
$PbTableAutoFormatCheckbookRegister = 0
$PbFixedFormatTypePDF = 2
$PbIntentStandard = 2
$DirectFillRgb = 12200429
$TagName = "PUB_ORACLE_ID"
$TagValue = "PUB_OPALG_T840_STYLE_FILL_01"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/operation-algebra-t840-style-fill"
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

        $fillScheme = $null
        try { $fillScheme = [long]$fillColor.SchemeColor } catch {}

        return [ordered]@{
            fill_visible = [long]$fill.Visible
            fill_type = [long]$fill.Type
            fill_rgb = [long]$fillColor.RGB
            fill_scheme = $fillScheme
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
    param([Parameter(Mandatory = $true)]$Document)

    $location = Find-TaggedTableShape -Document $Document
    $page = $null
    $shape = $null
    $table = $null
    $r2 = $null
    $r2c2 = $null
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
                        Add-HistogramValue -Histogram $fillHistogram -Key ("v={0};t={1};rgb={2};scheme={3}" -f $sig.fill_visible,$sig.fill_type,$sig.fill_rgb,$sig.fill_scheme)
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

        $r2 = $table.Rows.Item(2)
        $r2c2 = $r2.Cells.Item(2)

        return [ordered]@{
            rows = [int]$table.Rows.Count
            columns = [int]$table.Columns.Count
            cell_count = $cellCount
            r2c2 = Get-CellSignature -Cell $r2c2
            fill_histogram = $fillHistogram
            text_histogram = $textHistogram
            border_histogram = $borderHistogram
        }
    }
    finally {
        Release-Com $r2c2
        Release-Com $r2
        Release-Com $table
        Release-Com $shape
        Release-Com $page
    }
}

function Apply-TableStyle {
    param([Parameter(Mandatory = $true)]$Document)

    $location = Find-TaggedTableShape -Document $Document
    $page = $null
    $shape = $null
    $table = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $table = $shape.Table
        $table.ApplyAutoFormat($PbTableAutoFormatCheckbookRegister, $true, $true, $true, $true)
    }
    finally {
        Release-Com $table
        Release-Com $shape
        Release-Com $page
    }
}

function Apply-DirectFill {
    param([Parameter(Mandatory = $true)]$Document)

    $location = Find-TaggedTableShape -Document $Document
    $page = $null
    $shape = $null
    $table = $null
    $row = $null
    $cell = $null
    $fill = $null
    $color = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $table = $shape.Table
        $row = $table.Rows.Item(2)
        $cell = $row.Cells.Item(2)
        $fill = $cell.Fill
        try { $fill.Visible = -1 } catch {}
        try { $fill.Solid() } catch {}
        $color = $fill.ForeColor
        $color.RGB = $DirectFillRgb
    }
    finally {
        Release-Com $color
        Release-Com $fill
        Release-Com $cell
        Release-Com $row
        Release-Com $table
        Release-Com $shape
        Release-Com $page
    }
}

function Invoke-Operation {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$Operation
    )

    switch ($Operation) {
        "ApplyTableStyle_CheckbookRegister" { Apply-TableStyle -Document $Document; break }
        "DirectCellFillOverride_R2C2" { Apply-DirectFill -Document $Document; break }
        default { throw "Unsupported operation: $Operation" }
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
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Operations
    )

    $armDir = Join-Path $privateDir $Name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    $output = Join-Path $armDir "output.pub"
    $pdf = Join-Path $armDir "output.pdf"
    $semanticPath = Join-Path $armDir "semantic.json"
    $fingerprintPath = Join-Path $armDir "fingerprint.json"
    Copy-Item -LiteralPath $BaselinePath -Destination $working -Force

    $app = $null
    $doc = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        foreach ($operation in $Operations) {
            Invoke-Operation -Document $doc -Operation $operation
        }
        $doc.SaveAs($output, $PbFilePublication, $false)
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $app2 = $null
    $doc2 = $null
    $semantic = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        $semantic = Get-TableSnapshot -Document $doc2
        $semantic | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $semanticPath -Encoding UTF8
        $doc2.ExportAsFixedFormat($PbFixedFormatTypePDF, $pdf, $PbIntentStandard)
    }
    finally {
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $fingerprintTool = Join-Path $repoRoot "tools/pub_operation_algebra_fingerprint.py"
    & python $fingerprintTool --pub $output --pdf $pdf --semantic $semanticPath --out $fingerprintPath
    if ($LASTEXITCODE -ne 0) {
        throw "Fingerprint helper failed for arm $Name with exit code $LASTEXITCODE"
    }
    $fingerprint = Get-Content -LiteralPath $fingerprintPath -Raw | ConvertFrom-Json

    return [ordered]@{
        status = "ok"
        operations = @($Operations)
        semantic_snapshot = $semantic
        semantic_fingerprint = $fingerprint.semantic_fingerprint
        persistence_fingerprint = $fingerprint.persistence_fingerprint
        render_fingerprint = $fingerprint.render_fingerprint
        artifacts = $fingerprint.artifacts
    }
}

$baselinePath = Join-Path $privateDir "baseline.pub"
New-BaselinePublication -Path $baselinePath
$baselineHash = (Get-FileHash -LiteralPath $baselinePath -Algorithm SHA256).Hash.ToLowerInvariant()

$opA = "ApplyTableStyle_CheckbookRegister"
$opB = "DirectCellFillOverride_R2C2"

$AB = Invoke-Arm -Name "AB" -BaselinePath $baselinePath -Operations @($opA, $opB)
$BA = Invoke-Arm -Name "BA" -BaselinePath $baselinePath -Operations @($opB, $opA)
$AOnly = Invoke-Arm -Name "A_only" -BaselinePath $baselinePath -Operations @($opA)
$BOnly = Invoke-Arm -Name "B_only" -BaselinePath $baselinePath -Operations @($opB)
$Control = Invoke-Arm -Name "control" -BaselinePath $baselinePath -Operations @()

$semanticEqual = ([string]$AB.semantic_fingerprint.sha256 -eq [string]$BA.semantic_fingerprint.sha256)
$persistenceEqual = ([string]$AB.persistence_fingerprint.sha256 -eq [string]$BA.persistence_fingerprint.sha256)
$renderEqual = ([string]$AB.render_fingerprint.sha256 -eq [string]$BA.render_fingerprint.sha256)

$classification = "non_commutative_semantic_or_hidden_precedence"
if ($semanticEqual -and $persistenceEqual -and $renderEqual) {
    $classification = "commute_exact_after_reopen"
}
elseif ($semanticEqual -and -not $persistenceEqual) {
    $classification = "semantic_commute_persistence_diverges"
}
elseif ($semanticEqual -and $persistenceEqual -and -not $renderEqual) {
    $classification = "renderer_or_derived-state_divergence"
}

$result = [ordered]@{
    schema = "chaptera.pub.operation-algebra-native-result.v1"
    experiment_id = $ExpectedExperiment
    pair_id = "T840-STYLE-FILL-R2C2"
    publisher = [ordered]@{
        expected_version_prefix = "16.0.12527."
    }
    fixture = [ordered]@{
        kind = "generated"
        baseline_sha256 = $baselineHash
        rows = 4
        columns = 4
        cell_count = 16
    }
    a = $opA
    b = $opB
    direct_fill_rgb = $DirectFillRgb
    AB = $AB
    BA = $BA
    A_only = $AOnly
    B_only = $BOnly
    control = $Control
    comparison = [ordered]@{
        semantic_equal = [bool]$semanticEqual
        persistence_equal = [bool]$persistenceEqual
        render_equal = [bool]$renderEqual
        classification = $classification
    }
    authority_boundary = "This experiment proves only the bounded Publisher2019 ordering relation between CheckbookRegister ApplyAutoFormat and one R2C2 direct fill override. It does not generalize to other presets, cells, formatting categories, Publisher versions, or a retained TableStyle identity."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "operation-algebra-t840-style-fill-01.json")
@(
    "experiment=$ExpectedExperiment",
    "baseline_sha256=$baselineHash",
    "direct_fill_rgb=$DirectFillRgb",
    "semantic_equal=$semanticEqual",
    "persistence_equal=$persistenceEqual",
    "render_equal=$renderEqual",
    "classification=$classification"
) | Set-Content -LiteralPath (Join-Path $logDir "operation-algebra-t840-style-fill-01.txt") -Encoding ASCII
