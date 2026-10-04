param(
    [string]$OutputRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$ExperimentId = "STRUCT-TXN-ESCHER-01"
$Schema = "chaptera.struct-txn-escher-01.portable.v1"

$ExpectedSeedSha256 = "6fefdef46b87c767150878dc384549cb2d2ec2ac54de25f8ddb3a5628301107e"
$ExpectedBaseSha256 = "905bf75b00c0ff8680f61a20d5df4d843d753d3544a233fd237dc5a38a0a0599"
$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$EmuPerPoint = 12700

$RuntimeModule = Join-Path $Root "runtime\PubRuntime.psm1"
$StructuralTool = Join-Path $Root "runtime\structural_base_manifest.exe"
$SeedPath = Join-Path $Root "fixtures\Sample.pub"
$BundleManifestPath = Join-Path $Root "bundle-manifest.json"

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $OutputRoot = Join-Path $Root "out\$stamp"
} elseif (-not [IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $Root $OutputRoot
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Json {
    param([Parameter(Mandatory=$true)]$Value,[Parameter(Mandatory=$true)][string]$Path)
    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $Value | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $Path -Encoding UTF8
}

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

function Verify-BundleManifest {
    if (-not (Test-Path -LiteralPath $BundleManifestPath -PathType Leaf)) {
        throw "bundle-manifest.json is missing."
    }
    $manifest = Get-Content -LiteralPath $BundleManifestPath -Raw | ConvertFrom-Json
    if ([string]$manifest.schema -ne "chaptera.portable-bundle-manifest.v1") {
        throw "Unexpected bundle manifest schema."
    }
    foreach ($file in @($manifest.files)) {
        $candidate = Join-Path $Root ([string]$file.path)
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "Bundle file missing: $($file.path)"
        }
        $item = Get-Item -LiteralPath $candidate
        if ([long]$item.Length -ne [long]$file.size) {
            throw "Bundle file size mismatch: $($file.path)"
        }
        $actual = Get-Sha256 $candidate
        if ($actual -ne ([string]$file.sha256).ToLowerInvariant()) {
            throw "Bundle file SHA-256 mismatch: $($file.path)"
        }
    }
    return $manifest
}

function Get-ShapeInventory {
    param([Parameter(Mandatory=$true)]$Document)
    $pages = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            $ids = @()
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    $ids += [long]$shape.ID
                } finally {
                    Release-Com $shape
                }
            }
            $pages += [ordered]@{
                page_index = $pageIndex
                page_id = [long]$page.PageID
                shape_count = [int]$page.Shapes.Count
                shape_ids = @($ids)
            }
        } finally {
            Release-Com $page
        }
    }
    return [ordered]@{
        page_count = [int]$Document.Pages.Count
        pages = @($pages)
    }
}

function Assert-Publisher2019 {
    Import-Module $RuntimeModule -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) {
        throw "Publisher COM is unavailable."
    }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) {
        throw "Publisher Version mismatch: expected $ExpectedPublisherVersion"
    }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) {
        throw "Publisher Build mismatch: expected $ExpectedPublisherBuild"
    }
    if ($publisher.path.state -ne "value") {
        throw "Publisher executable directory is unavailable."
    }

    $publisherExe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
    if (-not (Test-Path -LiteralPath $publisherExe -PathType Leaf)) {
        throw "MSPUB.EXE is missing at the COM-reported path."
    }
    $fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($publisherExe).FileVersion
    if ([string]$fileVersion -ne $ExpectedPublisherFileVersion) {
        throw "MSPUB.EXE file version mismatch: expected $ExpectedPublisherFileVersion got $fileVersion"
    }
    $exeSha = Get-Sha256 $publisherExe
    if ($exeSha -ne $ExpectedPublisherExeSha256) {
        throw "MSPUB.EXE SHA mismatch: expected $ExpectedPublisherExeSha256 got $exeSha"
    }

    return [ordered]@{
        version = [string]$publisher.version.value
        build = [string]$publisher.build.value
        file_version = [string]$fileVersion
        exe_sha256 = [string]$exeSha
    }
}

function Invoke-StructuralManifest {
    param(
        [Parameter(Mandatory=$true)][string]$PubPath,
        [Parameter(Mandatory=$true)][string]$JsonPath
    )
    & $StructuralTool $PubPath $JsonPath
    if ($LASTEXITCODE -ne 0) {
        throw "structural_base_manifest.exe failed with exit code $LASTEXITCODE"
    }
    if (-not (Test-Path -LiteralPath $JsonPath -PathType Leaf)) {
        throw "structural_base_manifest.exe did not create $JsonPath"
    }
    return Get-Content -LiteralPath $JsonPath -Raw | ConvertFrom-Json
}

