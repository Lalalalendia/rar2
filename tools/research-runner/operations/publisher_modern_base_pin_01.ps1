param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "MODERN-BASE-PIN-01"
$ExpectedFixtureName = "Sample.pub"
$ExpectedFixtureSha256 = "6fefdef46b87c767150878dc384549cb2d2ec2ac54de25f8ddb3a5628301107e"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/modern-base-pin-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

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
        throw "Set PUB_RESEARCH_FIXTURE to exact Sample.pub or PUB_RESEARCH_FIXTURE_ROOT to a local corpus root."
    }

    $matches = @(
        Get-ChildItem -LiteralPath $root -Recurse -File -Filter $ExpectedFixtureName -ErrorAction SilentlyContinue |
            Where-Object {
                (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -eq $ExpectedFixtureSha256
            }
    )
    if ($matches.Count -ne 1) {
        throw "Expected exactly one pinned Sample.pub under PUB_RESEARCH_FIXTURE_ROOT; found $($matches.Count)."
    }
    return $matches[0].FullName
}

function Release-Com($Value) {
    if ($null -ne $Value -and [System.Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Close-Document($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Close() } catch {}
    Release-Com $Document
}

function Get-DocumentInventory {
    param([Parameter(Mandatory = $true)]$Document)

    $pages = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            $shapeIds = @()
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    $shapeIds += [long]$shape.ID
                }
                finally {
                    Release-Com $shape
                }
            }

            $pageId = Get-PubSafeValue { [long]$page.PageID } "Page.PageID"
            $pages += [ordered]@{
                page_index = $pageIndex
                page_id = $pageId
                shape_count = [int]$page.Shapes.Count
                shape_ids = @($shapeIds)
            }
        }
        finally {
            Release-Com $page
        }
    }

    return [ordered]@{
        page_count = [int]$Document.Pages.Count
        pages = @($pages)
    }
}

