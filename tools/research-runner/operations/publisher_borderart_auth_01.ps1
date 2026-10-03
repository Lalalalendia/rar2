param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "BORDERART-AUTH-01"
$PbFilePublication = 1
$PbFixedFormatTypePDF = 2
$PbIntentStandard = 2
$MsoShapeRectangle = 1
$MsoTrue = -1
$MsoFalse = 0
$TagName = "PUB_ORACLE_ID"
$TagValue = "BORDERART_AUTH_01"
$BaselineLineRgb = 16711935
$BaselineLineWeight = 3.25
$MutatedLineRgb = 65535
$MutatedLineWeight = 8.5
$MutatedBorderRgb = 255

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/borderart-auth-01"
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

function Find-TaggedShape {
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
        throw "Expected exactly one tagged rectangle; found $($matches.Count)."
    }
    return $matches[0]
}

function Get-BorderArtCatalog {
    param([Parameter(Mandatory = $true)]$Document)

    $rows = @()
    $collection = $null
    try {
        $collection = $Document.BorderArts
        $count = [int]$collection.Count
        for ($i = 1; $i -le $count; $i++) {
            $item = $null
            try {
                $item = $collection.Item($i)
                $rows += [ordered]@{
                    index = $i
                    name = [string]$item.Name
                }
            }
            finally {
                Release-Com $item
            }
        }
    }
    finally {
        Release-Com $collection
    }
    return @($rows)
}

function Get-LineSnapshot {
    param([Parameter(Mandatory = $true)]$Shape)

    $line = $null
    $color = $null
    try {
        $line = $Shape.Line
        $color = $line.ForeColor
        $dash = $null
        try { $dash = [int]$line.DashStyle } catch {}
        $transparency = $null
        try { $transparency = [double]$line.Transparency } catch {}
        return [ordered]@{
            visible = [int]$line.Visible
            weight = [double]$line.Weight
            rgb = [long]$color.RGB
            dash_style = $dash
            transparency = $transparency
        }
    }
    finally {
        Release-Com $color
        Release-Com $line
    }
}

function Get-BorderArtSnapshot {
    param([Parameter(Mandatory = $true)]$Shape)

    $border = $null
    $color = $null
    $snapshot = [ordered]@{
        access_ok = $false
        access_error = $null
        exists = $null
        name = $null
        weight = $null
        color_rgb = $null
        stretch_pictures = $null
        property_errors = @()
    }

    try {
        $border = $Shape.BorderArt
        $snapshot.access_ok = $true
    }
    catch {
        $snapshot.access_error = $_.Exception.Message
        return $snapshot
    }

    try {
        try { $snapshot.exists = [bool]$border.Exists } catch { $snapshot.property_errors += "Exists: $($_.Exception.Message)" }
        if ([bool]$snapshot.exists) {
            try { $snapshot.name = [string]$border.Name } catch { $snapshot.property_errors += "Name: $($_.Exception.Message)" }
            try { $snapshot.weight = [double]$border.Weight } catch { $snapshot.property_errors += "Weight: $($_.Exception.Message)" }
            try {
                $color = $border.Color
                $snapshot.color_rgb = [long]$color.RGB
            }
            catch {
                $snapshot.property_errors += "Color.RGB: $($_.Exception.Message)"
            }
            finally {
                Release-Com $color
                $color = $null
            }
            try { $snapshot.stretch_pictures = [bool]$border.StretchPictures } catch { $snapshot.property_errors += "StretchPictures: $($_.Exception.Message)" }
        }
    }
    finally {
        Release-Com $border
    }
    return $snapshot
}

