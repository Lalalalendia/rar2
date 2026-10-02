param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "VIEWER-TABLE-AUTOFORMAT-BORDER-CARRIERS-01"
$PbFilePublication = 1
$PbTableAutoFormatCheckbookRegister = 0
$MutationRgb = 255

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/table-border-carrier-map-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

$progressPath = Join-Path $analysisDir "table-border-carrier-map-01-progress.jsonl"
function Write-Progress {
    param(
        [Parameter(Mandatory = $true)][string]$Stage,
        [string]$Side = ""
    )
    $record = [ordered]@{
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        stage = $Stage
        side = $Side
    }
    ($record | ConvertTo-Json -Compress) | Add-Content -LiteralPath $progressPath -Encoding UTF8
}
Write-Progress -Stage "operation-started"

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

function Find-UniqueTableShape {
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
                    if ([int]$shape.HasTable -eq -1) {
                        $matches += [pscustomobject]@{
                            page_index = $pageIndex
                            shape_index = $shapeIndex
                        }
                    }
                }
                finally { Release-Com $shape }
            }
        }
        finally { Release-Com $page }
    }
    if ($matches.Count -ne 1) {
        throw "Expected exactly one table shape in generated fixture; found $($matches.Count)."
    }
    return $matches[0]
}

function Build-OracleTool {
    $prebuilt = [string]$env:PUB_RESEARCH_TABLE_BORDER_ORACLE_TOOL
    if (-not [string]::IsNullOrWhiteSpace($prebuilt)) {
        if (-not (Test-Path -LiteralPath $prebuilt -PathType Leaf)) {
            throw "Configured PUB_RESEARCH_TABLE_BORDER_ORACLE_TOOL does not exist."
        }
        $resolved = (Resolve-Path -LiteralPath $prebuilt).Path
        $expectedHash = [string]$env:PUB_RESEARCH_TABLE_BORDER_ORACLE_TOOL_SHA256
        if ([string]::IsNullOrWhiteSpace($expectedHash)) {
            throw "PUB_RESEARCH_TABLE_BORDER_ORACLE_TOOL_SHA256 is required for a prebuilt oracle helper."
        }
        $actualHash = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualHash -ne $expectedHash.ToLowerInvariant()) {
            throw "Prebuilt TABLE border oracle helper SHA-256 mismatch."
        }
        return $resolved
    }

    Push-Location $repoRoot
    try {
        & cargo build --release --manifest-path "vendor/producer-a/Cargo.toml" -p pub-reader --bin table_border_carrier_oracle_tool
        if ($LASTEXITCODE -ne 0) {
            throw "table_border_carrier_oracle_tool build failed with exit code $LASTEXITCODE"
        }
    }
    finally { Pop-Location }

    $exe = Join-Path $repoRoot "vendor/producer-a/target/release/table_border_carrier_oracle_tool.exe"
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
        throw "table_border_carrier_oracle_tool.exe missing after build"
    }
    return $exe
}

function New-AllEnabledFixture {
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
        $table = $shape.Table

        for ($rowIndex = 1; $rowIndex -le 4; $rowIndex++) {
            $row = $null
            try {
                $row = $table.Rows.Item($rowIndex)
                for ($cellIndex = 1; $cellIndex -le 4; $cellIndex++) {
                    $cell = $null
                    $text = $null
                    try {
                        $cell = $row.Cells.Item($cellIndex)
                        $text = $cell.TextRange
                        $text.Text = "R$($rowIndex)C$($cellIndex)"
                    }
                    finally {
                        Release-Com $text
                        Release-Com $cell
                    }
                }
            }
            finally { Release-Com $row }
        }

        $table.ApplyAutoFormat($PbTableAutoFormatCheckbookRegister, $true, $true, $true, $true)
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

function Get-Cell {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][int]$RowIndex,
        [Parameter(Mandatory = $true)][int]$ColumnIndex
    )
    $row = $Table.Rows.Item($RowIndex)
    try {
        return $row.Cells.Item($ColumnIndex)
    }
    finally { Release-Com $row }
}

function Get-Border {
    param(
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)][string]$Side
    )
    switch ($Side) {
        "top" { return $Cell.BorderTop }
        "right" { return $Cell.BorderRight }
        "bottom" { return $Cell.BorderBottom }
        "left" { return $Cell.BorderLeft }
        default { throw "Unsupported side $Side" }
    }
}

