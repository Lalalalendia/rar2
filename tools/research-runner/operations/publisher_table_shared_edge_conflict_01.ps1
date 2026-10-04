param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "TABLE-SHARED-EDGE-CONFLICT-01"
$PbFilePublication = 1
$PbTableAutoFormatCheckbookRegister = 0
$RedRgb = 255
$BlueRgb = 16711680

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/table-shared-edge-conflict-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

$analysisPath = Join-Path $analysisDir "table-shared-edge-conflict-01.json"
$logPath = Join-Path $logDir "table-shared-edge-conflict-01.txt"
$progressPath = Join-Path $analysisDir "table-shared-edge-conflict-01-progress.jsonl"

function Write-Progress {
    param(
        [Parameter(Mandatory = $true)][string]$Stage,
        [string]$Arm = ""
    )
    $record = [ordered]@{
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        stage = $Stage
        arm = $Arm
    }
    ($record | ConvertTo-Json -Compress) | Add-Content -LiteralPath $progressPath -Encoding UTF8
}

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
        throw "Expected exactly one table shape; found $($matches.Count)."
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

function New-Fixture {
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
        "right" { return $Cell.BorderRight }
        "left" { return $Cell.BorderLeft }
        default { throw "Unsupported side: $Side" }
    }
}

function Get-BorderRgbAt {
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

function Set-BorderRgbAt {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][int]$RowIndex,
        [Parameter(Mandatory = $true)][int]$ColumnIndex,
        [Parameter(Mandatory = $true)][string]$Side,
        [Parameter(Mandatory = $true)][long]$Rgb
    )

    $cell = $null
    $border = $null
    $color = $null
    try {
        $cell = Get-Cell -Table $Table -RowIndex $RowIndex -ColumnIndex $ColumnIndex
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

function Capture-EdgeState {
    param([Parameter(Mandatory = $true)]$Table)

    return [ordered]@{
        a = [ordered]@{
            accessor = "R2C2.Right"
            rgb = Get-BorderRgbAt -Table $Table -RowIndex 2 -ColumnIndex 2 -Side "right"
        }
        b = [ordered]@{
            accessor = "R2C3.Left"
            rgb = Get-BorderRgbAt -Table $Table -RowIndex 2 -ColumnIndex 3 -Side "left"
        }
    }
}

function Set-AccessorRgb {
    param(
        [Parameter(Mandatory = $true)]$Table,
        [Parameter(Mandatory = $true)][string]$Accessor,
        [Parameter(Mandatory = $true)][long]$Rgb
    )

    switch ($Accessor) {
        "A" {
            Set-BorderRgbAt -Table $Table -RowIndex 2 -ColumnIndex 2 -Side "right" -Rgb $Rgb
        }
        "B" {
            Set-BorderRgbAt -Table $Table -RowIndex 2 -ColumnIndex 3 -Side "left" -Rgb $Rgb
        }
        default { throw "Unsupported accessor: $Accessor" }
    }
}

function Get-Sha256Text {
    param([Parameter(Mandatory = $true)][string]$Text)

    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Get-PrivateGroupSummary {
    param([Parameter(Mandatory = $true)][string]$Path)

    $doc = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    $groups = @($doc.groups)
    $summaries = @()
    foreach ($group in $groups) {
        $keyJson = $group.key | ConvertTo-Json -Depth 32 -Compress
        $summaries += [ordered]@{
            key_fingerprint_sha256 = Get-Sha256Text -Text $keyJson
            before_state_count = @($group.before_states).Count
            after_state_count = @($group.after_states).Count
        }
    }

    return [ordered]@{
        group_count = $groups.Count
        groups = $summaries
    }
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][array]$Actions,
        [Parameter(Mandatory = $true)][string]$BaselinePath,
        [Parameter(Mandatory = $true)][string]$Tool
    )

    Write-Progress -Stage "arm-start" -Arm $Name
    $armDir = Join-Path $privateDir $Name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    Copy-Item -LiteralPath $BaselinePath -Destination $working -Force

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $table = $null
    $before = $null
    $steps = @()

    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $location = Find-UniqueTableShape -Document $doc
        $page = $doc.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $table = $shape.Table

        $before = Capture-EdgeState -Table $table
        foreach ($action in $Actions) {
            Set-AccessorRgb -Table $table -Accessor ([string]$action.accessor) -Rgb ([long]$action.rgb)
            $steps += [ordered]@{
                accessor = [string]$action.accessor
                requested_rgb = [long]$action.rgb
                readback = Capture-EdgeState -Table $table
            }
        }

        $doc.Save()
    }
    finally {
        Release-Com $table
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $reopenApp = $null
    $reopenDoc = $null
    $reopenPage = $null
    $reopenShape = $null
    $reopenTable = $null
    $reopened = $null
    try {
        $reopenApp = New-PubPublisherApplication
        $reopenDoc = $reopenApp.Open($working, $true, $false)
        $location2 = Find-UniqueTableShape -Document $reopenDoc
        $reopenPage = $reopenDoc.Pages.Item([int]$location2.page_index)
        $reopenShape = $reopenPage.Shapes.Item([int]$location2.shape_index)
        $reopenTable = $reopenShape.Table
        $reopened = Capture-EdgeState -Table $reopenTable
    }
    finally {
        Release-Com $reopenTable
        Release-Com $reopenShape
        Release-Com $reopenPage
        Close-Document $reopenDoc
        Close-PubPublisherApplication $reopenApp
    }

    $publicDiffPath = Join-Path $armDir "raw-diff.json"
    $privateDiffPath = Join-Path $armDir "raw-private-diff.json"

    & $Tool diff $BaselinePath $working $publicDiffPath
    if ($LASTEXITCODE -ne 0) {
        throw "Public TABLE border diff failed for $Name"
    }

    & $Tool diff-private $BaselinePath $working $privateDiffPath
    if ($LASTEXITCODE -ne 0) {
        throw "Private TABLE border diff failed for $Name"
    }

    $rawDiff = Get-Content -LiteralPath $publicDiffPath -Raw | ConvertFrom-Json
    $privateSummary = Get-PrivateGroupSummary -Path $privateDiffPath
    $file = Get-PubFileRecord -Path $working

    Write-Progress -Stage "arm-complete" -Arm $Name
    return [ordered]@{
        arm = $Name
        requested_actions = $Actions
        before = $before
        steps = $steps
        reopened = $reopened
        output = [ordered]@{
            size = $file.size
            sha256 = $file.sha256
        }
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
        private_key_summary = $privateSummary
    }
}