function Get-ShapeSnapshot {
    param([Parameter(Mandatory = $true)]$Document)

    $location = Find-TaggedShape -Document $Document
    $page = $null
    $shape = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        return [ordered]@{
            page_index = [int]$location.page_index
            shape_index = [int]$location.shape_index
            shape_id = [int]$shape.ID
            shape_type = [int]$shape.Type
            shape_name = [string]$shape.Name
            geometry = [ordered]@{
                left = [double]$shape.Left
                top = [double]$shape.Top
                width = [double]$shape.Width
                height = [double]$shape.Height
            }
            line = Get-LineSnapshot -Shape $shape
            borderart = Get-BorderArtSnapshot -Shape $shape
            borderart_catalog = @(Get-BorderArtCatalog -Document $Document)
        }
    }
    finally {
        Release-Com $shape
        Release-Com $page
    }
}

function Set-OrdinaryLine {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][long]$Rgb,
        [Parameter(Mandatory = $true)][double]$Weight
    )

    $line = $null
    $color = $null
    try {
        $line = $Shape.Line
        $line.Visible = $MsoTrue
        $line.Weight = $Weight
        $color = $line.ForeColor
        $color.RGB = $Rgb
    }
    finally {
        Release-Com $color
        Release-Com $line
    }
}

function Apply-BorderArtByName {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $attempts = @()
    $border = $null

    try {
        $border = $Shape.BorderArt
        try {
            $border.Set($Name)
            return [ordered]@{ method = "Shape.BorderArt.Set"; attempts = @($attempts) }
        }
        catch {
            $attempts += [ordered]@{ method = "Shape.BorderArt.Set"; error = $_.Exception.Message }
        }

        try {
            $border.Name = $Name
            return [ordered]@{ method = "Shape.BorderArt.Name"; attempts = @($attempts) }
        }
        catch {
            $attempts += [ordered]@{ method = "Shape.BorderArt.Name"; error = $_.Exception.Message }
        }
    }
    catch {
        $attempts += [ordered]@{ method = "Shape.BorderArt access"; error = $_.Exception.Message }
    }
    finally {
        Release-Com $border
    }

    $borderFormat = $null
    try {
        $borderFormat = $Shape.BorderArtFormat
        $borderFormat.Name = $Name
        return [ordered]@{ method = "Shape.BorderArtFormat.Name"; attempts = @($attempts) }
    }
    catch {
        $attempts += [ordered]@{ method = "Shape.BorderArtFormat.Name"; error = $_.Exception.Message }
    }
    finally {
        Release-Com $borderFormat
    }

    throw ("BorderArt apply failed. Attempts: " + (($attempts | ConvertTo-Json -Compress -Depth 8)))
}

function Delete-BorderArt {
    param([Parameter(Mandatory = $true)]$Shape)

    $border = $null
    try {
        $border = $Shape.BorderArt
        $exists = $false
        try { $exists = [bool]$border.Exists } catch {}
        if (-not $exists) {
            throw "BorderArt.Delete requested but BorderArt.Exists is false."
        }
        $border.Delete()
    }
    finally {
        Release-Com $border
    }
}

function Mutate-BorderArtWeight {
    param([Parameter(Mandatory = $true)]$Shape)

    $border = $null
    try {
        $border = $Shape.BorderArt
        if (-not [bool]$border.Exists) { throw "BorderArt is absent." }
        $before = [double]$border.Weight
        $target = $before + 4.0
        $border.Weight = $target
        return [ordered]@{ before = $before; requested = $target; readback = [double]$border.Weight }
    }
    finally {
        Release-Com $border
    }
}

function Mutate-BorderArtColor {
    param([Parameter(Mandatory = $true)]$Shape)

    $border = $null
    $color = $null
    try {
        $border = $Shape.BorderArt
        if (-not [bool]$border.Exists) { throw "BorderArt is absent." }
        $color = $border.Color
        $before = [long]$color.RGB
        $color.RGB = $MutatedBorderRgb
        return [ordered]@{ before = $before; requested = $MutatedBorderRgb; readback = [long]$color.RGB }
    }
    finally {
        Release-Com $color
        Release-Com $border
    }
}

