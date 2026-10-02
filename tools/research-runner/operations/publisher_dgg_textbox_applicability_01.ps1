param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "VIEWER-PUBLISHER-DGG-TEXTBOX-APPLICABILITY-01"
$ExpectedFixtureSha256 = "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506"
$ExpectedArchiveName = "french_teacher_pub_pdf_corpus_2026-09-20.zip"
$ExpectedArchiveSha256 = "306fa66cf3238ceab0d91e7de79bb95d138331bdcbcf6c22aaa1b9b5510976f1"
$ExpectedFixtureName = "virginia-remplacante-modifiable.pub"
$PbFilePublication = 1
$TagName = "PUB_ORACLE_ID"
$TagValue = "DGG_TEXTBOX_APPLICABILITY_01"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}
function Resolve-ExactFixture {
    if (-not [string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) {
        if (-not (Test-Path -LiteralPath $env:PUB_RESEARCH_FIXTURE -PathType Leaf)) {
            throw "Configured PUB_RESEARCH_FIXTURE does not exist."
        }
        $hash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_FIXTURE -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne $ExpectedFixtureSha256) {
            throw "Configured PUB_RESEARCH_FIXTURE has the wrong SHA-256."
        }
        return (Resolve-Path -LiteralPath $env:PUB_RESEARCH_FIXTURE).Path
    }

    $root = [string]$env:PUB_RESEARCH_FIXTURE_ROOT
    if ([string]::IsNullOrWhiteSpace($root) -or -not (Test-Path -LiteralPath $root -PathType Container)) {
        throw "Set PUB_RESEARCH_FIXTURE to the exact PUB or PUB_RESEARCH_FIXTURE_ROOT to the local research corpus root."
    }

    $pubMatches = @(
        Get-ChildItem -LiteralPath $root -Recurse -File -Filter $ExpectedFixtureName -ErrorAction SilentlyContinue |
            Where-Object {
                (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -eq $ExpectedFixtureSha256
            }
    )
    if ($pubMatches.Count -eq 1) {
        return $pubMatches[0].FullName
    }
    if ($pubMatches.Count -gt 1) {
        throw "Multiple byte-identical Virginia PUB copies found; set PUB_RESEARCH_FIXTURE explicitly."
    }

    $archives = @(Get-ChildItem -LiteralPath $root -Recurse -File -Filter $ExpectedArchiveName -ErrorAction SilentlyContinue)
    $validArchives = @(
        $archives | Where-Object {
            (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -eq $ExpectedArchiveSha256
        }
    )
    if ($validArchives.Count -ne 1) {
        throw "Exact Virginia PUB not found and expected pinned French corpus archive is not uniquely available."
    }

    $extractRoot = Join-Path $OutputRoot "private/fixture-extract"
    if (Test-Path -LiteralPath $extractRoot) {
        Remove-Item -LiteralPath $extractRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
    Expand-Archive -LiteralPath $validArchives[0].FullName -DestinationPath $extractRoot -Force

    $extracted = @(
        Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter $ExpectedFixtureName |
            Where-Object {
                (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -eq $ExpectedFixtureSha256
            }
    )
    if ($extracted.Count -ne 1) {
        throw "Pinned French corpus archive did not yield exactly one exact Virginia PUB."
    }
    return $extracted[0].FullName
}

$fixturePath = Resolve-ExactFixture
$fixtureHash = (Get-FileHash -LiteralPath $fixturePath -Algorithm SHA256).Hash.ToLowerInvariant()

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/dgg-textbox-applicability-01"
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

function Build-OracleTool {
    $prebuilt = [string]$env:PUB_RESEARCH_DGG_ORACLE_TOOL
    if (-not [string]::IsNullOrWhiteSpace($prebuilt)) {
        if (-not (Test-Path -LiteralPath $prebuilt -PathType Leaf)) {
            throw "Configured PUB_RESEARCH_DGG_ORACLE_TOOL does not exist."
        }
        $resolved = (Resolve-Path -LiteralPath $prebuilt).Path
        $expectedHash = [string]$env:PUB_RESEARCH_DGG_ORACLE_TOOL_SHA256
        if ([string]::IsNullOrWhiteSpace($expectedHash)) {
            throw "PUB_RESEARCH_DGG_ORACLE_TOOL_SHA256 is required for a prebuilt oracle helper."
        }
        $actualHash = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualHash -ne $expectedHash.ToLowerInvariant()) {
            throw "Prebuilt DGG oracle helper SHA-256 mismatch."
        }
        return $resolved
    }

    Push-Location $repoRoot
    try {
        & cargo build --locked --release --manifest-path "vendor/producer-a/Cargo.toml" -p pub-reader --bin dgg_textbox_oracle_tool
        if ($LASTEXITCODE -ne 0) {
            throw "dgg_textbox_oracle_tool build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }

    $exe = Join-Path $repoRoot "vendor/producer-a/target/release/dgg_textbox_oracle_tool.exe"
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
        throw "dgg_textbox_oracle_tool.exe missing after build"
    }
    return $exe
}

function Invoke-Profile {
    param(
        [Parameter(Mandatory = $true)][string]$Tool,
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Output
    )
    & $Tool profile $Source $Output
    if ($LASTEXITCODE -ne 0) {
        throw "profile failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $Output -Raw | ConvertFrom-Json
}

function Find-ShapeLocationById {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][long]$ShapeId
    )

    $matches = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    if ([long]$shape.ID -eq $ShapeId) {
                        $matches += [pscustomobject]@{
                            page_index = $pageIndex
                            shape_index = $shapeIndex
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

    if ($matches.Count -eq 0) { return $null }
    if ($matches.Count -ne 1) {
        throw "Shape.ID=$ShapeId is not document-unique."
    }
    return $matches[0]
}

function Find-TaggedShapeLocation {
    param(
        [Parameter(Mandatory = $true)]$Document
    )

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
        throw "Expected exactly one tagged oracle shape; found $($matches.Count)"
    }
    return $matches[0]
}

function Get-FillSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][string]$Phase
    )

    $fill = $null
    $foreColor = $null
    try {
        $fill = $Shape.Fill
        $foreColor = $fill.ForeColor
        return [ordered]@{
            phase = $Phase
            shape_id = [long]$Shape.ID
            fill_visible = [long]$fill.Visible
            fill_type = [long]$fill.Type
            fill_forecolor_rgb = [long]$foreColor.RGB
        }
    }
    finally {
        Release-Com $foreColor
        Release-Com $fill
    }
}

function Select-TargetCandidate {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)]$Profile
    )

    $matches = @()
    foreach ($candidate in @($Profile.sparse_textbox_candidates)) {
        $location = Find-ShapeLocationById -Document $Document -ShapeId ([long]$candidate.publisher_shape_id)
        if ($null -eq $location) { continue }
        if ([int]$location.page_index -in @(24, 25)) {
            $matches += [pscustomobject]@{
                publisher_shape_id = [long]$candidate.publisher_shape_id
                page_index = [int]$location.page_index
                shape_index = [int]$location.shape_index
            }
        }
    }
    if ($matches.Count -eq 0) {
        throw "No source-profiled sparse TextBox candidate is present on Publisher page 24 or 25."
    }

    return @($matches | Sort-Object page_index,publisher_shape_id)[0]
}

