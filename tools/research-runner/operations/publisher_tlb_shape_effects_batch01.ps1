param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "PUB-TLB-SHAPE-EFFECTS-BATCH01"
$ExpectedInventorySha256 = "a13d86edc1622b7f3cba6524cba7a4553e120864889e59038712061a05003b5c"
$PbFilePublication = 1
$PbFixedFormatTypePDF = 2
$PbIntentStandard = 2
$MsoShapeRectangle = 1
$MsoTrue = -1
$MsoFalse = 0
$TagName = "PUB_ORACLE_ID"
$TagValue = "PUB_TLB_SHAPE_EFFECTS_BATCH01"

$CandidateIds = @(
    "GlowFormat.Radius",
    "GlowFormat.Transparency",
    "GlowFormat.Visible",
    "ReflectionFormat.Type",
    "ReflectionFormat.Transparency",
    "ReflectionFormat.Size",
    "ReflectionFormat.Offset",
    "ReflectionFormat.Blur",
    "ReflectionFormat.Visible"
)

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}
if ([string]$packet.factory.source_inventory_sha256 -ne $ExpectedInventorySha256) {
    throw "Unexpected source inventory SHA-256."
}
$packetIds = @($packet.factory.candidates | ForEach-Object { [string]$_.id })
if ((($packetIds | Sort-Object) -join "|") -ne (($CandidateIds | Sort-Object) -join "|")) {
    throw "Packet candidate set does not match the bounded batch."
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$blastDir = Join-Path $analysisDir "blast-radius"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/tlb-shape-effects-batch01"
New-Item -ItemType Directory -Force -Path $analysisDir,$blastDir,$logDir,$privateDir | Out-Null

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

function Test-EquivalentValue {
    param($Left, $Right)
    if ($null -eq $Left -or $null -eq $Right) {
        return ($null -eq $Left -and $null -eq $Right)
    }
    try {
        return ([Math]::Abs(([double]$Left) - ([double]$Right)) -lt 0.000001)
    }
    catch {
        return ([string]$Left -eq [string]$Right)
    }
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
        throw "Expected exactly one tagged shape; found $($matches.Count)."
    }
    return $matches[0]
}

function Get-CandidateValue {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$CandidateId
    )

    $location = Find-TaggedShape -Document $Document
    $page = $null
    $shape = $null
    $format = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        switch ($CandidateId) {
            "GlowFormat.Radius" {
                $format = $shape.Glow
                return [double]$format.Radius
            }
            "GlowFormat.Transparency" {
                $format = $shape.Glow
                return [double]$format.Transparency
            }
            "GlowFormat.Visible" {
                $format = $shape.Glow
                return [int]$format.Visible
            }
            "ReflectionFormat.Type" {
                $format = $shape.Reflection
                return [int]$format.Type
            }
            "ReflectionFormat.Transparency" {
                $format = $shape.Reflection
                return [double]$format.Transparency
            }
            "ReflectionFormat.Size" {
                $format = $shape.Reflection
                return [double]$format.Size
            }
            "ReflectionFormat.Offset" {
                $format = $shape.Reflection
                return [double]$format.Offset
            }
            "ReflectionFormat.Blur" {
                $format = $shape.Reflection
                return [double]$format.Blur
            }
            "ReflectionFormat.Visible" {
                $format = $shape.Reflection
                return [int]$format.Visible
            }
            default { throw "Unsupported candidate: $CandidateId" }
        }
    }
    finally {
        Release-Com $format
        Release-Com $shape
        Release-Com $page
    }
}

