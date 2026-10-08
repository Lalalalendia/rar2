param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "PARAGRAPH-LINE-SPACING-0P75-NATIVE-01"
$MsoTextOrientationHorizontal = 1
$PbLineSpacingSingle = 0

Import-Module (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "windows\pub-runtime\PubRuntime.psm1") -Force

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) { throw "Unexpected experiment id: $($packet.id)" }
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) { throw "PUB_RESEARCH_FIXTURE missing" }

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private\paragraph-line-spacing-0p75-native-01"
$seedPub = Join-Path $privateDir "seed.pub"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Release-Com($value) {
    if ($null -ne $value -and [Runtime.InteropServices.Marshal]::IsComObject($value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($value) } catch {}
    }
}

function Close-Document($doc) {
    if ($null -eq $doc) { return }
    try { $doc.Saved = $true } catch {}
    try { $doc.Close() } catch {}
    Release-Com $doc
}

function Get-ParagraphSnapshot($paragraph, [string]$phase) {
    return [ordered]@{
        phase = $phase
        line_spacing = Get-PubSafeValue { [double]$paragraph.LineSpacing } "ParagraphFormat.LineSpacing"
        line_spacing_rule = Get-PubSafeValue { [int]$paragraph.LineSpacingRule } "ParagraphFormat.LineSpacingRule"
        left_indent = Get-PubSafeValue { [double]$paragraph.LeftIndent } "ParagraphFormat.LeftIndent"
        right_indent = Get-PubSafeValue { [double]$paragraph.RightIndent } "ParagraphFormat.RightIndent"
        first_line_indent = Get-PubSafeValue { [double]$paragraph.FirstLineIndent } "ParagraphFormat.FirstLineIndent"
        space_before = Get-PubSafeValue { [double]$paragraph.SpaceBefore } "ParagraphFormat.SpaceBefore"
        space_after = Get-PubSafeValue { [double]$paragraph.SpaceAfter } "ParagraphFormat.SpaceAfter"
    }
}

function Get-LineGeometry($textRange, [string]$phase) {
    $rows = @()
    $previousStart = $null
    $previousEnd = $null
    for ($index = 1; $index -le 64; $index++) {
        $line = $null
        try {
            $line = $textRange.Lines($index, 1)
            $start = [int]$line.Start
            $end = [int]$line.End
            if ($null -ne $previousStart -and $start -eq $previousStart -and $end -eq $previousEnd) {
                break
            }
            $rows += [ordered]@{
                index = $index
                start = $start
                end = $end
                bound_top = Get-PubSafeValue { [double]$line.BoundTop } "TextRange.BoundTop"
                bound_height = Get-PubSafeValue { [double]$line.BoundHeight } "TextRange.BoundHeight"
            }
            $previousStart = $start
            $previousEnd = $end
            if ($end -ge [int]$textRange.End) { break }
        }
        finally {
            Release-Com $line
        }
    }
    if ($rows.Count -eq 0) { throw "No line geometry returned for paragraph range" }
    return [ordered]@{
        phase = $phase
        line_count = $rows.Count
        lines = $rows
    }
}

function New-SeedFixture() {
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $seedPub -Force
    $app = $null; $doc = $null; $shape = $null; $range = $null; $paragraphRange = $null; $paragraph = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($seedPub, $false, $false)
        if ([int]$doc.Pages.Item(1).Shapes.Count -ne 0) { throw "Expected blank seed fixture" }
        $shape = $doc.Pages.Item(1).Shapes.AddTextbox($MsoTextOrientationHorizontal, 72, 72, 360, 180)
        $range = $shape.TextFrame.TextRange
        $cr = [char]13
        $range.Text = "Chaptera spacing alpha with enough words to wrap naturally." + $cr +
            "Chaptera spacing beta with enough words to wrap naturally and expose vertical progression." + $cr +
            "Chaptera spacing gamma with enough words to wrap naturally."
        $range.Font.Name = "Arial"
        $range.Font.Size = 12
        if ([int]$range.ParagraphsCount -lt 3) { throw "Expected three paragraphs" }
        $paragraphRange = $range.Paragraphs(2)
        $paragraph = $paragraphRange.ParagraphFormat
        $before = Get-ParagraphSnapshot $paragraph "seed_before_save"
        $seedGeometry = Get-LineGeometry $paragraphRange "seed_before_save"
        $doc.Save()
    }
    finally {
        Release-Com $paragraph; Release-Com $paragraphRange; Release-Com $range; Release-Com $shape
        Close-Document $doc; Close-PubPublisherApplication $app
    }
    $file = Get-Item -LiteralPath $seedPub
    return [ordered]@{
        sha256 = (Get-FileHash -LiteralPath $seedPub -Algorithm SHA256).Hash.ToLowerInvariant()
        size = [int64]$file.Length
        paragraph_before_save = $before
        line_geometry_before_save = $seedGeometry
    }
}