function Get-OppositeNeighbor {
    param([Parameter(Mandatory = $true)][string]$Side)
    switch ($Side) {
        "top" { return [ordered]@{ row = 1; col = 2; side = "bottom" } }
        "right" { return [ordered]@{ row = 2; col = 3; side = "left" } }
        "bottom" { return [ordered]@{ row = 3; col = 2; side = "top" } }
        "left" { return [ordered]@{ row = 2; col = 1; side = "right" } }
    }
}

function Get-BorderRgb {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][int]$RowIndex,
        [Parameter(Mandatory = $true)][int]$ColumnIndex,
        [Parameter(Mandatory = $true)][string]$Side
    )
    $cell = $null
    $border = $null
    $color = $null
    try {
        $cell = Get-Cell -Table $Table -RowIndex $RowIndex -ColumnIndex $ColumnIndex
        $border = Get-Border -Cell $cell -Side $Side
        $color = $border.Color
        return [long]$color.RGB
    }
    finally {
        Release-Com $color
        Release-Com $border
        Release-Com $cell
    }
}

function Set-BorderRgb {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][string]$Side,
        [Parameter(Mandatory = $true)][long]$Rgb
    )
    $cell = $null
    $border = $null
    $color = $null
    try {
        $cell = Get-Cell -Table $Table -RowIndex 2 -ColumnIndex 2
        $border = Get-Border -Cell $cell -Side $Side
        $color = $border.Color
        $color.RGB = $Rgb
    }
    finally {
        Release-Com $color
        Release-Com $border
        Release-Com $cell
    }
}

function Get-BorderWeight {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][int]$RowIndex,
        [Parameter(Mandatory = $true)][int]$ColumnIndex,
        [Parameter(Mandatory = $true)][string]$Side
    )
    $cell = $null
    $border = $null
    try {
        $cell = Get-Cell -Table $Table -RowIndex $RowIndex -ColumnIndex $ColumnIndex
        $border = Get-Border -Cell $cell -Side $Side
        return [double]$border.Weight
    }
    finally {
        Release-Com $border
        Release-Com $cell
    }
}

function Set-BorderWeight {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][string]$Side,
        [Parameter(Mandatory = $true)][double]$Weight
    )
    $cell = $null
    $border = $null
    try {
        $cell = Get-Cell -Table $Table -RowIndex 2 -ColumnIndex 2
        $border = Get-Border -Cell $cell -Side $Side
        $border.Weight = $Weight
    }
    finally {
        Release-Com $border
        Release-Com $cell
    }
}

function Test-Near {
    param(
        [Parameter(Mandatory = $true)][double]$Left,
        [Parameter(Mandatory = $true)][double]$Right,
        [double]$Tolerance = 0.01
    )
    return [bool]([Math]::Abs($Left - $Right) -le $Tolerance)
}

