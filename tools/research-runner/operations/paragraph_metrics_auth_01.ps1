param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "PARAGRAPH-METRICS-AUTH-01"
$PbFilePublication = 1
$MsoTextOrientationHorizontal = 1
$PbLineSpacingSingle = 0
$PbLineSpacing1pt5 = 1
$PbLineSpacingExactly = 4

Import-Module (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "windows\pub-runtime\PubRuntime.psm1") -Force

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) { throw "Unexpected experiment id: $($packet.id)" }
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) { throw "PUB_RESEARCH_FIXTURE missing" }

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private\paragraph-metrics-auth-01"
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
        alignment = Get-PubSafeValue { [int]$paragraph.Alignment } "ParagraphFormat.Alignment"
        first_line_indent = Get-PubSafeValue { [double]$paragraph.FirstLineIndent } "ParagraphFormat.FirstLineIndent"
        left_indent = Get-PubSafeValue { [double]$paragraph.LeftIndent } "ParagraphFormat.LeftIndent"
        right_indent = Get-PubSafeValue { [double]$paragraph.RightIndent } "ParagraphFormat.RightIndent"
        space_before = Get-PubSafeValue { [double]$paragraph.SpaceBefore } "ParagraphFormat.SpaceBefore"
        space_after = Get-PubSafeValue { [double]$paragraph.SpaceAfter } "ParagraphFormat.SpaceAfter"
        line_spacing = Get-PubSafeValue { [double]$paragraph.LineSpacing } "ParagraphFormat.LineSpacing"
        line_spacing_rule = Get-PubSafeValue { [int]$paragraph.LineSpacingRule } "ParagraphFormat.LineSpacingRule"
    }
}

function Get-FrameSnapshot($shape, [string]$phase) {
    return [ordered]@{
        phase = $phase
        left = Get-PubSafeValue { [double]$shape.Left } "Shape.Left"
        top = Get-PubSafeValue { [double]$shape.Top } "Shape.Top"
        width = Get-PubSafeValue { [double]$shape.Width } "Shape.Width"
        height = Get-PubSafeValue { [double]$shape.Height } "Shape.Height"
        overflowing = Get-PubSafeValue { [bool]$shape.TextFrame.Overflowing } "TextFrame.Overflowing"
        text_length = Get-PubSafeValue { [int]$shape.TextFrame.TextRange.Length } "TextRange.Length"
    }
}

function Apply-Arm($paragraph, [string]$kind, [double]$value) {
    switch ($kind) {
        "control" { return }
        "line-single" { $paragraph.SetLineSpacing($PbLineSpacingSingle, 12); return }
        "line-1p5" { $paragraph.SetLineSpacing($PbLineSpacing1pt5, 12); return }
        "line-exact" { $paragraph.SetLineSpacing($PbLineSpacingExactly, $value); return }
        "left-indent" { $paragraph.LeftIndent = $value; return }
        "right-indent" { $paragraph.RightIndent = $value; return }
        "first-line-indent" { $paragraph.FirstLineIndent = $value; return }
        "space-before" { $paragraph.SpaceBefore = $value; return }
        "space-after" { $paragraph.SpaceAfter = $value; return }
        default { throw "Unknown paragraph metric arm: $kind" }
    }
}