function Inventory-Equal {
    param(
        [Parameter(Mandatory = $true)]$Left,
        [Parameter(Mandatory = $true)]$Right
    )
    $leftJson = $Left | ConvertTo-Json -Depth 16 -Compress
    $rightJson = $Right | ConvertTo-Json -Depth 16 -Compress
    return $leftJson -eq $rightJson
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
        return $resolved
    }

    $targetDir = Join-Path $privateDir "cargo-target"
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

    Push-Location $repoRoot
    try {
        & cargo build --locked --offline --release --target-dir $targetDir --manifest-path "vendor/producer-a/Cargo.toml" -p pub-reader --bin structural_base_manifest
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
    return $exe
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

$fixturePath = Resolve-ExactFixture
$seed = Get-PubFileRecord -Path $fixturePath
if ([string]$seed.sha256 -ne $ExpectedFixtureSha256) {
    throw "Pinned Sample.pub source identity mismatch."
}

$working = Join-Path $privateDir "working.pub"
$binding = Copy-PubBoundFile -Source $fixturePath -Destination $working
$preSave = Get-PubFileRecord -Path $working

$app = $null
$doc = $null
$beforeInventory = $null
$afterSaveInventory = $null
$publisherIdentity = $null
try {
    $app = New-PubPublisherApplication
    $publisherIdentity = [ordered]@{
        version = [string]$app.Version
        build = [string]$app.Build
    }
    $doc = $app.Open($working, $false, $false)
    $beforeInventory = Get-DocumentInventory -Document $doc

    $doc.Save()
    $afterSaveInventory = Get-DocumentInventory -Document $doc
}
finally {
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$postSave = Get-PubFileRecord -Path $working
if ([string]$postSave.sha256 -eq [string]$preSave.sha256) {
    throw "Native one-save output is byte-identical to pinned source; expected Publisher normalization did not materialize."
}

$app2 = $null
$doc2 = $null
$reopenInventory = $null
try {
    $app2 = New-PubPublisherApplication
    $doc2 = $app2.Open($working, $true, $false)
    $reopenInventory = Get-DocumentInventory -Document $doc2
}
finally {
    Close-Document $doc2
    Close-PubPublisherApplication $app2
}

$beforeAfterEqual = Inventory-Equal -Left $beforeInventory -Right $afterSaveInventory
$beforeReopenEqual = Inventory-Equal -Left $beforeInventory -Right $reopenInventory
if (-not $beforeAfterEqual -or -not $beforeReopenEqual) {
    throw "No-semantic-edit one-save changed the bounded COM page/shape identity inventory."
}

$tool = Build-StructuralBaseTool
$privateReceipt = Join-Path $privateDir "structural-base.json"
$structural = Invoke-StructuralBase -Tool $tool -Source $working -Output $privateReceipt

if ([string]$structural.schema -ne "chaptera.modern-structural-base/v1") {
    throw "Unexpected structural-base receipt schema."
}
if ([string]$structural.source_sha256 -ne [string]$postSave.sha256) {
    throw "Structural-base receipt source SHA does not match native one-save output."
}
if ([int]$structural.candidate_count -lt 1) {
    throw "Native one-save output exposes no bounded structural-base candidate."
}
if (-not [bool]$structural.selected_target.contents_anchor_extent_exact) {
    throw "Selected target failed Contents 0xAA/0xAB versus Escher anchor extent invariant."
}

$streamReceipts = @(
    $structural.manifest.streams | ForEach-Object {
        [ordered]@{
            path = [string]$_.path
            len = [long]$_.len
            sha256 = [string]$_.sha256
        }
    }
)

$result = [ordered]@{
    schema = "chaptera.modern-base-pin.v1"
    experiment_id = $ExpectedExperiment
    publisher = $publisherIdentity
    seed = [ordered]@{
        sha256 = [string]$seed.sha256
        size = [long]$seed.size
    }
    native_lineage = [ordered]@{
        save_count = 1
        save_operation = "Document.Save"
        fresh_read_only_reopen = $true
        pre_save_sha256 = [string]$preSave.sha256
        post_save_sha256 = [string]$postSave.sha256
        post_save_size = [long]$postSave.size
        bounded_inventory_equal_after_save = [bool]$beforeAfterEqual
        bounded_inventory_equal_after_reopen = [bool]$beforeReopenEqual
    }
    structural_base = [ordered]@{
        schema = [string]$structural.structural_schema
        source_sha256 = [string]$structural.source_sha256
        stream_count = [int]$structural.stream_count
        streams = $streamReceipts
        candidate_count = [int]$structural.candidate_count
        selected_target = $structural.selected_target
    }
    authority_boundary = "This receipt pins one exact Publisher2019/build12527 no-semantic-edit save lineage of the SHA-bound Apache POI Sample.pub seed. It does not claim global no-op byte determinism. The selected target is the lowest-seq bounded ordinary page child with unique U32 Contents SHAPE_WIDTH/SHAPE_HEIGHT fields 0xAA/0xAB whose values exactly match XE-XS / YE-YS from one unique signed Escher ClientAnchor, with the same Contents seqNum carried by local Escher Publisher shape identity."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "modern-base-pin-01.json")
@(
    "experiment=$ExpectedExperiment",
    "seed_sha256=$($result.seed.sha256)",
    "post_save_sha256=$($result.native_lineage.post_save_sha256)",
    "post_save_size=$($result.native_lineage.post_save_size)",
    "stream_count=$($result.structural_base.stream_count)",
    "candidate_count=$($result.structural_base.candidate_count)",
    "selected_contents_seq=$($result.structural_base.selected_target.contents_seq_num)",
    "selected_officeart_spid=$($result.structural_base.selected_target.officeart_spid)",
    "contents_width_emu=$($result.structural_base.selected_target.contents_width_emu)",
    "contents_height_emu=$($result.structural_base.selected_target.contents_height_emu)",
    "anchor_width_emu=$($result.structural_base.selected_target.anchor_width_emu)",
    "anchor_height_emu=$($result.structural_base.selected_target.anchor_height_emu)",
    "contents_anchor_extent_exact=$($result.structural_base.selected_target.contents_anchor_extent_exact)",
    "inventory_equal_after_save=$beforeAfterEqual",
    "inventory_equal_after_reopen=$beforeReopenEqual"
) | Set-Content -LiteralPath (Join-Path $logDir "modern-base-pin-01.txt") -Encoding ASCII