Write-Progress -Stage "operation-started"
$tool = Build-OracleTool
Write-Progress -Stage "oracle-ready"

$baseline = Join-Path $privateDir "fixture-all-enabled.pub"
New-Fixture -Path $baseline
$baselineFile = Get-PubFileRecord -Path $baseline

$arms = @(
    [pscustomobject]@{ name = "C0"; actions = @() },
    [pscustomobject]@{ name = "C1"; actions = @([pscustomobject]@{ accessor = "A"; rgb = $RedRgb }) },
    [pscustomobject]@{ name = "C2"; actions = @([pscustomobject]@{ accessor = "B"; rgb = $BlueRgb }) },
    [pscustomobject]@{
        name = "C3"
        actions = @(
            [pscustomobject]@{ accessor = "A"; rgb = $RedRgb },
            [pscustomobject]@{ accessor = "B"; rgb = $BlueRgb }
        )
    },
    [pscustomobject]@{
        name = "C4"
        actions = @(
            [pscustomobject]@{ accessor = "B"; rgb = $BlueRgb },
            [pscustomobject]@{ accessor = "A"; rgb = $RedRgb }
        )
    }
)

$results = @()
foreach ($arm in $arms) {
    $results += Invoke-Arm -Name ([string]$arm.name) -Actions @($arm.actions) -BaselinePath $baseline -Tool $tool
}

$byName = @{}
foreach ($arm in $results) {
    $byName[[string]$arm.arm] = $arm
}

$control = $byName["C0"]
$controlClean = [bool](
    [int]$control.raw.changed_carrier_count -eq 0 -and
    [int]$control.raw.removed_carrier_count -eq 0 -and
    [int]$control.raw.added_carrier_count -eq 0 -and
    [int]$control.raw.ambiguous_changed_group_count -eq 0 -and
    [int]$control.private_key_summary.group_count -eq 0
)

$mutationNames = @("C1", "C2", "C3", "C4")
$allUnambiguous = $true
$oneGroupPerMutation = $true
$oneCarrierPerMutation = $true
$keyFingerprints = @()
$mirrorReadback = $true
foreach ($name in $mutationNames) {
    $arm = $byName[$name]
    if ([int]$arm.raw.ambiguous_changed_group_count -ne 0) { $allUnambiguous = $false }
    if ([int]$arm.private_key_summary.group_count -ne 1) {
        $oneGroupPerMutation = $false
        continue
    }
    $group = @($arm.private_key_summary.groups)[0]
    $keyFingerprints += [string]$group.key_fingerprint_sha256
    if ([int]$group.after_state_count -ne 1) { $oneCarrierPerMutation = $false }
    if ([long]$arm.reopened.a.rgb -ne [long]$arm.reopened.b.rgb) { $mirrorReadback = $false }
}

$uniqueKeyFingerprints = @($keyFingerprints | Sort-Object -Unique)
$sameCarrierKey = [bool](
    $oneGroupPerMutation -and
    $uniqueKeyFingerprints.Count -eq 1
)