function Invoke-Arm([string]$name, [string]$kind, [double]$value) {
    $armDir = Join-Path $privateDir $name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $input = Join-Path $armDir "input.pub"
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $input -Force

    $app = $null
    $doc = $null
    $shape = $null
    $range = $null
    $paragraphRange = $null
    $paragraph = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($input, $false, $false)
        $shape = $doc.Pages.Item(1).Shapes.AddTextbox($MsoTextOrientationHorizontal, 72, 72, 360, 180)
        $range = $shape.TextFrame.TextRange
        $cr = [char]13
        $range.Text = "Chaptera paragraph metric alpha." + $cr + "Chaptera paragraph metric beta with enough words to make spacing visible." + $cr + "Chaptera paragraph metric gamma."
        $range.Font.Name = "Arial"
        $range.Font.Size = 12
        if ([int]$range.Paragraphs.Count -lt 3) { throw "Expected at least three paragraphs in synthetic fixture" }
        $paragraphRange = $range.Paragraphs.Item(2)
        $paragraph = $paragraphRange.ParagraphFormat

        $before = Get-ParagraphSnapshot $paragraph "before_mutation"
        $frameBefore = Get-FrameSnapshot $shape "before_mutation"
        Apply-Arm $paragraph $kind $value
        $afterMutation = Get-ParagraphSnapshot $paragraph "after_mutation"
        $frameAfter = Get-FrameSnapshot $shape "after_mutation"

        $doc.SaveAs($output, $PbFilePublication, $false)
    }
    finally {
        Release-Com $paragraph
        Release-Com $paragraphRange
        Release-Com $range
        Release-Com $shape
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $app2 = $null
    $doc2 = $null
    $shape2 = $null
    $range2 = $null
    $paragraphRange2 = $null
    $paragraph2 = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        if ([int]$doc2.Pages.Item(1).Shapes.Count -ne 1) { throw "Expected one shape after fresh reopen" }
        $shape2 = $doc2.Pages.Item(1).Shapes.Item(1)
        $range2 = $shape2.TextFrame.TextRange
        if ([int]$range2.Paragraphs.Count -lt 3) { throw "Expected at least three paragraphs after reopen" }
        $paragraphRange2 = $range2.Paragraphs.Item(2)
        $paragraph2 = $paragraphRange2.ParagraphFormat
        $fresh = Get-ParagraphSnapshot $paragraph2 "fresh_reopen"
        $frameFresh = Get-FrameSnapshot $shape2 "fresh_reopen"
    }
    finally {
        Release-Com $paragraph2
        Release-Com $paragraphRange2
        Release-Com $range2
        Release-Com $shape2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $file = Get-Item -LiteralPath $output
    return [ordered]@{
        arm = $name
        mutation = [ordered]@{ kind = $kind; requested_value = $value }
        before_mutation = $before
        after_mutation = $afterMutation
        fresh_reopen = $fresh
        frame_before_mutation = $frameBefore
        frame_after_mutation = $frameAfter
        frame_fresh_reopen = $frameFresh
        output = [ordered]@{
            size = [int64]$file.Length
            sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
            private_pub_retained = $true
        }
    }
}

$specs = @(
    [ordered]@{ name = "control"; kind = "control"; value = 0.0 },
    [ordered]@{ name = "line-single"; kind = "line-single"; value = 1.0 },
    [ordered]@{ name = "line-1p5"; kind = "line-1p5"; value = 1.5 },
    [ordered]@{ name = "line-exact-18pt"; kind = "line-exact"; value = 18.0 },
    [ordered]@{ name = "line-exact-24pt"; kind = "line-exact"; value = 24.0 },
    [ordered]@{ name = "left-indent-18pt"; kind = "left-indent"; value = 18.0 },
    [ordered]@{ name = "right-indent-18pt"; kind = "right-indent"; value = 18.0 },
    [ordered]@{ name = "first-line-18pt"; kind = "first-line-indent"; value = 18.0 },
    [ordered]@{ name = "hanging-18pt"; kind = "first-line-indent"; value = -18.0 },
    [ordered]@{ name = "space-before-12pt"; kind = "space-before"; value = 12.0 },
    [ordered]@{ name = "space-after-12pt"; kind = "space-after"; value = 12.0 }
)

$arms = @()
foreach ($spec in $specs) {
    $arms += Invoke-Arm -name $spec.name -kind $spec.kind -value ([double]$spec.value)
}

$result = [ordered]@{
    schema = "chaptera.paragraph-metrics-auth-01.native.v1"
    experiment_id = $ExpectedExperiment
    arms = $arms
    private_analysis_next = "Compare control/arm PUBs structurally: exact FDPP/STSH changed blocks, raw values, TEXT invariance and 0x34 line-spacing candidate. Public receipt must retain only source-safe normalized deltas."
    verdict = "native-semantic-arms-captured"
    boundary = "This stage proves Publisher2019 Save/reopen behavior and retains private PUB outputs. Persisted carrier interpretation is a separate structural analysis and must not be inferred from COM values alone."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "paragraph-metrics-auth-01.json")
@(
    "experiment=$ExpectedExperiment",
    "arms=$($specs.Count)",
    "line_spacing_rules=single:0,one_point_five:1,exactly:4",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "paragraph-metrics-auth-01.txt") -Encoding ASCII