function Is-SparseCandidateAfterReopen {
    param(
        [Parameter(Mandatory = $true)]$Profile,
        [Parameter(Mandatory = $true)][long]$ShapeId
    )
    return @(
        $Profile.sparse_textbox_candidates |
            Where-Object { [long]$_.publisher_shape_id -eq $ShapeId }
    ).Count -eq 1
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$InputPath,
        [Parameter(Mandatory = $true)]$SourceProfile,
        [Parameter(Mandatory = $true)][string]$Tool
    )

    $armDir = Join-Path $privateDir $Name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $InputPath -Destination $working -Force

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $before = $null
    $target = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $target = Select-TargetCandidate -Document $doc -Profile $SourceProfile

        $page = $doc.Pages.Item([int]$target.page_index)
        $shape = $page.Shapes.Item([int]$target.shape_index)
        if ([long]$shape.ID -ne [long]$target.publisher_shape_id) {
            throw "Target Shape.ID drifted before isolation."
        }

        $shape.Tags.Add($TagName, $TagValue) | Out-Null
        $before = Get-FillSnapshot -Shape $shape -Phase "before_delete_around"

        for ($shapeIndex = [int]$page.Shapes.Count; $shapeIndex -ge 1; $shapeIndex--) {
            $other = $null
            try {
                $other = $page.Shapes.Item($shapeIndex)
                if ([long]$other.ID -ne [long]$target.publisher_shape_id) {
                    $other.Delete()
                }
            }
            finally {
                Release-Com $other
            }
        }
        if ([int]$page.Shapes.Count -ne 1) {
            throw "Delete-around did not leave exactly one target shape."
        }

        Release-Com $shape
        $shape = $page.Shapes.Item(1)
        $isolated = Get-FillSnapshot -Shape $shape -Phase "after_delete_around"
        $doc.SaveAs($output, $PbFilePublication, $false)
    }
    finally {
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
        throw "$Name did not produce output.pub"
    }

    $app2 = $null
    $doc2 = $null
    $page2 = $null
    $shape2 = $null
    $after = $null
    $reopenShapeId = 0L
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        $location = Find-TaggedShapeLocation -Document $doc2
        $page2 = $doc2.Pages.Item([int]$location.page_index)
        $shape2 = $page2.Shapes.Item([int]$location.shape_index)
        $reopenShapeId = [long]$shape2.ID
        $after = Get-FillSnapshot -Shape $shape2 -Phase "fresh_reopen"
    }
    finally {
        Release-Com $shape2
        Release-Com $page2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $afterProfilePath = Join-Path $armDir "after-profile.json"
    $afterProfile = Invoke-Profile -Tool $Tool -Source $output -Output $afterProfilePath
    $sparseSurvives = Is-SparseCandidateAfterReopen -Profile $afterProfile -ShapeId $reopenShapeId

    return [ordered]@{
        arm = $Name
        target_page_index = [int]$target.page_index
        source_target_shape_id = [long]$target.publisher_shape_id
        before = $before
        isolated = $isolated
        fresh_reopen = $after
        sparse_state_survives = [bool]$sparseSurvives
        after_dgg_fill_color_op = $afterProfile.dgg_primary_fill_color_op
        output_sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
        private_output = $true
    }
}

