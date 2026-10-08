param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [switch]$MixedSize
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

function Write-ResearchStage([string]$Stage) {
    Write-Host ("PUB_RESEARCH_STAGE stage={0}" -f $Stage)
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

function Get-MixedSizeSnapshot($range, [string]$phase) {
    $samples = @()
    foreach ($sample in @(
        @{ position = 1; role = "prefix" },
        @{ position = 24; role = "large" },
        @{ position = 52; role = "suffix" }
    )) {
        $char = $null
        try {
            $char = $range.Characters([int]$sample.position, 1)
            $samples += [ordered]@{
                role = $sample.role
                family = Get-PubSafeValue { [string]$char.Font.Name } "TextRange.Font.Name"
                point_size = Get-PubSafeValue { [double]$char.Font.Size } "TextRange.Font.Size"
            }
        }
        finally { Release-Com $char }
    }
    $paragraph = $null
    try {
        $paragraph = $range.ParagraphFormat
        return [ordered]@{
            phase = $phase
            range_start = [int]$range.Start
            range_end = [int]$range.End
            font_samples = $samples
            paragraph = Get-ParagraphSnapshot $paragraph $phase
            geometry = Get-LineGeometry $range $phase
        }
    }
    finally { Release-Com $paragraph }
}

function New-SeedFixture() {
    Write-ResearchStage "seed_copy_begin"
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $seedPub -Force
    Write-ResearchStage "seed_copy_complete"

    $app = $null; $doc = $null; $shape = $null; $range = $null; $paragraphRange = $null; $paragraph = $null
    $mixedSeedRange = $null; $mixedSeedRun = $null
    try {
        Write-ResearchStage "seed_application_create_begin"
        $app = New-PubPublisherApplication
        Write-ResearchStage "seed_application_create_complete"

        Write-ResearchStage "seed_open_begin"
        $doc = $app.Open($seedPub, $false, $false)
        Write-ResearchStage "seed_open_complete"

        if ([int]$doc.Pages.Item(1).Shapes.Count -ne 0) { throw "Expected blank seed fixture" }

        Write-ResearchStage "seed_textbox_create_begin"
        $shape = $doc.Pages.Item(1).Shapes.AddTextbox($MsoTextOrientationHorizontal, 72, 72, 360, 180)
        Write-ResearchStage "seed_textbox_create_complete"

        $range = $shape.TextFrame.TextRange
        $cr = [char]13
        $range.Text = "Chaptera spacing alpha with enough words to wrap naturally." + $cr +
            "Chaptera spacing beta with enough words to wrap naturally and expose vertical progression." + $cr +
            "Chaptera spacing gamma with enough words to wrap naturally."
        $range.Font.Name = "Arial"
        $range.Font.Size = 12
        if ([int]$range.ParagraphsCount -lt 3) { throw "Expected three paragraphs" }
        if ($MixedSize) {
            # Separate third-paragraph witness, leaving uniform paragraph 2 intact.
            $mixedSeedRange = $range.Paragraphs(3)
            $mixedSeedRun = $mixedSeedRange.Characters(18, 15)
            $mixedSeedRun.Font.Size = 18
            Release-Com $mixedSeedRun
            $mixedSeedSnapshot = Get-MixedSizeSnapshot $mixedSeedRange "seed_before_save"
            Release-Com $mixedSeedRange
        }
        $paragraphRange = $range.Paragraphs(2)
        $paragraph = $paragraphRange.ParagraphFormat
        $before = Get-ParagraphSnapshot $paragraph "seed_before_save"
        $seedGeometry = Get-LineGeometry $paragraphRange "seed_before_save"

        Write-ResearchStage "seed_save_begin"
        $doc.Save()
        Write-ResearchStage "seed_save_complete"
    }
    finally {
        Release-Com $mixedSeedRun; Release-Com $mixedSeedRange
        Release-Com $paragraph; Release-Com $paragraphRange; Release-Com $range; Release-Com $shape
        if ($null -ne $doc) { Write-ResearchStage "seed_document_close_begin" }
        Close-Document $doc
        if ($null -ne $doc) { Write-ResearchStage "seed_document_close_complete" }
        if ($null -ne $app) { Write-ResearchStage "seed_application_quit_begin" }
        Close-PubPublisherApplication $app
        if ($null -ne $app) { Write-ResearchStage "seed_application_quit_complete" }
    }
    $file = Get-Item -LiteralPath $seedPub
    return [ordered]@{
        sha256 = (Get-FileHash -LiteralPath $seedPub -Algorithm SHA256).Hash.ToLowerInvariant()
        size = [int64]$file.Length
        paragraph_before_save = $before
        line_geometry_before_save = $seedGeometry
        mixed_size_before_save = $(if ($MixedSize) { $mixedSeedSnapshot } else { $null })
    }
}

function Invoke-Arm([string]$name, [string]$kind) {
    $armDir = Join-Path $privateDir $name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $seedPub -Destination $output -Force

    $app = $null; $doc = $null; $shape = $null; $range = $null; $paragraphRange = $null; $paragraph = $null
    $mixedRange = $null; $mixedParagraph = $null
    try {
        Write-ResearchStage ("arm_{0}_application_create_begin" -f $name)
        $app = New-PubPublisherApplication
        Write-ResearchStage ("arm_{0}_application_create_complete" -f $name)

        Write-ResearchStage ("arm_{0}_open_begin" -f $name)
        $doc = $app.Open($output, $false, $false)
        Write-ResearchStage ("arm_{0}_open_complete" -f $name)

        $shape = $doc.Pages.Item(1).Shapes.Item(1)
        $range = $shape.TextFrame.TextRange
        $paragraphRange = $range.Paragraphs(2)
        $paragraph = $paragraphRange.ParagraphFormat
        $before = Get-ParagraphSnapshot $paragraph "before_mutation"
        $geometryBefore = Get-LineGeometry $paragraphRange "before_mutation"
        if ($MixedSize) {
            $mixedRange = $range.Paragraphs(3)
            $mixedBefore = Get-MixedSizeSnapshot $mixedRange "before_mutation"
        }

        Write-ResearchStage ("arm_{0}_mutation_begin" -f $name)
        switch ($kind) {
            "control" { }
            "single" { $paragraph.SetLineSpacing($PbLineSpacingSingle, 12) }
            "direct-1p0" { $paragraph.LineSpacing = 1.0 }
            "direct-0p80" { $paragraph.LineSpacing = 0.80 }
            "direct-0p75" { $paragraph.LineSpacing = 0.75 }
            default { throw "Unknown arm kind: $kind" }
        }
        if ($MixedSize -and $kind -in @("direct-0p80", "direct-0p75")) {
            $mixedParagraph = $mixedRange.ParagraphFormat
            $mixedParagraph.LineSpacing = $(if ($kind -eq "direct-0p80") { 0.80 } else { 0.75 })
            Release-Com $mixedParagraph
        }
        Write-ResearchStage ("arm_{0}_mutation_complete" -f $name)
        if ($MixedSize) {
            $mixedAfter = Get-MixedSizeSnapshot $mixedRange "after_mutation"
            Release-Com $mixedRange
        }

        $after = Get-ParagraphSnapshot $paragraph "after_mutation"
        $geometryAfter = Get-LineGeometry $paragraphRange "after_mutation"

        Write-ResearchStage ("arm_{0}_save_begin" -f $name)
        $doc.Save()
        Write-ResearchStage ("arm_{0}_save_complete" -f $name)
    }
    finally {
        Release-Com $mixedParagraph; Release-Com $mixedRange
        Release-Com $paragraph; Release-Com $paragraphRange; Release-Com $range; Release-Com $shape
        if ($null -ne $doc) { Write-ResearchStage ("arm_{0}_document_close_begin" -f $name) }
        Close-Document $doc
        if ($null -ne $doc) { Write-ResearchStage ("arm_{0}_document_close_complete" -f $name) }
        if ($null -ne $app) { Write-ResearchStage ("arm_{0}_application_quit_begin" -f $name) }
        Close-PubPublisherApplication $app
        if ($null -ne $app) { Write-ResearchStage ("arm_{0}_application_quit_complete" -f $name) }
    }

    $app2 = $null; $doc2 = $null; $shape2 = $null; $range2 = $null; $paragraphRange2 = $null; $paragraph2 = $null
    $mixedRange2 = $null
    try {
        Write-ResearchStage ("arm_{0}_reopen_application_create_begin" -f $name)
        $app2 = New-PubPublisherApplication
        Write-ResearchStage ("arm_{0}_reopen_application_create_complete" -f $name)

        Write-ResearchStage ("arm_{0}_reopen_open_begin" -f $name)
        $doc2 = $app2.Open($output, $true, $false)
        Write-ResearchStage ("arm_{0}_reopen_open_complete" -f $name)

        $shape2 = $doc2.Pages.Item(1).Shapes.Item(1)
        $range2 = $shape2.TextFrame.TextRange
        $paragraphRange2 = $range2.Paragraphs(2)
        $paragraph2 = $paragraphRange2.ParagraphFormat
        $fresh = Get-ParagraphSnapshot $paragraph2 "fresh_reopen"
        $geometryFresh = Get-LineGeometry $paragraphRange2 "fresh_reopen"
        if ($MixedSize) {
            $mixedRange2 = $range2.Paragraphs(3)
            $mixedFresh = Get-MixedSizeSnapshot $mixedRange2 "fresh_reopen"
            Release-Com $mixedRange2
        }
    }
    finally {
        Release-Com $mixedRange2
        Release-Com $paragraph2; Release-Com $paragraphRange2; Release-Com $range2; Release-Com $shape2
        if ($null -ne $doc2) { Write-ResearchStage ("arm_{0}_reopen_document_close_begin" -f $name) }
        Close-Document $doc2
        if ($null -ne $doc2) { Write-ResearchStage ("arm_{0}_reopen_document_close_complete" -f $name) }
        if ($null -ne $app2) { Write-ResearchStage ("arm_{0}_reopen_application_quit_begin" -f $name) }
        Close-PubPublisherApplication $app2
        if ($null -ne $app2) { Write-ResearchStage ("arm_{0}_reopen_application_quit_complete" -f $name) }
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
        mixed_size = $(if ($MixedSize) {
            [ordered]@{
                before_mutation = $mixedBefore
                after_mutation = $mixedAfter
                fresh_reopen = $mixedFresh
            }
        } else { $null })
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
    Invoke-Arm "direct-1p0" "direct-1p0"
    Invoke-Arm "direct-0p80" "direct-0p80"
    Invoke-Arm "direct-0p75" "direct-0p75"
)

$result = [ordered]@{
    schema = "chaptera.paragraph-line-spacing-0p75-native-01.v1"
    experiment_id = $ExpectedExperiment
    seed = $seed
    composition_profile = $(if ($MixedSize) { "mixed-12-18-12" } else { "uniform-12" })
    arms = $arms
    verdict = "native-roundtrip-and-line-geometry-captured-not-yet-carrier-authority"
    boundary = "This stage records Publisher2019 COM mutation/save/fresh-reopen behavior plus per-line Start/End/BoundTop/BoundHeight geometry and retains exact private PUB outputs. It does not grant 0.75-SP carrier authority until structural FDPP/STSH/TEXT review is complete."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "paragraph-line-spacing-0p75-native-01.json")
@(
    "experiment=$ExpectedExperiment",
    "arms=5",
    "composition_profile=$($result.composition_profile)",
    "matched_control=direct_line_spacing_0.80",
    "target=direct_line_spacing_0.75",
    "line_geometry=paragraph_range_lines_boundtop_boundheight",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "paragraph-line-spacing-0p75-native-01.txt") -Encoding ASCII