function Mutate-BorderArtStretch {
    param([Parameter(Mandatory = $true)]$Shape)

    $border = $null
    try {
        $border = $Shape.BorderArt
        if (-not [bool]$border.Exists) { throw "BorderArt is absent." }
        $before = [bool]$border.StretchPictures
        $target = -not $before
        $border.StretchPictures = $target
        return [ordered]@{ before = $before; requested = $target; readback = [bool]$border.StretchPictures }
    }
    finally {
        Release-Com $border
    }
}

function New-BaselinePublication {
    param([Parameter(Mandatory = $true)][string]$Path)

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $fill = $null
    $fillColor = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Documents.Add()
        $page = $doc.Pages.Item(1)
        $shape = $page.Shapes.AddShape($MsoShapeRectangle, 108, 144, 396, 216)
        $shape.Tags.Add($TagName, $TagValue) | Out-Null

        $fill = $shape.Fill
        try { $fill.Visible = $MsoTrue } catch {}
        try { $fill.Solid() } catch {}
        $fillColor = $fill.ForeColor
        $fillColor.RGB = 16777215

        Set-OrdinaryLine -Shape $shape -Rgb $BaselineLineRgb -Weight $BaselineLineWeight
        $doc.SaveAs($Path, $PbFilePublication, $false)
    }
    finally {
        Release-Com $fillColor
        Release-Com $fill
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Invoke-Fingerprint {
    param(
        [Parameter(Mandatory = $true)][string]$PubPath,
        [Parameter(Mandatory = $true)][string]$PdfPath,
        [Parameter(Mandatory = $true)][string]$SemanticPath,
        [Parameter(Mandatory = $true)][string]$FingerprintPath
    )

    $fingerprintTool = Join-Path $repoRoot "tools/pub_operation_algebra_fingerprint.py"
    & python $fingerprintTool --pub $PubPath --pdf $PdfPath --semantic $SemanticPath --out $FingerprintPath
    if ($LASTEXITCODE -ne 0) {
        throw "Fingerprint helper failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $FingerprintPath -Raw | ConvertFrom-Json
}

function Snapshot-Publication {
    param(
        [Parameter(Mandatory = $true)][string]$PubPath,
        [Parameter(Mandatory = $true)][string]$StageDir
    )

    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    $semanticPath = Join-Path $StageDir "semantic.json"
    $pdfPath = Join-Path $StageDir "render.pdf"
    $fingerprintPath = Join-Path $StageDir "fingerprint.json"

    $app = $null
    $doc = $null
    $semantic = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($PubPath, $true, $false)
        $semantic = Get-ShapeSnapshot -Document $doc
        $semantic | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $semanticPath -Encoding UTF8
        $doc.ExportAsFixedFormat($PbFixedFormatTypePDF, $pdfPath, $PbIntentStandard)
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $fingerprint = Invoke-Fingerprint -PubPath $PubPath -PdfPath $pdfPath -SemanticPath $semanticPath -FingerprintPath $fingerprintPath
    return [ordered]@{
        semantic = $semantic
        semantic_fingerprint = $fingerprint.semantic_fingerprint
        persistence_fingerprint = $fingerprint.persistence_fingerprint
        render_fingerprint = $fingerprint.render_fingerprint
        artifacts = $fingerprint.artifacts
    }
}

function Save-FromSource {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePub,
        [Parameter(Mandatory = $true)][string]$OutputPub,
        [Parameter(Mandatory = $true)][scriptblock]$Mutation
    )

    $working = [System.IO.Path]::ChangeExtension($OutputPub, ".working.pub")
    Copy-Item -LiteralPath $SourcePub -Destination $working -Force

    $app = $null
    $doc = $null
    $mutationReceipt = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $location = Find-TaggedShape -Document $doc
        $page = $null
        $shape = $null
        try {
            $page = $doc.Pages.Item([int]$location.page_index)
            $shape = $page.Shapes.Item([int]$location.shape_index)
            $mutationReceipt = & $Mutation $doc $shape
        }
        finally {
            Release-Com $shape
            Release-Com $page
        }
        $doc.SaveAs($OutputPub, $PbFilePublication, $false)
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    return $mutationReceipt
}

function Stream-Map {
    param($Fingerprint)

    $map = @{}
    foreach ($stream in @($Fingerprint.persistence_fingerprint.streams)) {
        $map[[string]$stream.name] = [string]$stream.sha256
    }
    return $map
}

function Compare-Stages {
    param(
        [Parameter(Mandatory = $true)]$Left,
        [Parameter(Mandatory = $true)]$Right
    )

    $leftMap = Stream-Map -Fingerprint $Left
    $rightMap = Stream-Map -Fingerprint $Right
    $names = @($leftMap.Keys + $rightMap.Keys | Sort-Object -Unique)
    $changed = @()
    foreach ($name in $names) {
        if (-not $leftMap.ContainsKey($name) -or -not $rightMap.ContainsKey($name) -or $leftMap[$name] -ne $rightMap[$name]) {
            $changed += $name
        }
    }

    return [ordered]@{
        semantic_equal = ([string]$Left.semantic_fingerprint.sha256 -eq [string]$Right.semantic_fingerprint.sha256)
        persistence_equal = ([string]$Left.persistence_fingerprint.sha256 -eq [string]$Right.persistence_fingerprint.sha256)
        render_equal = ([string]$Left.render_fingerprint.sha256 -eq [string]$Right.render_fingerprint.sha256)
        changed_streams = @($changed)
    }
}

function Line-Equivalent {
    param($A, $B)
    if ($null -eq $A -or $null -eq $B) { return $false }
    return (
        [int]$A.visible -eq [int]$B.visible -and
        [Math]::Abs(([double]$A.weight) - ([double]$B.weight)) -lt 0.000001 -and
        [long]$A.rgb -eq [long]$B.rgb -and
        [string]$A.dash_style -eq [string]$B.dash_style
    )
}

$baselinePub = Join-Path $privateDir "baseline.pub"
New-BaselinePublication -Path $baselinePub
$control = Snapshot-Publication -PubPath $baselinePub -StageDir (Join-Path $privateDir "control")

$catalog = @($control.semantic.borderart_catalog)
if ($catalog.Count -lt 1) {
    throw "Publisher Document.BorderArts catalog is empty."
}
$selectedCatalog = $catalog | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_.name) } | Select-Object -First 1
if ($null -eq $selectedCatalog) {
    throw "Publisher Document.BorderArts catalog has no non-empty name."
}
$selectedName = [string]$selectedCatalog.name

