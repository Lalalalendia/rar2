param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExperimentId = "STRUCT-TXN-ESCHER-01"
$Schema = "chaptera.struct-txn-escher-01.v1"
$ExpectedBaseSha256 = "905bf75b00c0ff8680f61a20d5df4d843d753d3544a233fd237dc5a38a0a0599"
$ExpectedBaseReceiptSchema = "chaptera.modern-base-pin.v1"
$ExpectedBaseExperiment = "MODERN-BASE-PIN-01"
$EmuPerPoint = 12700

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExperimentId) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/struct-txn-escher-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Close-Document($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Close() } catch {}
    Release-Com $Document
}

function Resolve-ExactBase {
    $configured = [string]$env:PUB_RESEARCH_FIXTURE
    if ([string]::IsNullOrWhiteSpace($configured)) {
        throw "PUB_RESEARCH_FIXTURE must point to the exact T370 Publisher2019-normalized base."
    }
    if (-not (Test-Path -LiteralPath $configured -PathType Leaf)) {
        throw "Configured T370 base does not exist."
    }
    $resolved = (Resolve-Path -LiteralPath $configured).Path
    $record = Get-PubFileRecord -Path $resolved
    if ([string]$record.sha256 -ne $ExpectedBaseSha256) {
        throw "T370 base SHA mismatch: expected $ExpectedBaseSha256 got $($record.sha256)"
    }
    return $resolved
}

function Resolve-ExactBaseReceipt {
    $configured = [string]$env:PUB_RESEARCH_MODERN_BASE_RECEIPT
    if ([string]::IsNullOrWhiteSpace($configured)) {
        throw "PUB_RESEARCH_MODERN_BASE_RECEIPT must point to analysis/modern-base-pin-01.json for the exact T370 base."
    }
    if (-not (Test-Path -LiteralPath $configured -PathType Leaf)) {
        throw "Configured T370 receipt does not exist."
    }
    $resolved = (Resolve-Path -LiteralPath $configured).Path
    $receipt = Get-Content -LiteralPath $resolved -Raw | ConvertFrom-Json
    if ([string]$receipt.schema -ne $ExpectedBaseReceiptSchema) {
        throw "Unexpected T370 receipt schema: $($receipt.schema)"
    }
    if ([string]$receipt.experiment_id -ne $ExpectedBaseExperiment) {
        throw "Unexpected T370 experiment identity: $($receipt.experiment_id)"
    }
    if ([string]$receipt.native_lineage.post_save_sha256 -ne $ExpectedBaseSha256) {
        throw "T370 receipt does not bind the exact admitted structural base."
    }
    return [pscustomobject]@{
        path = $resolved
        sha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
        receipt = $receipt
    }
}

function Build-StructuralBaseTool {
    $prebuilt = [string]$env:PUB_RESEARCH_STRUCTURAL_BASE_TOOL
    if (-not [string]::IsNullOrWhiteSpace($prebuilt)) {
        if (-not (Test-Path -LiteralPath $prebuilt -PathType Leaf)) {
            throw "Configured PUB_RESEARCH_STRUCTURAL_BASE_TOOL does not exist."
        }
        $expected = [string]$env:PUB_RESEARCH_STRUCTURAL_BASE_TOOL_SHA256
        if ([string]::IsNullOrWhiteSpace($expected)) {
            throw "PUB_RESEARCH_STRUCTURAL_BASE_TOOL_SHA256 is required for a prebuilt helper."
        }
        $resolved = (Resolve-Path -LiteralPath $prebuilt).Path
        $actual = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $expected.ToLowerInvariant()) {
            throw "Prebuilt structural-base helper SHA-256 mismatch."
        }
        return [pscustomobject]@{
            path = $resolved
            sha256 = $actual
            source = "prebuilt"
            cargo_version = $null
            cargo_lock_sha256 = $null
            cargo_lock_origin = $null
        }
    }

    $manifestPath = Join-Path $repoRoot "vendor/producer-a/Cargo.toml"
    $workspaceRoot = Split-Path -Parent $manifestPath
    $lockPath = Join-Path $workspaceRoot "Cargo.lock"
    $targetDir = Join-Path $privateDir "cargo-target"
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

    $cargoVersion = (& cargo --version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "cargo --version failed with exit code $LASTEXITCODE"
    }

    $lockExisted = Test-Path -LiteralPath $lockPath -PathType Leaf
    $generatedLock = $false
    try {
        if (-not $lockExisted) {
            Push-Location $repoRoot
            try {
                & cargo generate-lockfile --offline --manifest-path $manifestPath
                if ($LASTEXITCODE -ne 0) {
                    throw "offline Cargo.lock generation failed with exit code $LASTEXITCODE"
                }
            }
            finally {
                Pop-Location
            }
            if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
                throw "Cargo.lock missing after offline generation"
            }
            $generatedLock = $true
        }

        $lockSha = (Get-FileHash -LiteralPath $lockPath -Algorithm SHA256).Hash.ToLowerInvariant()

        Push-Location $repoRoot
        try {
            & cargo build --locked --offline --release --target-dir $targetDir --manifest-path $manifestPath -p pub-reader --bin structural_base_manifest
            if ($LASTEXITCODE -ne 0) {
                throw "structural_base_manifest offline build failed with exit code $LASTEXITCODE"
            }
        }
        finally {
            Pop-Location
        }

        $exe = Join-Path $targetDir "release/structural_base_manifest.exe"
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
            throw "structural_base_manifest.exe missing after offline private-target build"
        }
        return [pscustomobject]@{
            path = $exe
            sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
            source = "offline-cargo-build"
            cargo_version = $cargoVersion
            cargo_lock_sha256 = $lockSha
            cargo_lock_origin = $(if ($generatedLock) { "generated-offline-for-run" } else { "preexisting" })
        }
    }
    finally {
        if ($generatedLock -and (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
            Remove-Item -LiteralPath $lockPath -Force
        }
    }
}