function Invoke-WeightArm {
    param(
        [Parameter(Mandatory = $true)][double]$DeltaPoints,
        [Parameter(Mandatory = $true)][string]$AllEnabledPath,
        [Parameter(Mandatory = $true)][string]$Tool
    )

    $armName = "weight-delta-" + ([string]$DeltaPoints).Replace(".", "_")
    Write-Progress -Stage "weight-arm-start" -Side $armName
    $armDir = Join-Path $privateDir $armName
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    Copy-Item -LiteralPath $AllEnabledPath -Destination $working -Force

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $table = $null
    $targetBeforeWeight = 0.0
    $neighborBeforeWeight = 0.0
    $targetBeforeRgb = 0L
    $neighborBeforeRgb = 0L
    $requestedWeight = 0.0
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $location = Find-UniqueTableShape -Document $doc
        $page = $doc.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $table = $shape.Table

        $targetBeforeWeight = Get-BorderWeight -Table $table -RowIndex 2 -ColumnIndex 2 -Side "top"
        $neighborBeforeWeight = Get-BorderWeight -Table $table -RowIndex 1 -ColumnIndex 2 -Side "bottom"
        $targetBeforeRgb = Get-BorderRgb -Table $table -RowIndex 2 -ColumnIndex 2 -Side "top"
        $neighborBeforeRgb = Get-BorderRgb -Table $table -RowIndex 1 -ColumnIndex 2 -Side "bottom"
        $requestedWeight = $targetBeforeWeight + $DeltaPoints
        Set-BorderWeight -Table $table -Side "top" -Weight $requestedWeight
        Write-Progress -Stage "weight-border-mutated" -Side $armName
        $doc.Save()
        Write-Progress -Stage "weight-save-complete" -Side $armName
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
    $page2 = $null
    $shape2 = $null
    $table2 = $null
    $targetAfterWeight = 0.0
    $neighborAfterWeight = 0.0
    $targetAfterRgb = 0L
    $neighborAfterRgb = 0L
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($working, $true, $false)
        $location2 = Find-UniqueTableShape -Document $doc2
        $page2 = $doc2.Pages.Item([int]$location2.page_index)
        $shape2 = $page2.Shapes.Item([int]$location2.shape_index)
        $table2 = $shape2.Table

        $targetAfterWeight = Get-BorderWeight -Table $table2 -RowIndex 2 -ColumnIndex 2 -Side "top"
        $neighborAfterWeight = Get-BorderWeight -Table $table2 -RowIndex 1 -ColumnIndex 2 -Side "bottom"
        $targetAfterRgb = Get-BorderRgb -Table $table2 -RowIndex 2 -ColumnIndex 2 -Side "top"
        $neighborAfterRgb = Get-BorderRgb -Table $table2 -RowIndex 1 -ColumnIndex 2 -Side "bottom"
        Write-Progress -Stage "weight-reopen-read-after" -Side $armName
    }
    finally {
        Release-Com $table2
        Release-Com $shape2
        Release-Com $page2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $diffPath = Join-Path $armDir "raw-diff.json"
    & $Tool diff $AllEnabledPath $working $diffPath
    if ($LASTEXITCODE -ne 0) {
        throw "raw carrier weight diff failed for $armName"
    }
    $privateDiffPath = Join-Path $armDir "raw-private-diff.json"
    & $Tool diff-private $AllEnabledPath $working $privateDiffPath
    if ($LASTEXITCODE -ne 0) {
        throw "private raw carrier weight diff failed for $armName"
    }
    $rawDiff = Get-Content -LiteralPath $diffPath -Raw | ConvertFrom-Json
    Write-Progress -Stage "weight-raw-diff-after" -Side $armName

    return [ordered]@{
        arm = $armName
        delta_points = $DeltaPoints
        requested_weight_points = $requestedWeight
        target_before_weight_points = $targetBeforeWeight
        target_after_weight_points = $targetAfterWeight
        neighbor_before_weight_points = $neighborBeforeWeight
        neighbor_after_weight_points = $neighborAfterWeight
        requested_weight_persisted = [bool](
            (Test-Near -Left $targetAfterWeight -Right $requestedWeight) -and
            (Test-Near -Left $neighborAfterWeight -Right $requestedWeight)
        )
        color_unchanged = [bool](
            $targetBeforeRgb -eq $targetAfterRgb -and
            $neighborBeforeRgb -eq $neighborAfterRgb
        )
        raw = [ordered]@{
            matched_carrier_count = [int]$rawDiff.matched_carrier_count
            changed_carrier_count = [int]$rawDiff.changed_carrier_count
            removed_carrier_count = [int]$rawDiff.removed_carrier_count
            added_carrier_count = [int]$rawDiff.added_carrier_count
            ambiguous_changed_group_count = [int]$rawDiff.ambiguous_changed_group_count
            changed_anchor_signature_histogram = $rawDiff.changed_anchor_signature_histogram
            changed_property_id_histogram = $rawDiff.changed_property_id_histogram
            removed_anchor_signature_histogram = $rawDiff.removed_anchor_signature_histogram
            added_anchor_signature_histogram = $rawDiff.added_anchor_signature_histogram
        }
    }
}