function Find-ShapeById {
    param([Parameter(Mandatory=$true)]$Shapes,[Parameter(Mandatory=$true)][long]$ShapeId)
    for ($i = 1; $i -le [int]$Shapes.Count; $i++) {
        $shape = $null
        try {
            $shape = $Shapes.Item($i)
            if ([long]$shape.ID -eq $ShapeId) {
                return $shape
            }
        } catch {
            Release-Com $shape
            throw
        }
        Release-Com $shape
    }
    return $null
}

function Get-SafeValue {
    param([Parameter(Mandatory=$true)][scriptblock]$Getter)
    try { return & $Getter } catch { return $null }
}

function Shape-Snapshot {
    param([Parameter(Mandatory=$true)]$Shape)
    return [ordered]@{
        id = [long]$Shape.ID
        name = [string]$Shape.Name
        type = [int]$Shape.Type
        auto_shape_type = Get-SafeValue { [int]$Shape.AutoShapeType }
        left_pt = [double]$Shape.Left
        top_pt = [double]$Shape.Top
        width_pt = [double]$Shape.Width
        height_pt = [double]$Shape.Height
        fill_rgb = Get-SafeValue { [int]$Shape.Fill.ForeColor.RGB }
        line_visible = Get-SafeValue { [int]$Shape.Line.Visible }
        line_weight_pt = Get-SafeValue { [double]$Shape.Line.Weight }
        z_order_position = Get-SafeValue { [int]$Shape.ZOrderPosition }
    }
}

function To-Map($Rows,[string]$KeyName) {
    $map = @{}
    foreach ($row in @($Rows)) {
        $key = [string]$row.$KeyName
        $map[$key] = $row
    }
    return $map
}

function Json-Compact($Value) {
    return ($Value | ConvertTo-Json -Depth 64 -Compress)
}

function Diff-References($Before,$After) {
    $beforeMap = To-Map $Before "seq_num"
    $afterMap = To-Map $After "seq_num"
    $keys = @(@($beforeMap.Keys) + @($afterMap.Keys) | Sort-Object {[int]$_} -Unique)
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
    return [ordered]@{ added=$added; removed=$removed; changed=$changed }
}