$tool = Build-OracleTool
$sourceProfilePath = Join-Path $privateDir "source-profile.json"
$sourceProfile = Invoke-Profile -Tool $tool -Source $fixturePath -Output $sourceProfilePath
if ([string]$sourceProfile.source_sha256 -ne $ExpectedFixtureSha256) {
    throw "Profile source identity mismatch."
}
if ([int]$sourceProfile.dgg_primary_scalar_fill_color_count -ne 1) {
    throw "Expected one scalar DGG-primary fillColor in exact fixture."
}
if (@($sourceProfile.sparse_textbox_candidates).Count -eq 0) {
    throw "Exact fixture exposes no sparse TextBox candidate."
}

$controlInput = Join-Path $privateDir "control-input.pub"
Copy-Item -LiteralPath $fixturePath -Destination $controlInput -Force

$treatmentInput = Join-Path $privateDir "treatment-input.pub"
$patchReceiptPath = Join-Path $privateDir "dgg-patch.json"
& $tool patch-dgg-fill-color $fixturePath $treatmentInput $patchReceiptPath
if ($LASTEXITCODE -ne 0) {
    throw "DGG fillColor patch failed with exit code $LASTEXITCODE"
}
$patchReceipt = Get-Content -LiteralPath $patchReceiptPath -Raw | ConvertFrom-Json
if ([string]$patchReceipt.source_sha256 -ne $ExpectedFixtureSha256) {
    throw "DGG patch source identity mismatch."
}
if ([string]$patchReceipt.target_property_id -ne "0x0181") {
    throw "DGG patch touched unexpected property."
}
if ([int]$patchReceipt.changed_byte_count -lt 1 -or [int]$patchReceipt.changed_byte_count -gt 4) {
    throw "DGG patch changed unexpected byte count."
}

$treatmentProfilePath = Join-Path $privateDir "treatment-source-profile.json"
$treatmentProfile = Invoke-Profile -Tool $tool -Source $treatmentInput -Output $treatmentProfilePath
if ([long]$treatmentProfile.dgg_primary_fill_color_op -ne [long]$patchReceipt.new_op) {
    throw "Patched DGG fillColor did not reparse as requested."
}
$sourceCandidateIds = @($sourceProfile.sparse_textbox_candidates | ForEach-Object { [long]$_.publisher_shape_id })
$treatmentCandidateIds = @($treatmentProfile.sparse_textbox_candidates | ForEach-Object { [long]$_.publisher_shape_id })
if ((Compare-Object $sourceCandidateIds $treatmentCandidateIds).Count -ne 0) {
    throw "DGG-only patch changed sparse TextBox candidate identity set before Publisher."
}