function Invoke-StructuralBase {
    param(
        [Parameter(Mandatory = $true)][string]$Tool,
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Output
    )
    & $Tool $Source $Output
    if ($LASTEXITCODE -ne 0) {
        throw "structural_base_manifest failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $Output -Raw | ConvertFrom-Json
}

function Find-ShapeById {
    param(
        [Parameter(Mandatory = $true)]$Shapes,
        [Parameter(Mandatory = $true)][long]$ShapeId
    )
    for ($i = 1; $i -le [int]$Shapes.Count; $i++) {
        $shape = $null
        try {
            $shape = $Shapes.Item($i)
            if ([long]$shape.ID -eq $ShapeId) {
                return $shape
            }
        }
        catch {
            Release-Com $shape
            throw
        }
        Release-Com $shape
    }
    return $null
}

function Shape-Snapshot {
    param([Parameter(Mandatory = $true)]$Shape)
    return [ordered]@{
        id = [long]$Shape.ID
        name = [string]$Shape.Name
        type = [int]$Shape.Type
        auto_shape_type = Get-PubSafeValue { [int]$Shape.AutoShapeType } "Shape.AutoShapeType"
        left_pt = [double]$Shape.Left
        top_pt = [double]$Shape.Top
        width_pt = [double]$Shape.Width
        height_pt = [double]$Shape.Height
        fill_rgb = Get-PubSafeValue { [int]$Shape.Fill.ForeColor.RGB } "Shape.Fill.ForeColor.RGB"
        line_visible = Get-PubSafeValue { [int]$Shape.Line.Visible } "Shape.Line.Visible"
        line_weight_pt = Get-PubSafeValue { [double]$Shape.Line.Weight } "Shape.Line.Weight"
        z_order_position = Get-PubSafeValue { [int]$Shape.ZOrderPosition } "Shape.ZOrderPosition"
    }
}

function Ref-Map($References) {
    $map = @{}
    foreach ($reference in @($References)) {
        $map[[string]$reference.seq_num] = $reference
    }
    return $map
}

function Stream-Map($Streams) {
    $map = @{}
    foreach ($stream in @($Streams)) {
        $map[[string]$stream.path] = $stream
    }
    return $map
}

function Json-Compact($Value) {
    return ($Value | ConvertTo-Json -Depth 64 -Compress)
}

function Diff-References($Before, $After) {
    $beforeMap = Ref-Map $Before
    $afterMap = Ref-Map $After
    $keys = @($beforeMap.Keys + $afterMap.Keys | Sort-Object {[int]$_} -Unique)
    $added = @()
    $removed = @()
    $changed = @()
    foreach ($key in $keys) {
        if (-not $beforeMap.ContainsKey($key)) {
            $added += $afterMap[$key]
            continue
        }
        if (-not $afterMap.ContainsKey($key)) {
            $removed += $beforeMap[$key]
            continue
        }
        if ((Json-Compact $beforeMap[$key]) -ne (Json-Compact $afterMap[$key])) {
            $changed += [ordered]@{
                seq_num = [int]$key
                before = $beforeMap[$key]
                after = $afterMap[$key]
            }
        }
    }
    return [ordered]@{
        added = $added
        removed = $removed
        changed = $changed
    }
}

function Diff-Streams($Before, $After) {
    $beforeMap = Stream-Map $Before
    $afterMap = Stream-Map $After
    $keys = @($beforeMap.Keys + $afterMap.Keys | Sort-Object -Unique)
    $changed = @()
    foreach ($key in $keys) {
        $b = if ($beforeMap.ContainsKey($key)) { $beforeMap[$key] } else { $null }
        $a = if ($afterMap.ContainsKey($key)) { $afterMap[$key] } else { $null }
        if ($null -eq $b -or $null -eq $a -or [string]$b.sha256 -ne [string]$a.sha256 -or [long]$b.len -ne [long]$a.len) {
            $changed += [ordered]@{
                path = $key
                before_len = if ($null -ne $b) { [long]$b.len } else { $null }
                after_len = if ($null -ne $a) { [long]$a.len } else { $null }
                before_sha256 = if ($null -ne $b) { [string]$b.sha256 } else { $null }
                after_sha256 = if ($null -ne $a) { [string]$a.sha256 } else { $null }
            }
        }
    }
    return $changed
}

function Safe-Contents-Field-Summary($Fields) {
    $rows = @()
    foreach ($field in @($Fields)) {
        $scalar = $null
        if ($null -ne $field.body -and $field.body.PSObject.Properties.Name -contains "value") {
            $value = $field.body.value
            if ($value -is [ValueType]) { $scalar = $value }
        }
        $rows += [ordered]@{
            id = [int]$field.id
            block_type = [int]$field.block_type
            raw_tag = @($field.raw_tag)
            scalar_value = $scalar
            source = $field.source
        }
    }
    return $rows
}

$sourcePath = Resolve-ExactBase
$baseReceiptInfo = Resolve-ExactBaseReceipt
$sourceBefore = Get-PubFileRecord -Path $sourcePath
$toolInfo = Build-StructuralBaseTool

$environment = Get-PubEnvironmentManifest -SnapshotId $ExperimentId -RequirePublisher
$environment.machine = $null
$environment.default_printer = $null
Write-PubJson -Value $environment -Path (Join-Path $OutputRoot "environment.json")

$working = Join-Path $privateDir "working.pub"
[void](Copy-PubBoundFile -Source $sourcePath -Destination $working)
$workingBefore = Get-PubFileRecord -Path $working

$beforeManifestPath = Join-Path $privateDir "structural-before.json"
$afterManifestPath = Join-Path $privateDir "structural-after.json"
$beforeStructural = Invoke-StructuralBase -Tool $toolInfo.path -Source $working -Output $beforeManifestPath
if ([string]$beforeStructural.source_sha256 -ne $ExpectedBaseSha256) {
    throw "Pre-mutation structural manifest is not bound to the exact T370 base."
}

# Intentionally non-default geometry in points. Office msoShapeRectangle = 1.
$left = 79.0
$top = 113.0
$width = 181.0
$height = 73.0
$fillRgb = 0x0000A5FF
$lineWeight = 2.25

$app = $null
$doc = $null
$page = $null
$shapes = $null
$newShape = $null
$created = $null
$pageId = $null
$shapeCountBefore = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($working, $false, $false)
    if ([int]$doc.Pages.Count -lt 1) {
        throw "Exact T370 base exposes no ordinary page for AddShape."
    }
    $page = $doc.Pages.Item(1)
    $pageId = Get-PubSafeValue { [long]$page.PageID } "Page.PageID"
    $shapes = $page.Shapes
    $shapeCountBefore = [int]$shapes.Count

    $newShape = $shapes.AddShape(1, $left, $top, $width, $height)
    try { $newShape.Name = "CHAPTERA_T351_RECT" } catch {}
    $newShape.Fill.Solid()
    $newShape.Fill.ForeColor.RGB = $fillRgb
    $newShape.Line.Visible = -1
    $newShape.Line.Weight = $lineWeight
    $created = Shape-Snapshot -Shape $newShape

    $doc.Save()
}
finally {
    Release-Com $newShape
    Release-Com $shapes
    Release-Com $page
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$workingSaved = Get-PubFileRecord -Path $working
if ([string]$workingSaved.sha256 -eq [string]$workingBefore.sha256) {
    throw "AddShape arm did not persist any byte mutation."
}

$reopened = $null
$app = $null
$doc = $null
$page = $null
$shapes = $null
$found = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($working, $true, $false)
    $page = $doc.Pages.Item(1)
    $shapes = $page.Shapes
    if ([int]$shapes.Count -ne ($shapeCountBefore + 1)) {
        throw "Fresh reopen did not preserve exactly one added page-local shape."
    }
    $found = Find-ShapeById -Shapes $shapes -ShapeId ([long]$created.id)
    if ($null -eq $found) {
        throw "Fresh reopen cannot find the created shape by COM Shape.ID."
    }
    $reopened = Shape-Snapshot -Shape $found
}
finally {
    Release-Com $found
    Release-Com $shapes
    Release-Com $page
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$afterStructural = Invoke-StructuralBase -Tool $toolInfo.path -Source $working -Output $afterManifestPath
if ([string]$afterStructural.source_sha256 -ne [string]$workingSaved.sha256) {
    throw "Post-mutation structural manifest source SHA mismatch."
}

$sourceAfter = Get-PubFileRecord -Path $sourcePath
if ([string]$sourceAfter.sha256 -ne [string]$sourceBefore.sha256 -or [long]$sourceAfter.size -ne [long]$sourceBefore.size) {
    throw "Original exact T370 base changed during T351."
}

$referenceDiff = Diff-References -Before $beforeStructural.contents_references -After $afterStructural.contents_references
$streamDiff = Diff-Streams -Before $beforeStructural.manifest.streams -After $afterStructural.manifest.streams

$beforeCandidateKeys = @{}
foreach ($candidate in @($beforeStructural.manifest.candidates)) {
    $beforeCandidateKeys["$($candidate.contents_seq_num):$($candidate.officeart_spid)"] = $true
}
$addedCandidates = @()
foreach ($candidate in @($afterStructural.manifest.candidates)) {
    $key = "$($candidate.contents_seq_num):$($candidate.officeart_spid)"
    if (-not $beforeCandidateKeys.ContainsKey($key)) {
        $addedCandidates += $candidate
    }
}

$idMatches = @($afterStructural.manifest.candidates | Where-Object { [long]$_.contents_seq_num -eq [long]$created.id })
$target = $null
$targetSelection = "none"
if ($idMatches.Count -eq 1) {
    $target = $idMatches[0]
    $targetSelection = "contents_seq_matches_com_shape_id"
}
elseif ($addedCandidates.Count -eq 1) {
    $target = $addedCandidates[0]
    $targetSelection = "unique_new_structural_candidate"
}

$targetSummary = $null
if ($null -ne $target) {
    $escherShape = $target.escher_shape
    $targetSummary = [ordered]@{
        selection = $targetSelection
        com_shape_id = [long]$created.id
        page_id = $target.page_id
        node_id = $target.node_id
        contents_seq_num = [long]$target.contents_seq_num
        com_shape_id_equals_contents_seq = ([long]$target.contents_seq_num -eq [long]$created.id)
        officeart_spid = [long]$target.officeart_spid
        officeart_shape_type = [int]$target.officeart_shape_type
        bounds_emu = $target.bounds_emu
        contents_fields = Safe-Contents-Field-Summary $target.contents_chunk.fields
        escher_fsp = $escherShape.fsp
        escher_client_anchor = $escherShape.client_anchor
        escher_client_data = $escherShape.client_data
        escher_observation_keys = @($escherShape.PSObject.Properties.Name | Sort-Object)
    }
}

$relatedRefs = @()
foreach ($reference in @($referenceDiff.added)) {
    if ([int]$reference.seq_num -eq [int]$created.id -or @($reference.parent_seq_nums) -contains [int]$created.id) {
        $relatedRefs += $reference
    }
}
foreach ($change in @($referenceDiff.changed)) {
    if (
        [int]$change.seq_num -eq [int]$created.id -or
        @($change.before.parent_seq_nums) -contains [int]$created.id -or
        @($change.after.parent_seq_nums) -contains [int]$created.id
    ) {
        $relatedRefs += $change.after
    }
}

$verdict = if ($null -eq $target) {
    "structural_target_not_uniquely_joined"
}
elseif (-not [bool]$targetSummary.com_shape_id_equals_contents_seq) {
    "structural_candidate_found_but_shape_id_join_differs"
}
else {
    "bounded_creation_materialization_captured"
}

$result = [ordered]@{
    schema = $Schema
    experiment_id = $ExperimentId
    source = [ordered]@{
        admitted_base_sha256 = $ExpectedBaseSha256
        admitted_base_size = [long]$sourceBefore.size
        original_unchanged = $true
        modern_base_receipt_sha256 = [string]$baseReceiptInfo.sha256
        modern_base_receipt_schema = [string]$baseReceiptInfo.receipt.schema
    }
    publisher = [ordered]@{
        version = $environment.publisher.version
        build = $environment.publisher.build
        executable_sha256 = $environment.publisher.sha256
    }
    tooling = [ordered]@{
        structural_base_helper_source = [string]$toolInfo.source
        structural_base_helper_sha256 = [string]$toolInfo.sha256
        cargo_version = $toolInfo.cargo_version
        cargo_lock_sha256 = $toolInfo.cargo_lock_sha256
        cargo_lock_origin = $toolInfo.cargo_lock_origin
        private_before_manifest_sha256 = (Get-FileHash -LiteralPath $beforeManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
        private_after_manifest_sha256 = (Get-FileHash -LiteralPath $afterManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    mutation = [ordered]@{
        operation = "Publisher Page.Shapes.AddShape(msoShapeRectangle)"
        page_id = $pageId
        shape_count_before = $shapeCountBefore
        created = $created
        fresh_reopen = $reopened
        expected_geometry_emu = [ordered]@{
            left = [long]($left * $EmuPerPoint)
            top = [long]($top * $EmuPerPoint)
            width = [long]($width * $EmuPerPoint)
            height = [long]($height * $EmuPerPoint)
        }
        authored_fill_rgb = $fillRgb
        authored_line_weight_pt = $lineWeight
        saved_sha256 = [string]$workingSaved.sha256
        saved_size = [long]$workingSaved.size
    }
    structural_diff = [ordered]@{
        before_slot_count = [int]$beforeStructural.contents_slot_count
        after_slot_count = [int]$afterStructural.contents_slot_count
        added_references = @($referenceDiff.added)
        removed_references = @($referenceDiff.removed)
        changed_references = @($referenceDiff.changed)
        related_references = @($relatedRefs)
        changed_streams = @($streamDiff)
        added_candidate_count = @($addedCandidates).Count
        added_candidate_keys = @($addedCandidates | ForEach-Object { "$($_.contents_seq_num):$($_.officeart_spid)" })
        com_shape_id_candidate_match_count = $idMatches.Count
        target = $targetSummary
    }
    verdict = $verdict
    claims = [ordered]@{
        exact_t370_base_bound = $true
        exact_t370_receipt_bound = $true
        one_new_ordinary_non_text_shape = $true
        save_close_fresh_reopen = $true
        source_original_mutated = $false
        generated_pub_uploaded = $false
        private_full_manifests_uploaded = $false
        geometry_rediscovery_claimed = $false
        universal_allocator_claimed = $false
        effect_carrier_semantics_promoted = $false
        effect_carrier_instantiation_requires_semantic_review_of_private_target = $true
    }
}
Write-PubJson -Value $result -Path (Join-Path $analysisDir "struct-txn-escher-01.json")

@(
    "experiment=$ExperimentId",
    "base_sha256=$ExpectedBaseSha256",
    "base_receipt_sha256=$($baseReceiptInfo.sha256)",
    "created_shape_id=$($created.id)",
    "created_shape_name=$($created.name)",
    "saved_sha256=$($workingSaved.sha256)",
    "before_slots=$($beforeStructural.contents_slot_count)",
    "after_slots=$($afterStructural.contents_slot_count)",
    "added_refs=$(@($referenceDiff.added).Count)",
    "removed_refs=$(@($referenceDiff.removed).Count)",
    "changed_refs=$(@($referenceDiff.changed).Count)",
    "added_candidates=$(@($addedCandidates).Count)",
    "shape_id_candidate_matches=$($idMatches.Count)",
    "verdict=$verdict",
    "original_source_unchanged=true"
) | Set-Content -LiteralPath (Join-Path $logDir "struct-txn-escher-01.txt") -Encoding ASCII
