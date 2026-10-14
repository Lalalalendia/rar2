param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "PAGE-BACKGROUND-AUTH-READ-01"
$PbFilePublication = 1
$PbFixedFormatTypePDF = 2
$PbIntentStandard = 2
$MsoShapeRectangle = 1
$MsoTrue = -1
$MsoFalse = 0
# Office COLORREF (0x00BBGGRR): distinctive warm red/orange.
$BackgroundRgb = 0x003366CC

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/page-background-auth-read-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null
$logPath = Join-Path $logDir "page-background-auth-read-01.txt"
$fingerprintTool = Join-Path $repoRoot "tools/pub_operation_algebra_fingerprint.py"

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

function Write-JsonNoBom {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $json = $Value | ConvertTo-Json -Depth 32
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, $utf8)
}

function Get-BackgroundSnapshot {
    param([Parameter(Mandatory = $true)]$Page)

    $background = $null
    $fill = $null
    $fore = $null
    $back = $null
    $row = [ordered]@{
        access_ok = $false
        access_error = $null
        exists = $null
        fill_type = $null
        fill_visible = $null
        fore_rgb = $null
        back_rgb = $null
        transparency = $null
    }
    try {
        $background = $Page.Background
        $row.access_ok = $true
        $row.exists = [bool]$background.Exists
        if ($row.exists) {
            $fill = $background.Fill
            try { $row.fill_type = [int]$fill.Type } catch {}
            try { $row.fill_visible = [int]$fill.Visible } catch {}
            try { $row.transparency = [double]$fill.Transparency } catch {}
            try {
                $fore = $fill.ForeColor
                $row.fore_rgb = [long]$fore.RGB
            } catch {}
            try {
                $back = $fill.BackColor
                $row.back_rgb = [long]$back.RGB
            } catch {}
        }
    }
    catch {
        $row.access_error = $_.Exception.Message
    }
    finally {
        Release-Com $back
        Release-Com $fore
        Release-Com $fill
        Release-Com $background
    }
    return $row
}

function Get-PageSnapshot {
    param([Parameter(Mandatory = $true)]$Document)

    if ([int]$Document.Pages.Count -ne 1) {
        throw "Read-side PageBackground oracle expects exactly one publication page."
    }
    $page = $null
    try {
        $page = $Document.Pages.Item(1)
        return [ordered]@{
            page_count = [int]$Document.Pages.Count
            page_id = [long]$page.PageID
            page_number = [int]$page.PageNumber
            page_type = [int]$page.PageType
            width_pt = [double]$page.Width
            height_pt = [double]$page.Height
            shapes_count = [int]$page.Shapes.Count
            background = Get-BackgroundSnapshot -Page $page
        }
    }
    finally {
        Release-Com $page
    }
}

