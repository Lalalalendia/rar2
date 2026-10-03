param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "CARLTON-HEADING-ALIGNMENT-READONLY-01"
$ExpectedFixtureSha256 = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

$Targets = @(
    [pscustomobject]@{ label = "reception_heading"; text = "Reception" },
    [pscustomobject]@{ label = "year1_heading"; text = "Year 1" },
    [pscustomobject]@{ label = "year2_heading"; text = "Year 2" }
)

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) {
    throw "PUB_RESEARCH_FIXTURE was not resolved by prepare_native_run.ps1."
}
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_PUBLISHER_EXE)) {
    throw "PUB_RESEARCH_PUBLISHER_EXE was not resolved by prepare_native_run.ps1."
}

$fixtureHash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_FIXTURE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($fixtureHash -ne $ExpectedFixtureSha256) {
    throw "Exact Carlton fixture mismatch: $fixtureHash"
}
$publisherHash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_PUBLISHER_EXE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($publisherHash -ne $ExpectedPublisherExeSha256) {
    throw "Publisher executable SHA-256 mismatch: $publisherHash"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir | Out-Null

function Release-Com($Value) {
    if ($null -ne $Value -and [System.Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Find-TargetLocations {
    param([Parameter(Mandatory = $true)]$Document)

    $found = @{}
    foreach ($target in $Targets) {
        $found[$target.label] = @()
    }

    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                $textFrame = $null
                $textRange = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    try {
                        $textFrame = $shape.TextFrame
                        $textRange = $textFrame.TextRange
                        $text = [string]$textRange.Text
                        $normalized = $text.TrimEnd([char[]]([char]13,[char]10))
                        foreach ($target in $Targets) {
                            if ($normalized -eq $target.text) {
                                $found[$target.label] += [pscustomobject]@{
                                    page_index = $pageIndex
                                    shape_index = $shapeIndex
                                }
                            }
                        }
                    }
                    catch {
                        # Non-text shapes are expected.
                    }
                }
                finally {
                    Release-Com $textRange
                    Release-Com $textFrame
                    Release-Com $shape
                }
            }
        }
        finally {
            Release-Com $page
        }
    }

    foreach ($target in $Targets) {
        $matches = @($found[$target.label])
        if ($matches.Count -ne 1) {
            throw "Expected exactly one $($target.label) target in exact Carlton source; found $($matches.Count)."
        }
    }
    return $found
}

function Get-TargetSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)]$Location
    )

    $page = $null
    $shape = $null
    $textFrame = $null
    $textRange = $null
    $paragraph = $null
    try {
        $page = $Document.Pages.Item([int]$Location.page_index)
        $shape = $page.Shapes.Item([int]$Location.shape_index)
        $textFrame = $shape.TextFrame
        $textRange = $textFrame.TextRange
        $paragraph = $textRange.ParagraphFormat

        return [ordered]@{
            target = $Label
            page_index = [int]$Location.page_index
            text_length = Get-PubSafeValue { [int]$textRange.Length } "TextRange.Length"
            effective_alignment = Get-PubSafeValue { [int]$paragraph.Alignment } "ParagraphFormat.Alignment"
            shape_left_points = Get-PubSafeValue { [double]$shape.Left } "Shape.Left"
            shape_top_points = Get-PubSafeValue { [double]$shape.Top } "Shape.Top"
            shape_width_points = Get-PubSafeValue { [double]$shape.Width } "Shape.Width"
            shape_height_points = Get-PubSafeValue { [double]$shape.Height } "Shape.Height"
        }
    }
    finally {
        Release-Com $paragraph
        Release-Com $textRange
        Release-Com $textFrame
        Release-Com $shape
        Release-Com $page
    }
}

$app = $null
$doc = $null
try {
    $app = New-PubPublisherApplication
    $publisher = [ordered]@{
        version = [string]$app.Version
        build = [string]$app.Build
    }
    $doc = $app.Open($env:PUB_RESEARCH_FIXTURE, $true, $false)
    $locations = Find-TargetLocations -Document $doc
    $targets = @()
    foreach ($target in $Targets) {
        $targets += Get-TargetSnapshot -Document $doc -Label $target.label -Location @($locations[$target.label])[0]
    }
}
finally {
    if ($null -ne $doc) {
        try { $doc.Close() } catch {}
        Release-Com $doc
    }
    Close-PubPublisherApplication $app
}

$fixtureHashAfter = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_FIXTURE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($fixtureHashAfter -ne $fixtureHash) {
    throw "Read-only Carlton COM probe changed the fixture hash."
}

$alignmentStates = @($targets | ForEach-Object {
    if ($_.effective_alignment.state -eq "value") { [int]$_.effective_alignment.value } else { $null }
})
$verdict = if ($alignmentStates.Count -eq 3 -and $null -notin $alignmentStates) {
    "effective-alignment-observed"
} else {
    "inconclusive"
}

$result = [ordered]@{
    schema = "chaptera.carlton-heading-alignment-readonly-01.v1"
    experiment_id = $ExpectedExperiment
    fixture = [ordered]@{
        sha256 = $fixtureHash
        source_unchanged = ($fixtureHashAfter -eq $fixtureHash)
    }
    publisher = [ordered]@{
        exe_sha256 = $publisherHash
        version = $publisher.version
        build = $publisher.build
    }
    targets = $targets
    verdict = $verdict
    evidence_boundary = "Read-only Publisher2019 COM observation on the exact public Carlton source. Target text is used only internally to identify the Reception, Year 1 and Year 2 heading shapes; evidence emits semantic labels, effective ParagraphFormat.Alignment and shape geometry, not Story text or PUB bytes."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "carlton-heading-alignment-readonly-01.json")
@(
    "experiment=$ExpectedExperiment",
    "fixture_sha256=$fixtureHash",
    "source_unchanged=$($fixtureHashAfter -eq $fixtureHash)",
    "reception_alignment=$(if ($targets[0].effective_alignment.state -eq 'value') { $targets[0].effective_alignment.value } else { 'unavailable' })",
    "year1_alignment=$(if ($targets[1].effective_alignment.state -eq 'value') { $targets[1].effective_alignment.value } else { 'unavailable' })",
    "year2_alignment=$(if ($targets[2].effective_alignment.state -eq 'value') { $targets[2].effective_alignment.value } else { 'unavailable' })",
    "verdict=$verdict"
) | Set-Content -LiteralPath (Join-Path $logDir "carlton-heading-alignment-readonly-01.txt") -Encoding ASCII

if ($verdict -eq "inconclusive") {
    throw "Carlton heading read-only COM alignment was unavailable; inspect evidence."
}