function Invoke-SideArm {
    param(
        [Parameter(Mandatory = $true)][string]$Side,
        [Parameter(Mandatory = $true)][string]$AllEnabledPath,
        [Parameter(Mandatory = $true)][string]$Tool
    )

    Write-Progress -Stage "side-arm-start" -Side $Side
    $armDir = Join-Path $privateDir $Side
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    $output = $working
    Copy-Item -LiteralPath $AllEnabledPath -Destination $working -Force

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $table = $null
    $targetBefore = 0L
    $neighborBefore = 0L
    try {
        Write-Progress -Stage "side-open-before" -Side $Side
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        Write-Progress -Stage "side-open-after" -Side $Side
        $location = Find-UniqueTableShape -Document $doc
        Write-Progress -Stage "side-table-found" -Side $Side
        $page = $doc.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $table = $shape.Table

        $neighbor = Get-OppositeNeighbor -Side $Side
        $targetBefore = Get-BorderRgb -Table $table -RowIndex 2 -ColumnIndex 2 -Side $Side
        $neighborBefore = Get-BorderRgb -Table $table -RowIndex $neighbor.row -ColumnIndex $neighbor.col -Side $neighbor.side
        Write-Progress -Stage "side-border-read-before" -Side $Side
        Set-BorderRgb -Table $table -Side $Side -Rgb $MutationRgb
        Write-Progress -Stage "side-border-mutated" -Side $Side
        $doc.Save()
        Write-Progress -Stage "side-save-complete" -Side $Side
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
    $page2 = $null
    $shape2 = $null
    $table2 = $null
    $targetAfter = 0L
    $neighborAfter = 0L
    try {
        Write-Progress -Stage "side-reopen-before" -Side $Side
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        Write-Progress -Stage "side-reopen-after" -Side $Side
        $location2 = Find-UniqueTableShape -Document $doc2
        Write-Progress -Stage "side-reopen-table-found" -Side $Side
        $page2 = $doc2.Pages.Item([int]$location2.page_index)
        $shape2 = $page2.Shapes.Item([int]$location2.shape_index)
        $table2 = $shape2.Table
        $neighbor2 = Get-OppositeNeighbor -Side $Side
        $targetAfter = Get-BorderRgb -Table $table2 -RowIndex 2 -ColumnIndex 2 -Side $Side
        $neighborAfter = Get-BorderRgb -Table $table2 -RowIndex $neighbor2.row -ColumnIndex $neighbor2.col -Side $neighbor2.side
        Write-Progress -Stage "side-border-read-after" -Side $Side
    }
    finally {
        Release-Com $table2
        Release-Com $shape2
        Release-Com $page2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $diffPath = Join-Path $armDir "raw-diff.json"
    Write-Progress -Stage "side-raw-diff-before" -Side $Side
    & $Tool diff $AllEnabledPath $output $diffPath
    if ($LASTEXITCODE -ne 0) {
        throw "raw carrier diff failed for $Side"
    }
    $rawDiff = Get-Content -LiteralPath $diffPath -Raw | ConvertFrom-Json
    Write-Progress -Stage "side-raw-diff-after" -Side $Side

    return [ordered]@{
        side = $Side
        target_border_changed = [bool]($targetBefore -ne $targetAfter)
        neighbor_opposite_border_changed = [bool]($neighborBefore -ne $neighborAfter)
        raw = [ordered]@{
            matched_carrier_count = [int]$rawDiff.matched_carrier_count
            changed_carrier_count = [int]$rawDiff.changed_carrier_count
            removed_carrier_count = [int]$rawDiff.removed_carrier_count
            added_carrier_count = [int]$rawDiff.added_carrier_count
            ambiguous_changed_group_count = [int]$rawDiff.ambiguous_changed_group_count
            changed_anchor_signature_histogram = $rawDiff.changed_anchor_signature_histogram
            changed_property_id_histogram = $rawDiff.changed_property_id_histogram
            removed_anchor_signature_histogram = $rawDiff.removed_anchor_signature_histogram
            added_anchor_signature_histogram = $rawDiff.added_anchor_signature_histogram
        }
    }
}

$tool = Build-OracleTool
Write-Progress -Stage "oracle-tool-ready"
$allEnabled = Join-Path $privateDir "all-enabled.pub"
Write-Progress -Stage "fixture-create-before"
New-AllEnabledFixture -Path $allEnabled
Write-Progress -Stage "fixture-create-after"

$profilePath = Join-Path $privateDir "all-enabled-profile.json"
Write-Progress -Stage "profile-before"
& $tool profile $allEnabled $profilePath
if ($LASTEXITCODE -ne 0) {
    throw "all-enabled raw profile failed"
}
$profile = Get-Content -LiteralPath $profilePath -Raw | ConvertFrom-Json
Write-Progress -Stage "profile-after"

$arms = @()
foreach ($side in @("top", "right", "bottom", "left")) {
    $arms += Invoke-SideArm -Side $side -AllEnabledPath $allEnabled -Tool $tool
}

$allCausal = $true
$allRawLocalized = $true
foreach ($arm in $arms) {
    if (-not [bool]$arm.target_border_changed -or -not [bool]$arm.neighbor_opposite_border_changed) {
        $allCausal = $false
    }
    if (
        ([int]$arm.raw.changed_carrier_count -le 0 -and
            [int]$arm.raw.added_carrier_count -le 0 -and
            [int]$arm.raw.removed_carrier_count -le 0) -or
        [int]$arm.raw.ambiguous_changed_group_count -ne 0
    ) {
        $allRawLocalized = $false
    }
}

$verdict = if ($allCausal -and $allRawLocalized) {
    "border-side-carrier-delta-localized"
} else {
    "inconclusive"
}

$result = [ordered]@{
    schema = "chaptera.publisher-table-border-carrier-map.v1"
    experiment_id = $ExpectedExperiment
    generated_fixture = [ordered]@{
        rows = 4
        columns = 4
        autoformat = "CheckbookRegister"
        all_enabled_border_carrier_count = [int]$profile.border_carrier_count
        duplicate_anchor_group_count = [int]$profile.duplicate_anchor_group_count
        anchor_signature_histogram = $profile.anchor_signature_histogram
    }
    mutation = [ordered]@{
        target_cell = "R2C2"
        property = "CellBorder.Color.RGB"
        same_mutation_value_all_arms = $true
    }
    arms = $arms
    guards = [ordered]@{
        target_side_changed_all_arms = [bool]$allCausal
        raw_carrier_delta_localized_all_arms = [bool]$allRawLocalized
    }
    verdict = $verdict
    authority_boundary = "Each arm changes exactly one COM Cell border side on the same all-enabled 4x4 AutoFormat fixture. Raw comparison uses full ClientAnchor values only as private matching keys and emits only field-ID signatures/property-ID deltas. Geometry/proximity is not used to assign side semantics."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "table-border-carrier-map-01.json")
@(
    "experiment=$ExpectedExperiment",
    "all_enabled_carriers=$($profile.border_carrier_count)",
    "target_side_changed_all=$allCausal",
    "raw_localized_all=$allRawLocalized",
    "verdict=$verdict"
) | Set-Content -LiteralPath (Join-Path $logDir "table-border-carrier-map-01.txt") -Encoding ASCII


$weightArms = @()
foreach ($deltaPoints in @(1.0, 3.0)) {
    $weightArms += Invoke-WeightArm -DeltaPoints $deltaPoints -AllEnabledPath $allEnabled -Tool $tool
}

$weightCausal = $true
$weightRawLocalized = $true
foreach ($arm in $weightArms) {
    if (-not [bool]$arm.requested_weight_persisted -or -not [bool]$arm.color_unchanged) {
        $weightCausal = $false
    }
    $rawDeltaCount =
        [int]$arm.raw.changed_carrier_count +
        [int]$arm.raw.removed_carrier_count +
        [int]$arm.raw.added_carrier_count
    if ($rawDeltaCount -ne 1 -or [int]$arm.raw.ambiguous_changed_group_count -ne 0) {
        $weightRawLocalized = $false
    }
}

$weightVerdict = if ($weightCausal -and $weightRawLocalized) {
    "border-weight-carrier-delta-localized"
} else {
    "inconclusive"
}

$weightResult = [ordered]@{
    schema = "chaptera.publisher-table-border-weight-map.v1"
    experiment_id = $ExpectedExperiment
    mutation = [ordered]@{
        target_cell = "R2C2"
        side = "top"
        property = "CellBorder.Weight"
        delta_points = @(1.0, 3.0)
    }
    arms = $weightArms
    guards = [ordered]@{
        requested_weight_persisted_all_arms = [bool]$weightCausal
        raw_carrier_delta_localized_all_arms = [bool]$weightRawLocalized
    }
    verdict = $weightVerdict
    authority_boundary = "Each weight arm changes only COM CellBorder.Weight on the same proven R2C2 top/shared segment. Color must remain unchanged. Private raw deltas are retained only to derive the exact persisted source-value law if the carrier does not map to one already-documented scalar."
}
Write-PubJson -Value $weightResult -Path (Join-Path $analysisDir "table-border-weight-map-01.json")

if ($verdict -eq "inconclusive") {
    throw "TABLE border carrier map remained inconclusive; inspect private arm evidence."
}

if ($weightVerdict -eq "inconclusive") {
    throw "TABLE border weight map remained inconclusive; inspect private weight-arm evidence."
}