$applyPub = Join-Path $privateDir "apply.pub"
$applyReceipt = Save-FromSource -SourcePub $baselinePub -OutputPub $applyPub -Mutation {
    param($doc, $shape)
    return Apply-BorderArtByName -Shape $shape -Name $selectedName
}
$applied = Snapshot-Publication -PubPath $applyPub -StageDir (Join-Path $privateDir "apply")

$deletePub = Join-Path $privateDir "delete.pub"
$deleteReceipt = Save-FromSource -SourcePub $applyPub -OutputPub $deletePub -Mutation {
    param($doc, $shape)
    Delete-BorderArt -Shape $shape
    return [ordered]@{ deleted = $true }
}
$deleted = Snapshot-Publication -PubPath $deletePub -StageDir (Join-Path $privateDir "delete")

$linePub = Join-Path $privateDir "line-mutate.pub"
$lineReceipt = Save-FromSource -SourcePub $applyPub -OutputPub $linePub -Mutation {
    param($doc, $shape)
    $before = Get-LineSnapshot -Shape $shape
    Set-OrdinaryLine -Shape $shape -Rgb $MutatedLineRgb -Weight $MutatedLineWeight
    $after = Get-LineSnapshot -Shape $shape
    return [ordered]@{ before = $before; after = $after }
}
$lineMutated = Snapshot-Publication -PubPath $linePub -StageDir (Join-Path $privateDir "line-mutate")