function New-BaselinePublication {
    param([Parameter(Mandatory = $true)][string]$Path)

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $fill = $null
    $color = $null
    $line = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Documents.Add()
        if ([int]$doc.Pages.Count -ne 1) {
            throw "New Publisher publication does not have exactly one page."
        }
        $page = $doc.Pages.Item(1)
        $shape = $page.Shapes.AddShape($MsoShapeRectangle, 72.0, 72.0, 72.0, 72.0)
        $fill = $shape.Fill
        try { $fill.Visible = $MsoTrue } catch {}
        $fill.Solid()
        $color = $fill.ForeColor
        $color.RGB = 0x00FFFFFF
        $line = $shape.Line
        $line.Visible = $MsoFalse
        $doc.SaveAs($Path, $PbFilePublication, $false)
    }
    finally {
        Release-Com $line
        Release-Com $color
        Release-Com $fill
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Mutate-CreateBackground {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePub,
        [Parameter(Mandatory = $true)][string]$OutputPub
    )

    Copy-Item -LiteralPath $SourcePub -Destination $OutputPub -Force
    $app = $null
    $doc = $null
    $page = $null
    $background = $null
    $fill = $null
    $color = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($OutputPub, $false, $false)
        $page = $doc.Pages.Item(1)
        $beforeShapes = [int]$page.Shapes.Count
        $background = $page.Background
        if ([bool]$background.Exists) {
            throw "Baseline unexpectedly already has a PageBackground."
        }
        $background.Create()
        if (-not [bool]$background.Exists) {
            throw "PageBackground.Create returned without Exists=true."
        }
        $fill = $background.Fill
        try { $fill.Visible = $MsoTrue } catch {}
        $fill.Solid()
        $color = $fill.ForeColor
        $color.RGB = $BackgroundRgb
        if ([int]$page.Shapes.Count -ne $beforeShapes) {
            throw "PageBackground.Create changed Page.Shapes.Count."
        }
        $doc.Save()
    }
    finally {
        Release-Com $color
        Release-Com $fill
        Release-Com $background
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Mutate-DeleteBackground {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePub,
        [Parameter(Mandatory = $true)][string]$OutputPub
    )

    Copy-Item -LiteralPath $SourcePub -Destination $OutputPub -Force
    $app = $null
    $doc = $null
    $page = $null
    $background = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($OutputPub, $false, $false)
        $page = $doc.Pages.Item(1)
        $beforeShapes = [int]$page.Shapes.Count
        $background = $page.Background
        if (-not [bool]$background.Exists) {
            throw "Delete arm requires an existing PageBackground."
        }
        $background.Delete()
        if ([bool]$background.Exists) {
            throw "PageBackground.Delete returned without Exists=false."
        }
        if ([int]$page.Shapes.Count -ne $beforeShapes) {
            throw "PageBackground.Delete changed Page.Shapes.Count."
        }
        $doc.Save()
    }
    finally {
        Release-Com $background
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Snapshot-Stage {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$PubPath
    )

    $stageDir = Join-Path $privateDir $Name
    New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
    $semanticPath = Join-Path $stageDir "semantic.json"
    $pdfPath = Join-Path $stageDir "render.pdf"
    $fingerprintPath = Join-Path $stageDir "fingerprint.json"

    $app = $null
    $doc = $null
    $semantic = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($PubPath, $true, $false)
        $semantic = Get-PageSnapshot -Document $doc
        Write-JsonNoBom -Value $semantic -Path $semanticPath
        $doc.ExportAsFixedFormat($PbFixedFormatTypePDF, $pdfPath, $PbIntentStandard)
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    & python $fingerprintTool --pub $PubPath --pdf $pdfPath --semantic $semanticPath --out $fingerprintPath | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Fingerprint helper failed for stage $Name."
    }
    $fingerprint = Get-Content -LiteralPath $fingerprintPath -Raw | ConvertFrom-Json
    return [ordered]@{
        name = $Name
        semantic = $semantic
        fingerprint = $fingerprint
    }
}

function Stream-Map {
    param($Stage)
    $map = @{}
    foreach ($stream in @($Stage.fingerprint.persistence_fingerprint.streams)) {
        $map[[string]$stream.name] = [ordered]@{
            size = [int64]$stream.size
            sha256 = [string]$stream.sha256
        }
    }
    return $map
}

function Compare-Stages {
    param(
        [Parameter(Mandatory = $true)]$Left,
        [Parameter(Mandatory = $true)]$Right
    )

    $leftMap = Stream-Map -Stage $Left
    $rightMap = Stream-Map -Stage $Right
    $names = @($leftMap.Keys + $rightMap.Keys | Sort-Object -Unique)
    $changed = @()
    foreach ($name in $names) {
        $leftRow = if ($leftMap.ContainsKey($name)) { $leftMap[$name] } else { $null }
        $rightRow = if ($rightMap.ContainsKey($name)) { $rightMap[$name] } else { $null }
        if ($null -eq $leftRow -or $null -eq $rightRow -or
            [int64]$leftRow.size -ne [int64]$rightRow.size -or
            [string]$leftRow.sha256 -ne [string]$rightRow.sha256) {
            $changed += [ordered]@{
                name = $name
                left = $leftRow
                right = $rightRow
            }
        }
    }

    return [ordered]@{
        semantic_equal = ([string]$Left.fingerprint.semantic_fingerprint.sha256 -eq [string]$Right.fingerprint.semantic_fingerprint.sha256)
        persistence_equal = ([string]$Left.fingerprint.persistence_fingerprint.sha256 -eq [string]$Right.fingerprint.persistence_fingerprint.sha256)
        render_equal = ([string]$Left.fingerprint.render_fingerprint.sha256 -eq [string]$Right.fingerprint.render_fingerprint.sha256)
        changed_stream_count = $changed.Count
        changed_streams = @($changed)
    }
}

$baselinePub = Join-Path $privateDir "baseline.pub"
$createdPub = Join-Path $privateDir "background-created.pub"
$deletedPub = Join-Path $privateDir "background-deleted.pub"