function Set-CandidateValue {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$CandidateId,
        [Parameter(Mandatory = $true)]$Value
    )

    $location = Find-TaggedShape -Document $Document
    $page = $null
    $shape = $null
    $format = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        switch ($CandidateId) {
            "GlowFormat.Radius" {
                $format = $shape.Glow
                $format.Radius = [single]$Value
                break
            }
            "GlowFormat.Transparency" {
                $format = $shape.Glow
                $format.Transparency = [single]$Value
                break
            }
            "GlowFormat.Visible" {
                $format = $shape.Glow
                $format.Visible = [int]$Value
                break
            }
            "ReflectionFormat.Type" {
                $format = $shape.Reflection
                $format.Type = [int]$Value
                break
            }
            "ReflectionFormat.Transparency" {
                $format = $shape.Reflection
                $format.Transparency = [single]$Value
                break
            }
            "ReflectionFormat.Size" {
                $format = $shape.Reflection
                $format.Size = [single]$Value
                break
            }
            "ReflectionFormat.Offset" {
                $format = $shape.Reflection
                $format.Offset = [single]$Value
                break
            }
            "ReflectionFormat.Blur" {
                $format = $shape.Reflection
                $format.Blur = [single]$Value
                break
            }
            "ReflectionFormat.Visible" {
                $format = $shape.Reflection
                $format.Visible = [int]$Value
                break
            }
            default { throw "Unsupported candidate: $CandidateId" }
        }
    }
    finally {
        Release-Com $format
        Release-Com $shape
        Release-Com $page
    }
}

function Get-ChangedValueCandidates {
    param(
        [Parameter(Mandatory = $true)][string]$CandidateId,
        [Parameter(Mandatory = $true)]$Current
    )

    switch ($CandidateId) {
        "GlowFormat.Radius" {
            return @(
                [single](([double]$Current) + 2.0),
                [single]([Math]::Max(0.0, ([double]$Current) - 2.0)),
                [single]5.0,
                [single]10.0
            )
        }
        "GlowFormat.Transparency" {
            return @([single]0.75, [single]0.25, [single]0.5, [single]0.0)
        }
        "GlowFormat.Visible" {
            if ([int]$Current -eq $MsoFalse) { return @([int]$MsoTrue) }
            return @([int]$MsoFalse, [int]$MsoTrue)
        }
        "ReflectionFormat.Type" {
            return @([int]2, [int]1, [int]3)
        }
        "ReflectionFormat.Transparency" {
            return @([single]0.75, [single]0.25, [single]0.5, [single]0.0)
        }
        "ReflectionFormat.Size" {
            return @([single]75.0, [single]50.0, [single]100.0)
        }
        "ReflectionFormat.Offset" {
            return @(
                [single](([double]$Current) + 2.0),
                [single]([Math]::Max(0.0, ([double]$Current) - 2.0)),
                [single]5.0,
                [single]10.0
            )
        }
        "ReflectionFormat.Blur" {
            return @(
                [single](([double]$Current) + 2.0),
                [single]([Math]::Max(0.0, ([double]$Current) - 2.0)),
                [single]5.0,
                [single]10.0
            )
        }
        "ReflectionFormat.Visible" {
            if ([int]$Current -eq $MsoFalse) { return @([int]$MsoTrue) }
            return @([int]$MsoFalse, [int]$MsoTrue)
        }
        default { throw "Unsupported candidate: $CandidateId" }
    }
}