$weightPub = Join-Path $privateDir "border-weight.pub"
$weightReceipt = Save-FromSource -SourcePub $applyPub -OutputPub $weightPub -Mutation {
    param($doc, $shape)
    return Mutate-BorderArtWeight -Shape $shape
}
$weightMutated = Snapshot-Publication -PubPath $weightPub -StageDir (Join-Path $privateDir "border-weight")

$colorPub = Join-Path $privateDir "border-color.pub"
$colorReceipt = Save-FromSource -SourcePub $applyPub -OutputPub $colorPub -Mutation {
    param($doc, $shape)
    return Mutate-BorderArtColor -Shape $shape
}
$colorMutated = Snapshot-Publication -PubPath $colorPub -StageDir (Join-Path $privateDir "border-color")

$stretchPub = Join-Path $privateDir "border-stretch.pub"
$stretchReceipt = Save-FromSource -SourcePub $applyPub -OutputPub $stretchPub -Mutation {
    param($doc, $shape)
    return Mutate-BorderArtStretch -Shape $shape
}
$stretchMutated = Snapshot-Publication -PubPath $stretchPub -StageDir (Join-Path $privateDir "border-stretch")

$controlVsApply = Compare-Stages -Left $control -Right $applied
$applyVsDelete = Compare-Stages -Left $applied -Right $deleted
$applyVsLine = Compare-Stages -Left $applied -Right $lineMutated
$applyVsWeight = Compare-Stages -Left $applied -Right $weightMutated
$applyVsColor = Compare-Stages -Left $applied -Right $colorMutated
$applyVsStretch = Compare-Stages -Left $applied -Right $stretchMutated

$applyPersists = (
    [bool]$applied.semantic.borderart.access_ok -and
    [bool]$applied.semantic.borderart.exists -and
    [string]$applied.semantic.borderart.name -eq $selectedName
)
$deleteRemoves = (
    [bool]$deleted.semantic.borderart.access_ok -and
    -not [bool]$deleted.semantic.borderart.exists
)
$linePreservedAcrossApply = Line-Equivalent -A $control.semantic.line -B $applied.semantic.line
$lineRestoredAfterDelete = Line-Equivalent -A $control.semantic.line -B $deleted.semantic.line
$lineMutationPersists = (
    [long]$lineMutated.semantic.line.rgb -eq $MutatedLineRgb -and
    [Math]::Abs(([double]$lineMutated.semantic.line.weight) - $MutatedLineWeight) -lt 0.000001
)
$borderSurvivesLineMutation = (
    [bool]$lineMutated.semantic.borderart.exists -and
    [string]$lineMutated.semantic.borderart.name -eq $selectedName
)
$weightMutationPersists = (
    [bool]$weightMutated.semantic.borderart.exists -and
    [Math]::Abs(([double]$weightMutated.semantic.borderart.weight) - ([double]$weightReceipt.requested)) -lt 0.000001
)
$colorMutationPersists = (
    [bool]$colorMutated.semantic.borderart.exists -and
    [long]$colorMutated.semantic.borderart.color_rgb -eq $MutatedBorderRgb
)
$stretchMutationPersists = (
    [bool]$stretchMutated.semantic.borderart.exists -and
    [bool]$stretchMutated.semantic.borderart.stretch_pictures -eq [bool]$stretchReceipt.requested
)

$classification = "inconclusive"
if ($applyPersists -and $deleteRemoves -and $linePreservedAcrossApply -and $lineRestoredAfterDelete -and $lineMutationPersists -and $borderSurvivesLineMutation) {
    $classification = "independent-borderart-layer-candidate"
}
elseif ($applyPersists -and $deleteRemoves) {
    $classification = "persisted-borderart-with-line-coupling-or-materialization"
}
elseif (-not $applyPersists) {
    $classification = "borderart-apply-did-not-survive-reopen"
}