New-BaselinePublication -Path $baselinePub
$baseline = Snapshot-Stage -Name "baseline" -PubPath $baselinePub

Mutate-CreateBackground -SourcePub $baselinePub -OutputPub $createdPub
$created = Snapshot-Stage -Name "created" -PubPath $createdPub

Mutate-DeleteBackground -SourcePub $createdPub -OutputPub $deletedPub
$deleted = Snapshot-Stage -Name "deleted" -PubPath $deletedPub

if ([bool]$baseline.semantic.background.exists) {
    throw "Baseline reopened with PageBackground.Exists=true."
}
if (-not [bool]$created.semantic.background.exists) {
    throw "Created arm did not persist PageBackground.Exists=true."
}
if ([long]$created.semantic.background.fore_rgb -ne [long]$BackgroundRgb) {
    throw "Created arm foreground RGB did not persist exactly."
}
if ([bool]$deleted.semantic.background.exists) {
    throw "Deleted arm did not persist PageBackground.Exists=false."
}
if ([int]$baseline.semantic.shapes_count -ne [int]$created.semantic.shapes_count -or
    [int]$created.semantic.shapes_count -ne [int]$deleted.semantic.shapes_count) {
    throw "PageBackground lifecycle changed ordinary Page.Shapes.Count."
}
if ([long]$baseline.semantic.page_id -ne [long]$created.semantic.page_id -or
    [long]$created.semantic.page_id -ne [long]$deleted.semantic.page_id) {
    throw "Page identity changed across PageBackground lifecycle."
}

$baselineToCreated = Compare-Stages -Left $baseline -Right $created
$createdToDeleted = Compare-Stages -Left $created -Right $deleted
$baselineToDeleted = Compare-Stages -Left $baseline -Right $deleted

if ($baselineToCreated.persistence_equal -or $baselineToCreated.render_equal) {
    throw "PageBackground.Create did not change both persistence and normalized render."
}
if ($createdToDeleted.persistence_equal -or $createdToDeleted.render_equal) {
    throw "PageBackground.Delete did not change both persistence and normalized render."
}
if (-not $baselineToDeleted.render_equal) {
    throw "Delete arm did not restore the baseline normalized render."
}

$result = [ordered]@{
    schema = "chaptera.page-background-auth-read.v1"
    experiment = $ExpectedExperiment
    requested_rgb = [long]$BackgroundRgb
    baseline = [ordered]@{
        semantic = $baseline.semantic
        pub_sha256 = [string]$baseline.fingerprint.artifacts.pub_sha256
        normalized_pdf_sha256 = [string]$baseline.fingerprint.artifacts.normalized_pdf_sha256
    }
    created = [ordered]@{
        semantic = $created.semantic
        pub_sha256 = [string]$created.fingerprint.artifacts.pub_sha256
        normalized_pdf_sha256 = [string]$created.fingerprint.artifacts.normalized_pdf_sha256
    }
    deleted = [ordered]@{
        semantic = $deleted.semantic
        pub_sha256 = [string]$deleted.fingerprint.artifacts.pub_sha256
        normalized_pdf_sha256 = [string]$deleted.fingerprint.artifacts.normalized_pdf_sha256
    }
    baseline_to_created = $baselineToCreated
    created_to_deleted = $createdToDeleted
    baseline_to_deleted = $baselineToDeleted
    claims = [ordered]@{
        page_background_is_separate_from_shapes = $true
        create_persists_after_reopen = $true
        delete_persists_after_reopen = $true
        exact_carrier_field_identified = $false
        apply_to_all_lifecycle_tested = $false
    }
}

$resultPath = Join-Path $analysisDir "page-background-auth-read-01.json"
Write-JsonNoBom -Value $result -Path $resultPath

$logLines = @(
    "PAGE_BACKGROUND_AUTH_READ PASS",
    ("page_id={0}" -f $baseline.semantic.page_id),
    ("shapes_count={0}" -f $baseline.semantic.shapes_count),
    ("baseline_to_created.changed_stream_count={0}" -f $baselineToCreated.changed_stream_count),
    ("created_to_deleted.changed_stream_count={0}" -f $createdToDeleted.changed_stream_count),
    ("baseline_to_deleted.render_equal={0}" -f $baselineToDeleted.render_equal)
)
$logLines | Set-Content -LiteralPath $logPath -Encoding UTF8
Get-Content -LiteralPath $resultPath -Raw
Get-Content -LiteralPath $logPath -Raw