$control = Invoke-Arm -Name "control" -InputPath $controlInput -SourceProfile $sourceProfile -Tool $tool
$treatment = Invoke-Arm -Name "treatment" -InputPath $treatmentInput -SourceProfile $treatmentProfile -Tool $tool

$sameTarget = (
    [int]$control.target_page_index -eq [int]$treatment.target_page_index -and
    [long]$control.source_target_shape_id -eq [long]$treatment.source_target_shape_id
)
$sparseSurvives = ([bool]$control.sparse_state_survives -and [bool]$treatment.sparse_state_survives)
$dggDeltaSurvives = (
    [long]$control.after_dgg_fill_color_op -eq [long]$patchReceipt.old_op -and
    [long]$treatment.after_dgg_fill_color_op -eq [long]$patchReceipt.new_op -and
    [long]$control.after_dgg_fill_color_op -ne [long]$treatment.after_dgg_fill_color_op
)
$visibleEqual = [long]$control.fresh_reopen.fill_visible -eq [long]$treatment.fresh_reopen.fill_visible
$typeEqual = [long]$control.fresh_reopen.fill_type -eq [long]$treatment.fresh_reopen.fill_type
$fillRgbChanged = [long]$control.fresh_reopen.fill_forecolor_rgb -ne [long]$treatment.fresh_reopen.fill_forecolor_rgb

$verdict = "inconclusive"
if ($sameTarget -and $sparseSurvives -and $dggDeltaSurvives -and $visibleEqual -and $typeEqual) {
    $verdict = if ($fillRgbChanged) {
        "sparse-textbox-fill-follows-dgg"
    } else {
        "publisher-specific-dgg-fillcolor-nonapplicability"
    }
}

$result = [ordered]@{
    schema = "chaptera.publisher-dgg-textbox-applicability.v1"
    experiment_id = $ExpectedExperiment
    fixture_sha256 = $fixtureHash
    publisher = [ordered]@{
        expected_version_prefix = "16.0.12527."
    }
    target = [ordered]@{
        page_index = [int]$control.target_page_index
        same_source_shape_both_arms = [bool]$sameTarget
        sparse_state_survives_both_arms = [bool]$sparseSurvives
    }
    causal_change = [ordered]@{
        property = "DGG-primary fillColor / OfficeArt 0x0181"
        bounded_cfb_stream_only = $true
        changed_stream = "/Escher/EscherStm"
        changed_byte_count = [int]$patchReceipt.changed_byte_count
        dgg_delta_survives_publisher_save_reopen = [bool]$dggDeltaSurvives
    }
    native_observation = [ordered]@{
        fill_visible_equal = [bool]$visibleEqual
        fill_type_equal = [bool]$typeEqual
        fill_forecolor_rgb_changed = [bool]$fillRgbChanged
    }
    verdict = $verdict
    authority_boundary = "The two arms differ before Publisher only in one structurally located DGG-primary fillColor scalar. Both arms then execute the same delete-around, SaveAs, close and fresh reopen. Raw post-save profiles must preserve sparse local TextBox state and the DGG delta before a semantic verdict is emitted. PDF pixels are not used as semantic authority."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "dgg-textbox-applicability-01.json")
@(
    "experiment=$ExpectedExperiment",
    "fixture_sha256=$fixtureHash",
    "target_page=$($result.target.page_index)",
    "same_target=$sameTarget",
    "sparse_survives=$sparseSurvives",
    "dgg_delta_survives=$dggDeltaSurvives",
    "fill_visible_equal=$visibleEqual",
    "fill_type_equal=$typeEqual",
    "fill_rgb_changed=$fillRgbChanged",
    "verdict=$verdict"
) | Set-Content -LiteralPath (Join-Path $logDir "dgg-textbox-applicability-01.txt") -Encoding ASCII

if ($verdict -eq "inconclusive") {
    throw "DGG TextBox applicability experiment remained inconclusive; inspect private local arm receipts."
}