$result = [ordered]@{
    schema = "chaptera.publisher.borderart-auth-01.v1"
    experiment_id = $ExpectedExperiment
    authority_boundary = "Bounded Publisher2019 rectangle experiment only. It proves COM/persistence/render relations among built-in Shape.BorderArt, ordinary Shape.Line and Save/reopen. It does not generalize to text boxes, picture frames, custom BorderArt portability, or exact .PUB carrier naming without separate byte-localization evidence."
    documentation_contract = [ordered]@{
        shape_borderart_surface = "Shape.BorderArt -> BorderArtFormat"
        borderart_operations = @("Set/Name","Delete","Weight","Color.RGB","StretchPictures")
        eligible_control = "rectangle"
    }
    fixture = [ordered]@{
        kind = "generated"
        tag_name = $TagName
        tag_value = $TagValue
        baseline_line_rgb = $BaselineLineRgb
        baseline_line_weight = $BaselineLineWeight
        baseline_pub_sha256 = $control.artifacts.pub_sha256
    }
    selected_borderart = [ordered]@{
        catalog_index = [int]$selectedCatalog.index
        name = $selectedName
        catalog_count = $catalog.Count
    }
    mutation_receipts = [ordered]@{
        apply = $applyReceipt
        delete = $deleteReceipt
        line = $lineReceipt
        border_weight = $weightReceipt
        border_color = $colorReceipt
        border_stretch = $stretchReceipt
    }
    stages = [ordered]@{
        control = $control
        applied = $applied
        deleted = $deleted
        line_mutated = $lineMutated
        border_weight_mutated = $weightMutated
        border_color_mutated = $colorMutated
        border_stretch_mutated = $stretchMutated
    }
    comparisons = [ordered]@{
        control_vs_apply = $controlVsApply
        apply_vs_delete = $applyVsDelete
        apply_vs_line_mutate = $applyVsLine
        apply_vs_border_weight = $applyVsWeight
        apply_vs_border_color = $applyVsColor
        apply_vs_border_stretch = $applyVsStretch
    }
    checks = [ordered]@{
        apply_persists_after_reopen = [bool]$applyPersists
        ordinary_line_preserved_across_apply = [bool]$linePreservedAcrossApply
        delete_removes_borderart_after_reopen = [bool]$deleteRemoves
        ordinary_line_restored_or_preserved_after_delete = [bool]$lineRestoredAfterDelete
        ordinary_line_mutation_persists_while_borderart_present = [bool]$lineMutationPersists
        borderart_survives_ordinary_line_mutation = [bool]$borderSurvivesLineMutation
        borderart_weight_mutation_persists = [bool]$weightMutationPersists
        borderart_color_mutation_persists = [bool]$colorMutationPersists
        borderart_stretch_mutation_persists = [bool]$stretchMutationPersists
    }
    classification = $classification
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "borderart-auth-01.json")

@(
    "experiment=$ExpectedExperiment",
    "selected_borderart=$selectedName",
    "catalog_count=$($catalog.Count)",
    "apply_method=$($applyReceipt.method)",
    "apply_persists=$applyPersists",
    "line_preserved_across_apply=$linePreservedAcrossApply",
    "delete_removes=$deleteRemoves",
    "line_restored_after_delete=$lineRestoredAfterDelete",
    "line_mutation_persists=$lineMutationPersists",
    "border_survives_line_mutation=$borderSurvivesLineMutation",
    "weight_mutation_persists=$weightMutationPersists",
    "color_mutation_persists=$colorMutationPersists",
    "stretch_mutation_persists=$stretchMutationPersists",
    "classification=$classification"
) | Set-Content -LiteralPath (Join-Path $logDir "borderart-auth-01.txt") -Encoding ASCII