function Get-ShapeEffectsSnapshot {
    param([Parameter(Mandatory = $true)]$Document)

    $location = Find-TaggedShape -Document $Document
    $page = $null
    $shape = $null
    $glow = $null
    $reflection = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $glow = $shape.Glow
        $reflection = $shape.Reflection

        return [ordered]@{
            page_index = [int]$location.page_index
            shape_index = [int]$location.shape_index
            shape_id = [int]$shape.ID
            shape_type = [int]$shape.Type
            geometry = [ordered]@{
                left = [double]$shape.Left
                top = [double]$shape.Top
                width = [double]$shape.Width
                height = [double]$shape.Height
            }
            glow = [ordered]@{
                radius = [double]$glow.Radius
                transparency = [double]$glow.Transparency
                visible = [int]$glow.Visible
            }
            reflection = [ordered]@{
                type = [int]$reflection.Type
                transparency = [double]$reflection.Transparency
                size = [double]$reflection.Size
                offset = [double]$reflection.Offset
                blur = [double]$reflection.Blur
                visible = [int]$reflection.Visible
            }
        }
    }
    finally {
        Release-Com $reflection
        Release-Com $glow
        Release-Com $shape
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
    $fillColor = $null
    $glow = $null
    $reflection = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Documents.Add()
        $page = $doc.Pages.Item(1)
        $shape = $page.Shapes.AddShape($MsoShapeRectangle, 144, 144, 288, 144)
        $shape.Tags.Add($TagName, $TagValue) | Out-Null

        $fill = $shape.Fill
        try { $fill.Visible = $MsoTrue } catch {}
        try { $fill.Solid() } catch {}
        $fillColor = $fill.ForeColor
        $fillColor.RGB = 12632256

        $glow = $shape.Glow
        $glow.Visible = $MsoTrue
        $glow.Radius = [single]10.0
        $glow.Transparency = [single]0.25

        $reflection = $shape.Reflection
        $reflection.Type = 1
        $reflection.Visible = $MsoTrue

        $doc.SaveAs($Path, $PbFilePublication, $false)
    }
    finally {
        Release-Com $reflection
        Release-Com $glow
        Release-Com $fillColor
        Release-Com $fill
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$CandidateId,
        [Parameter(Mandatory = $true)][string]$ArmName,
        [Parameter(Mandatory = $true)][string]$BaselinePath
    )

    $slug = ($CandidateId -replace '[^A-Za-z0-9]+','-').Trim('-').ToLowerInvariant()
    $armDir = Join-Path $privateDir (Join-Path $slug $ArmName)
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null

    $working = Join-Path $armDir "working.pub"
    $output = Join-Path $armDir "output.pub"
    $pdf = Join-Path $armDir "output.pdf"
    $semanticPath = Join-Path $armDir "semantic.json"
    $fingerprintPath = Join-Path $armDir "fingerprint.json"
    Copy-Item -LiteralPath $BaselinePath -Destination $working -Force

    $before = $null
    $runtimeAfter = $null
    $selectedValue = $null
    $attempts = @()
    $armStatus = "ok"
    $armError = $null

    $app = $null
    $doc = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $before = Get-CandidateValue -Document $doc -CandidateId $CandidateId

        if ($ArmName -eq "same_value") {
            try {
                Set-CandidateValue -Document $doc -CandidateId $CandidateId -Value $before
                $runtimeAfter = Get-CandidateValue -Document $doc -CandidateId $CandidateId
                $selectedValue = $before
            }
            catch {
                $armStatus = "setter_error"
                $armError = $_.Exception.Message
            }
        }
        elseif ($ArmName -eq "changed_value") {
            foreach ($candidateValue in @(Get-ChangedValueCandidates -CandidateId $CandidateId -Current $before)) {
                if (Test-EquivalentValue $candidateValue $before) { continue }
                try {
                    Set-CandidateValue -Document $doc -CandidateId $CandidateId -Value $candidateValue
                    $readback = Get-CandidateValue -Document $doc -CandidateId $CandidateId
                    $attempts += [ordered]@{
                        requested = $candidateValue
                        state = "accepted"
                        readback = $readback
                    }
                    if (-not (Test-EquivalentValue $readback $before)) {
                        $selectedValue = $candidateValue
                        $runtimeAfter = $readback
                        break
                    }
                }
                catch {
                    $attempts += [ordered]@{
                        requested = $candidateValue
                        state = "error"
                        message = $_.Exception.Message
                    }
                }
            }
            if ($null -eq $selectedValue) {
                $armStatus = "no_changed_value"
                $runtimeAfter = Get-CandidateValue -Document $doc -CandidateId $CandidateId
            }
        }
        else {
            $runtimeAfter = $before
        }

        $doc.SaveAs($output, $PbFilePublication, $false)
    }
    catch {
        $armStatus = "arm_error"
        $armError = $_.Exception.Message
        throw
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $app2 = $null
    $doc2 = $null
    $semantic = $null
    $reopenedValue = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        $reopenedValue = Get-CandidateValue -Document $doc2 -CandidateId $CandidateId
        $semantic = Get-ShapeEffectsSnapshot -Document $doc2
        Write-PubJson -Value $semantic -Path $semanticPath
        $doc2.ExportAsFixedFormat($PbFixedFormatTypePDF, $pdf, $PbIntentStandard)
    }
    finally {
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $fingerprintTool = Join-Path $repoRoot "tools/pub_operation_algebra_fingerprint.py"
    & python $fingerprintTool --pub $output --pdf $pdf --out $fingerprintPath
    if ($LASTEXITCODE -ne 0) {
        throw "Fingerprint helper failed for $CandidateId / $ArmName with exit code $LASTEXITCODE"
    }
    $fingerprint = Get-Content -LiteralPath $fingerprintPath -Raw | ConvertFrom-Json
    $semanticSha = (Get-FileHash -LiteralPath $semanticPath -Algorithm SHA256).Hash.ToLowerInvariant()

    return [ordered]@{
        status = $armStatus
        error = $armError
        before = $before
        selected_value = $selectedValue
        runtime_after = $runtimeAfter
        runtime_changed = (-not (Test-EquivalentValue $runtimeAfter $before))
        reopened_value = $reopenedValue
        attempts = $attempts
        semantic_snapshot = $semantic
        semantic_fingerprint = [ordered]@{
            contract = "chaptera.publisher-com-shape-effects-snapshot-file.v1"
            sha256 = $semanticSha
        }
        persistence_fingerprint = $fingerprint.persistence_fingerprint
        render_fingerprint = $fingerprint.render_fingerprint
        artifacts = $fingerprint.artifacts
    }
}