$singleAccessorControls = [bool](
    [long]$byName["C1"].reopened.a.rgb -eq $RedRgb -and
    [long]$byName["C1"].reopened.b.rgb -eq $RedRgb -and
    [long]$byName["C2"].reopened.a.rgb -eq $BlueRgb -and
    [long]$byName["C2"].reopened.b.rgb -eq $BlueRgb
)

$c3Final = [long]$byName["C3"].reopened.a.rgb
$c4Final = [long]$byName["C4"].reopened.a.rgb

$verdict = "not_evaluable"
if (-not $controlClean) {
    $verdict = "control_not_clean"
}
elseif (-not $allUnambiguous) {
    $verdict = "raw_diff_ambiguous"
}
elseif (-not $oneGroupPerMutation -or -not $sameCarrierKey -or -not $singleAccessorControls) {
    $verdict = "canonical_edge_identity_not_proven"
}
elseif (-not $mirrorReadback) {
    $verdict = "separate_or_conflicting_side_state"
}
elseif (-not $oneCarrierPerMutation) {
    $verdict = "multiple_carriers_same_edge"
}
elseif ($c3Final -eq $BlueRgb -and $c4Final -eq $RedRgb) {
    $verdict = "single_edge_last_mutation_wins"
}
elseif ($c3Final -eq $c4Final) {
    $verdict = "deterministic_winner_independent_of_order"
}
else {
    $verdict = "single_edge_aliasing_conflict_rule_unknown"
}

$receipt = [ordered]@{
    schema = "chaptera.table-shared-edge-conflict.v1"
    experiment_id = $ExpectedExperiment
    semantic_target = [ordered]@{
        table = "generated-4x4-checkbook-register"
        accessor_a = "R2C2.Right"
        accessor_b = "R2C3.Left"
        expected_edge = [ordered]@{
            orientation = "vertical"
            column_boundary = 2
            row_boundary_start = 1
            row_boundary_end = 2
        }
    }
    requested_values = [ordered]@{
        red_rgb = $RedRgb
        blue_rgb = $BlueRgb
    }
    baseline = [ordered]@{
        size = $baselineFile.size
        sha256 = $baselineFile.sha256
    }
    invariants = [ordered]@{
        control_clean = $controlClean
        raw_diffs_unambiguous = $allUnambiguous
        one_changed_edge_group_per_mutation = $oneGroupPerMutation
        same_changed_carrier_key_across_mutations = $sameCarrierKey
        one_persisted_carrier_for_changed_edge = $oneCarrierPerMutation
        opposite_accessors_mirror_after_reopen = $mirrorReadback
        single_accessor_controls_roundtrip = $singleAccessorControls
    }
    changed_carrier_key_fingerprints = $uniqueKeyFingerprints
    arms = $results
    verdict = $verdict
    interpretation = switch ($verdict) {
        "single_edge_last_mutation_wins" {
            "A and B are bounded aliases of one canonical TABLE edge; write order selects the final scalar value."
        }
        "deterministic_winner_independent_of_order" {
            "A and B join to one edge, but a setter/accessor-specific precedence rule is indicated."
        }
        "multiple_carriers_same_edge" {
            "The one-edge/one-carrier persistence model is falsified in this bounded conflict state."
        }
        "separate_or_conflicting_side_state" {
            "Opposite accessors no longer mirror after reopen; alias semantics are falsified or incomplete."
        }
        default {
            "Evidence is insufficient for a stronger conflict-write law; keep the semantic result NotEvaluable."
        }
    }
    authority_boundary = [ordered]@{
        scope = "Publisher16/build12527 generated simple unmerged 4x4 CheckbookRegister table; one internal vertical edge; color writes only"
        merged_cells = "not_proven"
        other_versions = "not_proven"
        border_weight_conflict = "not_executed"
        arbitrary_scalar_composition = "not_granted_unless_verdict_requires_it"
    }
}

Write-PubJson -Value $receipt -Path $analysisPath

$logLines = @(
    "TABLE-SHARED-EDGE-CONFLICT-01",
    "baseline_sha256=$($baselineFile.sha256)",
    "control_clean=$controlClean",
    "same_changed_carrier_key=$sameCarrierKey",
    "one_persisted_carrier=$oneCarrierPerMutation",
    "opposite_accessors_mirror=$mirrorReadback",
    "C3_final_rgb=$c3Final",
    "C4_final_rgb=$c4Final",
    "verdict=$verdict",
    "analysis=$analysisPath"
)
$logLines | Set-Content -LiteralPath $logPath -Encoding UTF8

Write-Progress -Stage "operation-complete"
Get-Content -LiteralPath $analysisPath -Raw
