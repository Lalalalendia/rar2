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
$seedDir = Join-Path $privateDir "seed"
$seedPub = Join-Path $seedDir "seed.pub"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir,$seedDir | Out-Null

$progressPath = Join-Path $logDir "paragraph-metrics-progress.txt"
function Write-ProgressMarker([string]$stage) {
    ((Get-Date).ToString("o") + "`t" + $stage) | Add-Content -LiteralPath $progressPath -Encoding UTF8
}

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
        "line-proportional" { $paragraph.LineSpacing = $value; return }
        "line-exact" { $paragraph.SetLineSpacing($PbLineSpacingExactly, $value); return }
        "left-indent" { $paragraph.LeftIndent = $value; return }
        "right-indent" { $paragraph.RightIndent = $value; return }
        "first-line-indent" { $paragraph.FirstLineIndent = $value; return }
        "space-before" { $paragraph.SpaceBefore = $value; return }
        "space-after" { $paragraph.SpaceAfter = $value; return }
        default { throw "Unknown paragraph metric arm: $kind" }
    }
}

function New-SeedFixture() {
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $seedPub -Force
    Write-ProgressMarker "seed_copy_ready"

    $app = $null
    $doc = $null
    $shape = $null
    $range = $null
    $paragraphRange = $null
    $paragraph = $null
    try {
        Write-ProgressMarker "seed_app_create_begin"
        $app = New-PubPublisherApplication
        Write-ProgressMarker "seed_app_create_done"
        Write-ProgressMarker "seed_open_begin"
        $doc = $app.Open($seedPub, $false, $false)
        Write-ProgressMarker "seed_open_done"
        if ([int]$doc.Pages.Item(1).Shapes.Count -ne 0) { throw "Expected blank seed fixture before synthetic TextBox creation" }
        Write-ProgressMarker "seed_add_textbox_begin"
        $shape = $doc.Pages.Item(1).Shapes.AddTextbox($MsoTextOrientationHorizontal, 72, 72, 360, 180)
        Write-ProgressMarker "seed_add_textbox_done"
        $range = $shape.TextFrame.TextRange
        $cr = [char]13
        Write-ProgressMarker "seed_set_text_begin"
        $range.Text = "Chaptera paragraph metric alpha." + $cr + "Chaptera paragraph metric beta with enough words to make spacing visible." + $cr + "Chaptera paragraph metric gamma."
        $range.Font.Name = "Arial"
        $range.Font.Size = 12
        Write-ProgressMarker "seed_set_text_done"
        if ([int]$range.ParagraphsCount -lt 3) { throw "Expected at least three paragraphs in synthetic seed fixture" }
        Write-ProgressMarker "seed_select_paragraph_begin"
        $paragraphRange = $range.Paragraphs(2)
        $paragraph = $paragraphRange.ParagraphFormat
        Write-ProgressMarker "seed_select_paragraph_done"
        $seedParagraph = Get-ParagraphSnapshot $paragraph "seed_before_save"
        $seedFrame = Get-FrameSnapshot $shape "seed_before_save"
        Write-ProgressMarker "seed_save_begin"
        $doc.Save()
        Write-ProgressMarker "seed_save_done"
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
        Write-ProgressMarker "seed_reopen_app_begin"
        $app2 = New-PubPublisherApplication
        Write-ProgressMarker "seed_reopen_app_done"
        Write-ProgressMarker "seed_reopen_begin"
        $doc2 = $app2.Open($seedPub, $true, $false)
        Write-ProgressMarker "seed_reopen_done"
        if ([int]$doc2.Pages.Item(1).Shapes.Count -ne 1) { throw "Expected one shape in fresh-reopened seed fixture" }
        $shape2 = $doc2.Pages.Item(1).Shapes.Item(1)
        $range2 = $shape2.TextFrame.TextRange
        if ([int]$range2.ParagraphsCount -lt 3) { throw "Expected at least three paragraphs in fresh-reopened seed fixture" }
        $paragraphRange2 = $range2.Paragraphs(2)
        $paragraph2 = $paragraphRange2.ParagraphFormat
        $seedFresh = Get-ParagraphSnapshot $paragraph2 "seed_fresh_reopen"
        $seedFrameFresh = Get-FrameSnapshot $shape2 "seed_fresh_reopen"
        Write-ProgressMarker "seed_reopen_snapshot_done"
    }
    finally {
        Release-Com $paragraph2
        Release-Com $paragraphRange2
        Release-Com $range2
        Release-Com $shape2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $file = Get-Item -LiteralPath $seedPub
    return [ordered]@{
        size = [int64]$file.Length
        sha256 = (Get-FileHash -LiteralPath $seedPub -Algorithm SHA256).Hash.ToLowerInvariant()
        private_pub_retained = $true
        paragraph_before_save = $seedParagraph
        paragraph_fresh_reopen = $seedFresh
        frame_before_save = $seedFrame
        frame_fresh_reopen = $seedFrameFresh
    }
}

function Invoke-Arm([string]$name, [string]$kind, [double]$value) {
    $armDir = Join-Path $privateDir $name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $input = Join-Path $armDir "input.pub"
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $seedPub -Destination $input -Force
    Copy-Item -LiteralPath $seedPub -Destination $output -Force
    Write-ProgressMarker ("arm_" + $name + "_copies_ready")

    $app = $null
    $doc = $null
    $shape = $null
    $range = $null
    $paragraphRange = $null
    $paragraph = $null
    try {
        Write-ProgressMarker ("arm_" + $name + "_open_begin")
        $app = New-PubPublisherApplication
        $doc = $app.Open($output, $false, $false)
        Write-ProgressMarker ("arm_" + $name + "_open_done")
        if ([int]$doc.Pages.Item(1).Shapes.Count -ne 1) { throw "Expected one shape copied from the common seed fixture" }
        $shape = $doc.Pages.Item(1).Shapes.Item(1)
        $range = $shape.TextFrame.TextRange
        if ([int]$range.ParagraphsCount -lt 3) { throw "Expected at least three paragraphs copied from the common seed fixture" }
        $paragraphRange = $range.Paragraphs(2)
        $paragraph = $paragraphRange.ParagraphFormat
        Write-ProgressMarker ("arm_" + $name + "_paragraph_ready")

        $before = Get-ParagraphSnapshot $paragraph "before_mutation"
        $frameBefore = Get-FrameSnapshot $shape "before_mutation"
        Write-ProgressMarker ("arm_" + $name + "_mutation_begin")
        Apply-Arm $paragraph $kind $value
        Write-ProgressMarker ("arm_" + $name + "_mutation_done")
        $afterMutation = Get-ParagraphSnapshot $paragraph "after_mutation"
        $frameAfter = Get-FrameSnapshot $shape "after_mutation"

        Write-ProgressMarker ("arm_" + $name + "_save_begin")
        $doc.Save()
        Write-ProgressMarker ("arm_" + $name + "_save_done")
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
        Write-ProgressMarker ("arm_" + $name + "_reopen_begin")
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        Write-ProgressMarker ("arm_" + $name + "_reopen_done")
        if ([int]$doc2.Pages.Item(1).Shapes.Count -ne 1) { throw "Expected one shape after fresh reopen" }
        $shape2 = $doc2.Pages.Item(1).Shapes.Item(1)
        $range2 = $shape2.TextFrame.TextRange
        if ([int]$range2.ParagraphsCount -lt 3) { throw "Expected at least three paragraphs after reopen" }
        $paragraphRange2 = $range2.Paragraphs(2)
        $paragraph2 = $paragraphRange2.ParagraphFormat
        $fresh = Get-ParagraphSnapshot $paragraph2 "fresh_reopen"
        $frameFresh = Get-FrameSnapshot $shape2 "fresh_reopen"
        Write-ProgressMarker ("arm_" + $name + "_reopen_snapshot_done")
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

$seed = New-SeedFixture

$specs = @(
    [ordered]@{ name = "control"; kind = "control"; value = 0.0 },
    [ordered]@{ name = "line-single"; kind = "line-single"; value = 1.0 },
    [ordered]@{ name = "line-0p75"; kind = "line-proportional"; value = 0.75 },
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
    seed = $seed
    arms = $arms
    private_analysis_next = "Run tools/research-runner/analysis/paragraph_metrics_auth_01_blast_radius.py against this OutputRoot. It must compare the common seed to the matched no-op control and each one-property arm through OperationBlastRadiusV1. Raw FDPP 0x34 decoding and Quill TEXT invariance remain a separate structured-snapshot step until explicitly implemented."
    verdict = "native-semantic-arms-captured-with-common-seed"
    boundary = "This stage proves Publisher2019 Save/reopen behavior from one common seed and retains private PUB outputs. OperationBlastRadiusV1 may localize structural deltas, but persisted carrier semantics must not be inferred from byte inequality alone."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "paragraph-metrics-auth-01.json")
@(
    "experiment=$ExpectedExperiment",
    "arms=$($specs.Count)",
    "common_seed_sha256=$($seed.sha256)",
    "line_spacing_rules=single:0,one_point_five:1,exactly:4;direct_proportional_probe=0.75",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "paragraph-metrics-auth-01.txt") -Encoding ASCII