function New-BlastEvidence {
    param(
        [Parameter(Mandatory = $true)]$Spec,
        [Parameter(Mandatory = $true)][string]$Mode,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $payload = [ordered]@{
        schema = "chaptera.pub.tlb-property-blast-evidence.v1"
        operation = [ordered]@{
            kind = "publisher-tlb-property-$Mode"
            candidate_id = [string]$Spec.id
            dispid = [int]$Spec.dispid
            value_type = [string]$Spec.value_type
        }
        producer = [ordered]@{
            experiment_id = $ExpectedExperiment
            expected_publisher_version_prefix = [string]$packet.publisher.version_prefix
            source_inventory_sha256 = [string]$packet.factory.source_inventory_sha256
        }
        requested_streams = @()
        expected_derived_streams = @()
        requested_records = @()
        expected_derived_records = @()
        requested_entities = @()
        expected_derived_entities = @()
        arms = [ordered]@{
            source = [ordered]@{}
            control = [ordered]@{}
            mutation = [ordered]@{}
        }
    }
    Write-PubJson -Value $payload -Path $Path
}

function Invoke-BlastRadius {
    param(
        [Parameter(Mandatory = $true)]$Spec,
        [Parameter(Mandatory = $true)][string]$Mode,
        [Parameter(Mandatory = $true)][string]$BaselinePath,
        [Parameter(Mandatory = $true)]$Control,
        [Parameter(Mandatory = $true)]$Mutation
    )

    $slug = ([string]$Spec.id -replace '[^A-Za-z0-9]+','-').Trim('-').ToLowerInvariant()
    $evidencePath = Join-Path $privateDir "$slug/$Mode-blast-evidence.json"
    $receiptPath = Join-Path $blastDir "$slug-$Mode.json"
    New-BlastEvidence -Spec $Spec -Mode $Mode -Path $evidencePath

    $controlPub = Join-Path $privateDir "$slug/control/output.pub"
    $mutationPub = Join-Path $privateDir "$slug/$Mode/output.pub"
    $tool = Join-Path $repoRoot "tools/operation_blast_radius_v1.py"
    $toolArgs = @(
        $tool,
        "--source", $BaselinePath,
        "--control", $controlPub,
        "--mutation", $mutationPub,
        "--evidence", $evidencePath,
        "--out", $receiptPath
    )
    & python @toolArgs
    if ($LASTEXITCODE -ne 0) {
        throw "OperationBlastRadiusV1 failed for $($Spec.id) / $Mode with exit code $LASTEXITCODE"
    }

    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    return [ordered]@{
        receipt_path = ("analysis/blast-radius/{0}-{1}.json" -f $slug,$Mode)
        changed_stream_count = @($receipt.cfb.control_mutation_stream_delta).Count
        topology_delta_count = @($receipt.cfb.control_mutation_topology_delta).Count
        changed_range_count = @($receipt.cfb.control_mutation_byte_ranges).Count
        changed_ranges = @($receipt.cfb.control_mutation_byte_ranges)
        classification_counts = $receipt.classification_counts
    }
}

function Get-ChangedStreams {
    param(
        [Parameter(Mandatory = $true)]$Left,
        [Parameter(Mandatory = $true)]$Right
    )

    $leftMap = @{}
    foreach ($stream in @($Left.streams)) {
        $leftMap[[string]$stream.name] = "$($stream.size):$($stream.sha256)"
    }
    $rightMap = @{}
    foreach ($stream in @($Right.streams)) {
        $rightMap[[string]$stream.name] = "$($stream.size):$($stream.sha256)"
    }

    $names = @(@($leftMap.Keys) + @($rightMap.Keys) | Sort-Object -Unique)
    $changed = @()
    foreach ($name in $names) {
        if (-not $leftMap.ContainsKey($name) -or -not $rightMap.ContainsKey($name) -or $leftMap[$name] -ne $rightMap[$name]) {
            $changed += $name
        }
    }
    return $changed
}

function Compare-Arms {
    param(
        [Parameter(Mandatory = $true)]$Control,
        [Parameter(Mandatory = $true)]$Other
    )

    return [ordered]@{
        semantic_equal = ([string]$Control.semantic_fingerprint.sha256 -eq [string]$Other.semantic_fingerprint.sha256)
        persistence_equal = ([string]$Control.persistence_fingerprint.sha256 -eq [string]$Other.persistence_fingerprint.sha256)
        render_equal = ([string]$Control.render_fingerprint.sha256 -eq [string]$Other.render_fingerprint.sha256)
        changed_streams = @(Get-ChangedStreams -Left $Control.persistence_fingerprint -Right $Other.persistence_fingerprint)
    }
}

function Classify-Candidate {
    param(
        [Parameter(Mandatory = $true)]$Same,
        [Parameter(Mandatory = $true)]$Changed,
        [Parameter(Mandatory = $true)]$SameComparison,
        [Parameter(Mandatory = $true)]$ChangedComparison
    )

    if ([string]$Same.status -ne "ok") { return "inconclusive-same-value-arm" }
    if ([string]$Changed.status -ne "ok") { return "inconclusive-changed-value-arm" }
    if (-not [bool]$Changed.runtime_changed) { return "setter-no-runtime-change" }

    $sameExact = [bool]$SameComparison.semantic_equal -and [bool]$SameComparison.persistence_equal -and [bool]$SameComparison.render_equal
    $changedExact = [bool]$ChangedComparison.semantic_equal -and [bool]$ChangedComparison.persistence_equal -and [bool]$ChangedComparison.render_equal

    if ($changedExact) { return "runtime-only-or-normalized-away" }

    if (-not [bool]$ChangedComparison.semantic_equal -and -not [bool]$ChangedComparison.persistence_equal) {
        if ($sameExact) { return "persisted-semantic-change" }
        if ([bool]$SameComparison.semantic_equal -and -not [bool]$SameComparison.persistence_equal) {
            return "same-value-materialization-plus-persisted-change"
        }
        return "persisted-change-with-same-value-side-effect"
    }

    if ([bool]$ChangedComparison.semantic_equal -and -not [bool]$ChangedComparison.persistence_equal) {
        return "persistence-only-or-normalization"
    }
    if (-not [bool]$ChangedComparison.semantic_equal -and [bool]$ChangedComparison.persistence_equal) {
        return "semantic-change-without-logical-stream-delta"
    }
    return "mixed-or-render-derived-divergence"
}

$baselinePath = Join-Path $privateDir "baseline.pub"
New-BaselinePublication -Path $baselinePath
$baselineHash = (Get-FileHash -LiteralPath $baselinePath -Algorithm SHA256).Hash.ToLowerInvariant()

$baselineApp = $null
$baselineDoc = $null
$baselineSnapshot = $null
try {
    $baselineApp = New-PubPublisherApplication
    $baselineDoc = $baselineApp.Open($baselinePath, $true, $false)
    $baselineSnapshot = Get-ShapeEffectsSnapshot -Document $baselineDoc
}
finally {
    Close-Document $baselineDoc
    Close-PubPublisherApplication $baselineApp
}

$results = @()
$logLines = @(
    "experiment=$ExpectedExperiment",
    "inventory_sha256=$ExpectedInventorySha256",
    "baseline_sha256=$baselineHash"
)

foreach ($candidateId in $CandidateIds) {
    $spec = $packet.factory.candidates | Where-Object { [string]$_.id -eq $candidateId } | Select-Object -First 1
    if ($null -eq $spec) { throw "Missing packet metadata for $candidateId" }

    $control = Invoke-Arm -CandidateId $candidateId -ArmName "control" -BaselinePath $baselinePath
    $same = Invoke-Arm -CandidateId $candidateId -ArmName "same_value" -BaselinePath $baselinePath
    $changed = Invoke-Arm -CandidateId $candidateId -ArmName "changed_value" -BaselinePath $baselinePath

    $sameComparison = Compare-Arms -Control $control -Other $same
    $changedComparison = Compare-Arms -Control $control -Other $changed
    $sameBlast = Invoke-BlastRadius -Spec $spec -Mode "same_value" -BaselinePath $baselinePath -Control $control -Mutation $same
    $changedBlast = Invoke-BlastRadius -Spec $spec -Mode "changed_value" -BaselinePath $baselinePath -Control $control -Mutation $changed
    $classification = Classify-Candidate -Same $same -Changed $changed -SameComparison $sameComparison -ChangedComparison $changedComparison

    $results += [ordered]@{
        candidate = [ordered]@{
            id = $candidateId
            dispid = [int]$spec.dispid
            value_type = [string]$spec.value_type
        }
        control = $control
        same_value = $same
        changed_value = $changed
        comparison = [ordered]@{
            same_vs_control = $sameComparison
            changed_vs_control = $changedComparison
        }
        blast_radius = [ordered]@{
            same_vs_control = $sameBlast
            changed_vs_control = $changedBlast
        }
        classification = $classification
    }

    $logLines += "$candidateId classification=$classification same_streams=$($sameComparison.changed_streams -join ',') changed_streams=$($changedComparison.changed_streams -join ',') same_ranges=$($sameBlast.changed_range_count) changed_ranges=$($changedBlast.changed_range_count)"
}

$result = [ordered]@{
    schema = "chaptera.pub.tlb-native-batch-result.v1"
    experiment_id = $ExpectedExperiment
    task_id = "PUB-T-891"
    batch_id = "TLB-BATCH-01-SHAPE-EFFECTS"
    source = [ordered]@{
        inventory_schema = [string]$packet.factory.source_inventory_schema
        inventory_sha256 = [string]$packet.factory.source_inventory_sha256
        tlb_sha256 = [string]$packet.factory.source_tlb_sha256
        generated_property_put_count = [int]$packet.factory.generated_property_put_count
        admitted_candidate_count = [int]$packet.factory.admitted_candidate_count
    }
    fixture = [ordered]@{
        kind = "generated-single-rectangle"
        baseline_sha256 = $baselineHash
        baseline_semantic_snapshot = $baselineSnapshot
    }
    candidates = $results
    authority_boundary = "This batch is an automated semantic-differential probe of nine TLB-admitted Publisher2019 shape-effect setters. Candidate generation, logical-stream deltas, and physical byte-range receipts are evidence only, not PUB laws by themselves. Promotion requires candidate-specific carrier attribution and review; no result generalizes to other setters, shapes, defaults, precedence contexts, or Publisher versions."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "tlb-shape-effects-batch01.json")
$logLines | Set-Content -LiteralPath (Join-Path $logDir "tlb-shape-effects-batch01.txt") -Encoding ASCII