function Diff-Streams($Before,$After) {
    $beforeMap = To-Map $Before "path"
    $afterMap = To-Map $After "path"
    $keys = @(@($beforeMap.Keys) + @($afterMap.Keys) | Sort-Object -Unique)
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

if (Test-Path -LiteralPath $OutputRoot) {
    $existing = @(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction SilentlyContinue)
    if ($existing.Count -ne 0) {
        throw "OutputRoot is not empty: $OutputRoot"
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$analysisDir = Join-Path $OutputRoot "analysis"
$logsDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private"
New-Item -ItemType Directory -Force -Path $analysisDir,$logsDir,$privateDir | Out-Null

$bundleManifest = Verify-BundleManifest
if (-not (Test-Path -LiteralPath $SeedPath -PathType Leaf)) { throw "Bundled Sample.pub is missing." }
if ((Get-Sha256 $SeedPath) -ne $ExpectedSeedSha256) {
    throw "Bundled Sample.pub SHA mismatch."
}
if (-not (Test-Path -LiteralPath $StructuralTool -PathType Leaf)) { throw "Bundled structural helper is missing." }
if (-not (Test-Path -LiteralPath $RuntimeModule -PathType Leaf)) { throw "Bundled Publisher runtime module is missing." }

$publisher = Assert-Publisher2019

# Materialize the exact previously admitted T370 base locally.
$basePub = Join-Path $privateDir "t370-base.pub"
Copy-Item -LiteralPath $SeedPath -Destination $basePub -Force
$seedRecord = [ordered]@{ sha256 = Get-Sha256 $basePub; size = (Get-Item -LiteralPath $basePub).Length }

$app = $null
$doc = $null
$beforeInventory = $null
$afterInventory = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($basePub, $false, $false)
    $beforeInventory = Get-ShapeInventory -Document $doc
    $doc.Save()
    $afterInventory = Get-ShapeInventory -Document $doc
} finally {
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$baseSha = Get-Sha256 $basePub
if ($baseSha -ne $ExpectedBaseSha256) {
    throw "Publisher one-save did not materialize the exact admitted T370 base. Expected $ExpectedBaseSha256 got $baseSha"
}
if ((Json-Compact $beforeInventory) -ne (Json-Compact $afterInventory)) {
    throw "T370 prerequisite save changed bounded page/shape inventory."
}

$app = $null
$doc = $null
$reopenInventory = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($basePub, $true, $false)
    $reopenInventory = Get-ShapeInventory -Document $doc
} finally {
    Close-Document $doc
    Close-PubPublisherApplication $app
}
if ((Json-Compact $beforeInventory) -ne (Json-Compact $reopenInventory)) {
    throw "T370 prerequisite fresh reopen changed bounded page/shape inventory."
}

$beforeManifestPath = Join-Path $privateDir "structural-before.json"
$beforeStructural = Invoke-StructuralManifest -PubPath $basePub -JsonPath $beforeManifestPath
if ([string]$beforeStructural.source_sha256 -ne $ExpectedBaseSha256) {
    throw "Pre-mutation structural helper did not bind the exact T370 base."
}

$workingPub = Join-Path $privateDir "working-addshape.pub"
Copy-Item -LiteralPath $basePub -Destination $workingPub -Force
$workingBeforeSha = Get-Sha256 $workingPub

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
    $doc = $app.Open($workingPub, $false, $false)
    if ([int]$doc.Pages.Count -lt 1) { throw "T370 base exposes no page." }
    $page = $doc.Pages.Item(1)
    $pageId = [long]$page.PageID
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
} finally {
    Release-Com $newShape
    Release-Com $shapes
    Release-Com $page
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$workingSavedSha = Get-Sha256 $workingPub
if ($workingSavedSha -eq $workingBeforeSha) {
    throw "AddShape mutation produced no persisted byte change."
}

$app = $null
$doc = $null
$page = $null
$shapes = $null
$found = $null
$reopened = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($workingPub, $true, $false)
    $page = $doc.Pages.Item(1)
    $shapes = $page.Shapes
    if ([int]$shapes.Count -ne ($shapeCountBefore + 1)) {
        throw "Fresh reopen did not preserve exactly one new shape."
    }
    $found = Find-ShapeById -Shapes $shapes -ShapeId ([long]$created.id)
    if ($null -eq $found) {
        throw "Fresh reopen cannot find created shape by Shape.ID."
    }
    $reopened = Shape-Snapshot -Shape $found
} finally {
    Release-Com $found
    Release-Com $shapes
    Release-Com $page
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$afterManifestPath = Join-Path $privateDir "structural-after.json"
$afterStructural = Invoke-StructuralManifest -PubPath $workingPub -JsonPath $afterManifestPath
if ([string]$afterStructural.source_sha256 -ne $workingSavedSha) {
    throw "Post-mutation structural helper source SHA mismatch."
}

if ((Get-Sha256 $SeedPath) -ne $ExpectedSeedSha256) {
    throw "Bundled seed changed during the experiment."
}
if ((Get-Sha256 $basePub) -ne $ExpectedBaseSha256) {
    throw "Exact T370 base changed after AddShape arm."
}

$referenceDiff = Diff-References $beforeStructural.contents_references $afterStructural.contents_references
$streamDiff = Diff-Streams $beforeStructural.manifest.streams $afterStructural.manifest.streams

$beforeCandidateKeys = @{}
foreach ($candidate in @($beforeStructural.manifest.candidates)) {
    $beforeCandidateKeys["$($candidate.contents_seq_num):$($candidate.officeart_spid)"] = $true
}
$addedCandidates = @()
foreach ($candidate in @($afterStructural.manifest.candidates)) {
    $key = "$($candidate.contents_seq_num):$($candidate.officeart_spid)"
    if (-not $beforeCandidateKeys.ContainsKey($key)) { $addedCandidates += $candidate }
}

$idMatches = @($afterStructural.manifest.candidates | Where-Object { [long]$_.contents_seq_num -eq [long]$created.id })
$target = $null
$selection = "none"
if ($idMatches.Count -eq 1) {
    $target = $idMatches[0]
    $selection = "contents_seq_matches_com_shape_id"
} elseif ($addedCandidates.Count -eq 1) {
    $target = $addedCandidates[0]
    $selection = "unique_new_structural_candidate"
}

$targetSummary = $null
if ($null -ne $target) {
    $escherShape = $target.escher_shape
    $fopt = [ordered]@{}
    foreach ($property in $escherShape.PSObject.Properties) {
        if ($property.Name -match "(?i)fopt|option") {
            $fopt[$property.Name] = $property.Value
        }
    }
    $targetSummary = [ordered]@{
        selection = $selection
        com_shape_id = [long]$created.id
        contents_seq_num = [long]$target.contents_seq_num
        com_shape_id_equals_contents_seq = ([long]$target.contents_seq_num -eq [long]$created.id)
        officeart_spid = [long]$target.officeart_spid
        officeart_shape_type = [int]$target.officeart_shape_type
        bounds_emu = $target.bounds_emu
        contents_fields = @($target.contents_chunk.fields | ForEach-Object {
            [ordered]@{
                id = [int]$_.id
                block_type = [int]$_.block_type
                raw_tag = @($_.raw_tag)
                source = $_.source
            }
        })
        escher_fsp = $escherShape.fsp
        escher_client_anchor = $escherShape.client_anchor
        escher_client_data = $escherShape.client_data
        escher_fopt_observations = $fopt
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
} elseif (-not [bool]$targetSummary.com_shape_id_equals_contents_seq) {
    "structural_candidate_found_but_shape_id_join_differs"
} else {
    "bounded_creation_materialization_captured"
}

$environment = [ordered]@{
    schema = "chaptera.struct-txn-escher.environment.v1"
    experiment_id = $ExperimentId
    os_version = [System.Environment]::OSVersion.VersionString
    powershell_version = $PSVersionTable.PSVersion.ToString()
    publisher = $publisher
    bundle_manifest_sha256 = Get-Sha256 $BundleManifestPath
    structural_helper_sha256 = Get-Sha256 $StructuralTool
}

$result = [ordered]@{
    schema = $Schema
    experiment_id = $ExperimentId
    prerequisite = [ordered]@{
        seed_sha256 = $ExpectedSeedSha256
        seed_size = [long]$seedRecord.size
        materialized_t370_base_sha256 = $baseSha
        exact_t370_base = ($baseSha -eq $ExpectedBaseSha256)
        inventory_equal_after_save = ((Json-Compact $beforeInventory) -eq (Json-Compact $afterInventory))
        inventory_equal_after_fresh_reopen = ((Json-Compact $beforeInventory) -eq (Json-Compact $reopenInventory))
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
        saved_sha256 = $workingSavedSha
        saved_size = (Get-Item -LiteralPath $workingPub).Length
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
        exact_t370_base_materialized_locally = $true
        one_new_ordinary_non_text_shape = $true
        save_close_fresh_reopen = $true
        bundled_seed_mutated = $false
        generated_pub_uploaded = $false
        full_structural_manifests_uploaded = $false
        geometry_rediscovery_claimed = $false
        universal_allocator_claimed = $false
        effect_carrier_semantics_promoted = $false
    }
}

$environmentPath = Join-Path $OutputRoot "environment.json"
$analysisPath = Join-Path $analysisDir "struct-txn-escher-01.json"
$logPath = Join-Path $logsDir "struct-txn-escher-01.txt"
Write-Json $environment $environmentPath
Write-Json $result $analysisPath

@(
    "experiment=$ExperimentId",
    "seed_sha256=$ExpectedSeedSha256",
    "base_sha256=$baseSha",
    "created_shape_id=$($created.id)",
    "saved_sha256=$workingSavedSha",
    "before_slots=$($beforeStructural.contents_slot_count)",
    "after_slots=$($afterStructural.contents_slot_count)",
    "added_refs=$(@($referenceDiff.added).Count)",
    "removed_refs=$(@($referenceDiff.removed).Count)",
    "changed_refs=$(@($referenceDiff.changed).Count)",
    "added_candidates=$(@($addedCandidates).Count)",
    "shape_id_candidate_matches=$($idMatches.Count)",
    "verdict=$verdict"
) | Set-Content -LiteralPath $logPath -Encoding ASCII

$publicFiles = @($environmentPath,$analysisPath,$logPath)
$evidence = [ordered]@{
    schema = "chaptera.struct-txn-escher.evidence.v1"
    experiment_id = $ExperimentId
    generated_utc = [DateTime]::UtcNow.ToString("o")
    files = @($publicFiles | ForEach-Object {
        $item = Get-Item -LiteralPath $_
        [ordered]@{
            path = $item.Name
            size = [long]$item.Length
            sha256 = Get-Sha256 $item.FullName
        }
    })
}
$evidencePath = Join-Path $OutputRoot "evidence-manifest.json"
Write-Json $evidence $evidencePath

$returnStamp = Get-Date -Format "yyyyMMdd-HHmmss"
$returnRoot = Join-Path $OutputRoot "return"
New-Item -ItemType Directory -Force -Path $returnRoot | Out-Null
Copy-Item -LiteralPath $environmentPath -Destination $returnRoot
Copy-Item -LiteralPath $analysisPath -Destination $returnRoot
Copy-Item -LiteralPath $logPath -Destination $returnRoot
Copy-Item -LiteralPath $evidencePath -Destination $returnRoot

$returnZip = Join-Path $Root "RETURN-TO-CHAT-STRUCT-TXN-ESCHER-$returnStamp.zip"
if (Test-Path -LiteralPath $returnZip) { Remove-Item -LiteralPath $returnZip -Force }
Compress-Archive -Path (Join-Path $returnRoot "*") -DestinationPath $returnZip -CompressionLevel Optimal

Write-Host ""
Write-Host "STRUCT-TXN-ESCHER-01 completed."
Write-Host "Verdict: $verdict"
Write-Host "Return ZIP: $returnZip"
Write-Host "Upload that RETURN-TO-CHAT ZIP back to ChatGPT."