function Invoke-Arm([string]$name, [string]$kind) {
    $armDir = Join-Path $privateDir $name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $seedPub -Destination $output -Force

    $app = $null; $doc = $null; $shape = $null; $range = $null; $paragraphRange = $null; $paragraph = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($output, $false, $false)
        $shape = $doc.Pages.Item(1).Shapes.Item(1)
        $range = $shape.TextFrame.TextRange
        $paragraphRange = $range.Paragraphs(2)
        $paragraph = $paragraphRange.ParagraphFormat
        $before = Get-ParagraphSnapshot $paragraph "before_mutation"
        $geometryBefore = Get-LineGeometry $paragraphRange "before_mutation"
        switch ($kind) {
            "control" { }
            "single" { $paragraph.SetLineSpacing($PbLineSpacingSingle, 12) }
            "direct-0p75" { $paragraph.LineSpacing = 0.75 }
            default { throw "Unknown arm kind: $kind" }
        }
        $after = Get-ParagraphSnapshot $paragraph "after_mutation"
        $geometryAfter = Get-LineGeometry $paragraphRange "after_mutation"
        $doc.Save()
    }
    finally {
        Release-Com $paragraph; Release-Com $paragraphRange; Release-Com $range; Release-Com $shape
        Close-Document $doc; Close-PubPublisherApplication $app
    }

    $app2 = $null; $doc2 = $null; $shape2 = $null; $range2 = $null; $paragraphRange2 = $null; $paragraph2 = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        $shape2 = $doc2.Pages.Item(1).Shapes.Item(1)
        $range2 = $shape2.TextFrame.TextRange
        $paragraphRange2 = $range2.Paragraphs(2)
        $paragraph2 = $paragraphRange2.ParagraphFormat
        $fresh = Get-ParagraphSnapshot $paragraph2 "fresh_reopen"
        $geometryFresh = Get-LineGeometry $paragraphRange2 "fresh_reopen"
    }
    finally {
        Release-Com $paragraph2; Release-Com $paragraphRange2; Release-Com $range2; Release-Com $shape2
        Close-Document $doc2; Close-PubPublisherApplication $app2
    }

    $file = Get-Item -LiteralPath $output
    return [ordered]@{
        arm = $name
        mutation = $kind
        before_mutation = $before
        after_mutation = $after
        fresh_reopen = $fresh
        line_geometry_before_mutation = $geometryBefore
        line_geometry_after_mutation = $geometryAfter
        line_geometry_fresh_reopen = $geometryFresh
        output = [ordered]@{
            sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
            size = [int64]$file.Length
            private_pub_retained = $true
        }
    }
}

$seed = New-SeedFixture
$arms = @(
    Invoke-Arm "control" "control"
    Invoke-Arm "single" "single"
    Invoke-Arm "direct-0p75" "direct-0p75"
)

$result = [ordered]@{
    schema = "chaptera.paragraph-line-spacing-0p75-native-01.v1"
    experiment_id = $ExpectedExperiment
    seed = $seed
    arms = $arms
    verdict = "native-roundtrip-and-line-geometry-captured-not-yet-carrier-authority"
    boundary = "This stage records Publisher2019 COM mutation/save/fresh-reopen behavior plus per-line Start/End/BoundTop/BoundHeight geometry and retains exact private PUB outputs. It does not grant 0.75-SP carrier authority until structural FDPP/STSH/TEXT review is complete."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "paragraph-line-spacing-0p75-native-01.json")
@(
    "experiment=$ExpectedExperiment",
    "arms=3",
    "target=direct_line_spacing_0.75",
    "line_geometry=paragraph_range_lines_boundtop_boundheight",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "paragraph-line-spacing-0p75-native-01.txt") -Encoding ASCII
