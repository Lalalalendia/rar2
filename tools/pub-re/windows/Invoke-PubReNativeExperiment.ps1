param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,

    [Parameter(Mandatory = $true)]
    [string]$OutputRoot,

    [switch]$ValidateManifestOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExperimentSchema = "chaptera.pub-re-native-experiment.v1"
$ReceiptSchema = "chaptera.pub-re-native-receipt.v1"
$StageReceiptSchema = "chaptera.pub-re-native-stage.v1"
$DiffSchema = "chaptera.pub-re-experiment.v1"
$MsoAutomationSecurityForceDisable = 3
$PbFilePublication = 1

function Get-OptionalProperty {
    param(
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        $Object,
        [Parameter(Mandatory = $true)]
        [string]$Name,
        $Default = $null
    )

    if ($null -eq $Object) {
        return $Default
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $Default
    }
    return $property.Value
}

function Assert-ExperimentId {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$') {
        throw "manifest_invalid_experiment_id"
    }
}

function Assert-Sha256 {
    param(
        [Parameter(Mandatory = $true)][string]$Value,
        [Parameter(Mandatory = $true)][string]$Field
    )

    if ($Value -notmatch '^[0-9A-Fa-f]{64}$') {
        throw "manifest_invalid_sha256:$Field"
    }
}

function Assert-NativeManifest {
    param([Parameter(Mandatory = $true)]$Manifest)

    if ([string](Get-OptionalProperty $Manifest "schema" "") -ne $ExperimentSchema) {
        throw "manifest_unsupported_schema"
    }

    $experimentId = [string](Get-OptionalProperty $Manifest "experiment_id" "")
    Assert-ExperimentId $experimentId

    $source = Get-OptionalProperty $Manifest "source"
    if ($null -eq $source) {
        throw "manifest_missing_source"
    }
    $sourcePath = [string](Get-OptionalProperty $source "path" "")
    if ([string]::IsNullOrWhiteSpace($sourcePath)) {
        throw "manifest_missing_source_path"
    }
    Assert-Sha256 ([string](Get-OptionalProperty $source "expected_sha256" "")) "source.expected_sha256"

    $operation = Get-OptionalProperty $Manifest "operation"
    if ($null -eq $operation) {
        throw "manifest_missing_operation"
    }
    $kind = [string](Get-OptionalProperty $operation "kind" "")
    if ($kind -ne "snapshot_only" -and $kind -ne "shape_rotation_delta") {
        throw "manifest_unsupported_operation"
    }

    if ($kind -eq "shape_rotation_delta") {
        $selector = Get-OptionalProperty $operation "selector"
        if ($null -eq $selector) {
            throw "manifest_missing_selector"
        }
        $pageId = Get-OptionalProperty $selector "page_id"
        $shapeId = Get-OptionalProperty $selector "shape_id"
        if ($null -eq $pageId -or $null -eq $shapeId) {
            throw "manifest_selector_requires_page_id_shape_id"
        }

        $deltaRaw = Get-OptionalProperty $operation "delta_degrees"
        if ($null -eq $deltaRaw) {
            throw "manifest_missing_delta_degrees"
        }
        $delta = [double]$deltaRaw
        if ([double]::IsNaN($delta) -or [double]::IsInfinity($delta) -or [math]::Abs($delta) -lt 0.0000001) {
            throw "manifest_invalid_delta_degrees"
        }
    }

    $policy = Get-OptionalProperty $Manifest "policy"
    $writer = [string](Get-OptionalProperty $policy "writer" "current")
    if ($writer -ne "current") {
        throw "manifest_v02_writer_must_be_current"
    }
    $grace = [int](Get-OptionalProperty $policy "process_exit_grace_seconds" 60)
    if ($grace -lt 1 -or $grace -gt 120) {
        throw "manifest_invalid_process_exit_grace_seconds"
    }

    $attribution = Get-OptionalProperty $Manifest "attribution"
    $stream = [string](Get-OptionalProperty $attribution "officeart_stream" "")
    if (-not [string]::IsNullOrWhiteSpace($stream) -and -not $stream.StartsWith("/")) {
        throw "manifest_officeart_stream_must_be_absolute"
    }
}

function Get-FileFingerprint {
    param([Parameter(Mandatory = $true)][string]$Path)

    $item = Get-Item -LiteralPath $Path
    if ($item.PSIsContainer) {
        throw "source_is_directory"
    }
    $hash = Get-FileHash -LiteralPath $Path -Algorithm SHA256
    return [ordered]@{
        sha256 = $hash.Hash.ToLowerInvariant()
        byte_len = [int64]$item.Length
    }
}

function Format-HResult {
    param([Parameter(Mandatory = $true)][int]$HResult)
    return ('0x{0:X8}' -f ($HResult -band 0xFFFFFFFFL))
}

function Get-SafeValue {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Getter,
        [Parameter(Mandatory = $true)][string]$Member
    )

    try {
        return [ordered]@{
            state = "value"
            member = $Member
            value = & $Getter
        }
    }
    catch {
        $hresult = $null
        if ($_.Exception.HResult) {
            $hresult = Format-HResult ([int]$_.Exception.HResult)
        }
        return [ordered]@{
            state = "error"
            member = $Member
            hresult = $hresult
        }
    }
}

function Write-Json {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $parent = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    $Value | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $Path -Encoding utf8
}

function Set-NativeStage {
    param([Parameter(Mandatory = $true)][string]$Stage)

    $script:NativeStageSequence = [int]$script:NativeStageSequence + 1
    Write-Host ("PUB_RE_NATIVE_STAGE sequence={0} stage={1}" -f $script:NativeStageSequence, $Stage)

    if (-not [string]::IsNullOrWhiteSpace([string]$script:NativeStageReceiptPath)) {
        Write-Json -Value ([ordered]@{
            schema = $StageReceiptSchema
            experiment_id = [string]$script:NativeExperimentId
            operation = [string]$script:NativeOperationKind
            sequence = [int]$script:NativeStageSequence
            stage = $Stage
        }) -Path $script:NativeStageReceiptPath
    }
}

function Get-PublisherProcessCount {
    return @((Get-Process -Name MSPUB -ErrorAction SilentlyContinue)).Count
}

function Assert-PublisherIdle {
    if ((Get-PublisherProcessCount) -ne 0) {
        throw "publisher_busy"
    }
}

function Wait-PublisherExit {
    param([Parameter(Mandatory = $true)][int]$GraceSeconds)

    $deadline = [DateTime]::UtcNow.AddSeconds($GraceSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Get-PublisherProcessCount) -eq 0) {
            return $true
        }
        Start-Sleep -Milliseconds 500
    }
    return ((Get-PublisherProcessCount) -eq 0)
}

function New-SafePublisherApplication {
    param([switch]$Visible)

    $application = New-Object -ComObject Publisher.Application
    $before = Get-SafeValue { [int]$application.AutomationSecurity } "Application.AutomationSecurity"
    if ($before.state -ne "value") {
        try { $application.Quit() } catch {}
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($application) } catch {}
        throw "automation_security_unavailable"
    }

    try {
        $application.AutomationSecurity = $MsoAutomationSecurityForceDisable
    }
    catch {
        try { $application.Quit() } catch {}
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($application) } catch {}
        throw "automation_security_force_disable_failed"
    }

    $selected = Get-SafeValue { [int]$application.AutomationSecurity } "Application.AutomationSecurity"
    if ($selected.state -ne "value" -or [int]$selected.value -ne $MsoAutomationSecurityForceDisable) {
        try { $application.Quit() } catch {}
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($application) } catch {}
        throw "automation_security_force_disable_unconfirmed"
    }

    if ($Visible) {
        try { $application.ActiveWindow.Visible = $true } catch {}
    }

    return [ordered]@{
        application = $application
        automation_security_before = [int]$before.value
        automation_security_selected = [int]$selected.value
    }
}

function Close-SafePublisherApplication {
    param(
        $Application,
        [Parameter(Mandatory = $false)]$AutomationSecurityBefore
    )

    if ($null -eq $Application) {
        return [ordered]@{ security_restore = "not_applicable"; quit = "not_applicable" }
    }

    $restore = "not_attempted"
    if ($null -ne $AutomationSecurityBefore) {
        try {
            $Application.AutomationSecurity = [int]$AutomationSecurityBefore
            $restore = "ok"
        }
        catch {
            $restore = "error"
        }
    }

    $quit = "not_attempted"
    try {
        $Application.Quit()
        $quit = "ok"
    }
    catch {
        $quit = "error"
    }

    try {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Application)
    }
    catch {}

    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()

    return [ordered]@{
        security_restore = $restore
        quit = $quit
    }
}

function Release-ComObject {
    param($Object)
    if ($null -eq $Object) {
        return
    }
    try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object) } catch {}
}

function Get-ShapeSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][int]$PageId,
        [Parameter(Mandatory = $true)][string]$TraversalPath
    )

    $groupCount = 0
    try { $groupCount = [int]$Shape.GroupItems.Count } catch {}

    return [ordered]@{
        page_id = $PageId
        traversal_path = $TraversalPath
        shape_id = Get-SafeValue { [int]$Shape.ID } "Shape.ID"
        shape_type = Get-SafeValue { [int]$Shape.Type } "Shape.Type"
        left_points = Get-SafeValue { [double]$Shape.Left } "Shape.Left"
        top_points = Get-SafeValue { [double]$Shape.Top } "Shape.Top"
        width_points = Get-SafeValue { [double]$Shape.Width } "Shape.Width"
        height_points = Get-SafeValue { [double]$Shape.Height } "Shape.Height"
        rotation_degrees = Get-SafeValue { [double]$Shape.Rotation } "Shape.Rotation"
        has_text_frame = Get-SafeValue { [int]$Shape.HasTextFrame } "Shape.HasTextFrame"
        group_item_count = $groupCount
    }
}

function Add-ShapeInventoryRecursive {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][int]$PageId,
        [Parameter(Mandatory = $true)][string]$TraversalPath,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][System.Collections.ArrayList]$Output
    )

    [void]$Output.Add((Get-ShapeSnapshot -Shape $Shape -PageId $PageId -TraversalPath $TraversalPath))

    $count = 0
    try { $count = [int]$Shape.GroupItems.Count } catch { return }
    for ($i = 1; $i -le $count; $i++) {
        $child = $null
        try {
            $child = $Shape.GroupItems.Item($i)
            Add-ShapeInventoryRecursive -Shape $child -PageId $PageId -TraversalPath "$TraversalPath/group[$i]" -Output $Output
        }
        finally {
            Release-ComObject $child
        }
    }
}

function Get-DocumentInventory {
    param([Parameter(Mandatory = $true)]$Document)

    $pages = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $Document.Pages.Item($pageIndex)
        try {
            $pageId = [int]$page.PageID
            $shapes = New-Object System.Collections.ArrayList
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $page.Shapes.Item($shapeIndex)
                try {
                    Add-ShapeInventoryRecursive -Shape $shape -PageId $pageId -TraversalPath "shape[$shapeIndex]" -Output $shapes
                }
                finally {
                    Release-ComObject $shape
                }
            }
            $pages += [ordered]@{
                collection_index = $pageIndex
                page_id = $pageId
                shapes = @($shapes)
            }
        }
        finally {
            Release-ComObject $page
        }
    }

    return [ordered]@{
        page_count = [int]$Document.Pages.Count
        pages = $pages
    }
}

function Add-TargetMatchesRecursive {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][int]$PageId,
        [Parameter(Mandatory = $true)][int]$WantedShapeId,
        [Parameter(Mandatory = $true)][string]$TraversalPath,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][System.Collections.ArrayList]$Matches
    )

    $shapeId = $null
    try { $shapeId = [int]$Shape.ID } catch {}
    if ($null -ne $shapeId -and $shapeId -eq $WantedShapeId) {
        [void]$Matches.Add([ordered]@{
            page_id = $PageId
            traversal_path = $TraversalPath
            shape = $Shape
        })
        return
    }

    $count = 0
    try { $count = [int]$Shape.GroupItems.Count } catch { return }
    for ($i = 1; $i -le $count; $i++) {
        $child = $null
        try {
            $child = $Shape.GroupItems.Item($i)
            Add-TargetMatchesRecursive -Shape $child -PageId $PageId -WantedShapeId $WantedShapeId -TraversalPath "$TraversalPath/group[$i]" -Matches $Matches
        }
        finally {
            if ($null -ne $child) {
                $isMatch = $false
                foreach ($match in $Matches) {
                    if ([object]::ReferenceEquals($match.shape, $child)) {
                        $isMatch = $true
                        break
                    }
                }
                if (-not $isMatch) {
                    Release-ComObject $child
                }
            }
        }
    }
}

function Find-TargetShape {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$WantedPageId,
        [Parameter(Mandatory = $true)][int]$WantedShapeId
    )

    $page = $null
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $candidate = $Document.Pages.Item($pageIndex)
        if ([int]$candidate.PageID -eq $WantedPageId) {
            $page = $candidate
            break
        }
        Release-ComObject $candidate
    }
    if ($null -eq $page) {
        return $null
    }

    try {
        $matches = New-Object System.Collections.ArrayList
        for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
            $shape = $page.Shapes.Item($shapeIndex)
            Add-TargetMatchesRecursive -Shape $shape -PageId $WantedPageId -WantedShapeId $WantedShapeId -TraversalPath "shape[$shapeIndex]" -Matches $matches
            $isMatch = $false
            foreach ($match in $matches) {
                if ([object]::ReferenceEquals($match.shape, $shape)) {
                    $isMatch = $true
                    break
                }
            }
            if (-not $isMatch) {
                Release-ComObject $shape
            }
        }

        if ($matches.Count -eq 0) {
            return $null
        }
        if ($matches.Count -ne 1) {
            foreach ($match in $matches) { Release-ComObject $match.shape }
            throw "selector_ambiguous"
        }
        return $matches[0]
    }
    finally {
        Release-ComObject $page
    }
}

$manifestSource = Get-Content -LiteralPath $ManifestPath -Raw
$manifest = $manifestSource | ConvertFrom-Json
Assert-NativeManifest $manifest

if ($ValidateManifestOnly) {
    Write-Host "PUB_RE_NATIVE_MANIFEST_OK"
    exit 0
}

if ($env:OS -ne "Windows_NT") {
    throw "native_oracle_requires_windows"
}

Assert-PublisherIdle

New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$runRoot = (Resolve-Path -LiteralPath $OutputRoot).Path
$privateDir = Join-Path $runRoot "private"
$evidenceDir = Join-Path $runRoot "evidence"
New-Item -ItemType Directory -Force -Path $privateDir | Out-Null
New-Item -ItemType Directory -Force -Path $evidenceDir | Out-Null

$sourceConfig = $manifest.source
$sourcePath = [string]$sourceConfig.path
if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
    throw "source_missing"
}
$sourceFingerprint = Get-FileFingerprint $sourcePath
$expectedSha = ([string]$sourceConfig.expected_sha256).ToLowerInvariant()
if ($sourceFingerprint.sha256 -ne $expectedSha) {
    throw "source_sha256_mismatch"
}

$inputCopy = Join-Path $privateDir "input.pub"
Copy-Item -LiteralPath $sourcePath -Destination $inputCopy
$copyFingerprint = Get-FileFingerprint $inputCopy
if ($copyFingerprint.sha256 -ne $sourceFingerprint.sha256 -or $copyFingerprint.byte_len -ne $sourceFingerprint.byte_len) {
    throw "source_copy_binding_failed"
}

$policy = Get-OptionalProperty $manifest "policy"
$visible = [bool](Get-OptionalProperty $policy "visible" $false)
$graceSeconds = [int](Get-OptionalProperty $policy "process_exit_grace_seconds" 60)
$operation = $manifest.operation
$operationKind = [string]$operation.kind
$attribution = Get-OptionalProperty $manifest "attribution"
$officeArtStream = [string](Get-OptionalProperty $attribution "officeart_stream" "")

$receipt = [ordered]@{
    schema = $ReceiptSchema
    experiment_id = [string]$manifest.experiment_id
    status = "started"
    publisher = $null
    source = [ordered]@{
        sha256 = $sourceFingerprint.sha256
        byte_len = $sourceFingerprint.byte_len
    }
    writer = [ordered]@{
        name = "current"
        save_as_format = $PbFilePublication
    }
    operation = [ordered]@{
        kind = $operationKind
        selector = $null
        delta_degrees = $null
    }
    automation_security = [ordered]@{
        before = $null
        selected = $MsoAutomationSecurityForceDisable
        restore = $null
    }
    before = $null
    mutation = [ordered]@{ state = "not_applicable" }
    after = $null
    save = [ordered]@{ state = "not_applicable" }
    reopen = [ordered]@{ state = "not_applicable" }
    process_exit = [ordered]@{
        grace_seconds = $graceSeconds
        primary = "not_attempted"
        reopen = "not_applicable"
    }
    invariants = [ordered]@{
        source_copy_hash_equal = $true
        source_path_emitted = $false
        raw_document_bytes_emitted = $false
        raw_text_emitted = $false
        preflight_required_mspub_zero = $true
        publisher_process_kill_used = $false
    }
}

$script:NativeStageSequence = 0
$script:NativeStageReceiptPath = Join-Path $evidenceDir "native-stage-receipt.json"
$script:NativeExperimentId = [string]$manifest.experiment_id
$script:NativeOperationKind = $operationKind
Set-NativeStage "transaction_begin"

$application = $null
$document = $null
$securityBefore = $null
$target = $null
$mutatedPath = Join-Path $privateDir "mutated.pub"

try {
    Set-NativeStage "primary_application_create_begin"
    $appState = New-SafePublisherApplication -Visible:$visible
    $application = $appState.application
    Set-NativeStage "primary_application_create_complete"

    $securityBefore = $appState.automation_security_before
    $receipt.automation_security.before = $securityBefore
    $receipt.publisher = [ordered]@{
        version = Get-SafeValue { [string]$application.Version } "Application.Version"
        build = Get-SafeValue { [string]$application.Build } "Application.Build"
    }

    if ($operationKind -eq "snapshot_only") {
        Set-NativeStage "primary_open_begin"
        $document = $application.Open($inputCopy, $true, $false)
        Set-NativeStage "primary_open_complete"

        Set-NativeStage "primary_inventory_begin"
        $receipt.before = Get-DocumentInventory $document
        Set-NativeStage "primary_inventory_complete"
        $receipt.status = "complete"
    }
    else {
        $selector = $operation.selector
        $wantedPageId = [int]$selector.page_id
        $wantedShapeId = [int]$selector.shape_id
        $delta = [double]$operation.delta_degrees
        $receipt.operation.selector = [ordered]@{
            page_id = $wantedPageId
            shape_id = $wantedShapeId
        }
        $receipt.operation.delta_degrees = $delta

        Set-NativeStage "primary_open_begin"
        $document = $application.Open($inputCopy, $false, $false)
        Set-NativeStage "primary_open_complete"

        Set-NativeStage "primary_selector_begin"
        $target = Find-TargetShape -Document $document -WantedPageId $wantedPageId -WantedShapeId $wantedShapeId
        if ($null -eq $target) {
            Set-NativeStage "primary_selector_not_found"
            throw "selector_not_found"
        }
        Set-NativeStage "primary_selector_complete"

        $receipt.before = Get-ShapeSnapshot -Shape $target.shape -PageId $wantedPageId -TraversalPath ([string]$target.traversal_path)
        $beforeRotation = [double]$target.shape.Rotation

        Set-NativeStage "primary_mutation_begin"
        $target.shape.Rotation = $beforeRotation + $delta
        $afterRotation = [double]$target.shape.Rotation
        Set-NativeStage "primary_mutation_complete"
        if ([math]::Abs($afterRotation - $beforeRotation) -lt 0.0000001) {
            throw "mutation_no_effect"
        }
        $receipt.mutation = [ordered]@{
            state = "ok"
            before_rotation_degrees = $beforeRotation
            after_rotation_degrees = $afterRotation
        }
        $receipt.after = Get-ShapeSnapshot -Shape $target.shape -PageId $wantedPageId -TraversalPath ([string]$target.traversal_path)

        Set-NativeStage "primary_save_as_begin"
        $document.SaveAs($mutatedPath, $PbFilePublication, $false)
        Set-NativeStage "primary_save_as_complete"
        $saved = Get-FileFingerprint $mutatedPath
        $receipt.save = [ordered]@{
            state = "ok"
            sha256 = $saved.sha256
            byte_len = $saved.byte_len
        }
        $receipt.status = "saved"
    }
}
finally {
    if ($null -ne $target) {
        Release-ComObject $target.shape
    }
    if ($null -ne $document) {
        Set-NativeStage "primary_document_close_begin"
        try {
            $document.Close()
            Set-NativeStage "primary_document_close_complete"
        }
        catch {
            Set-NativeStage "primary_document_close_error"
        }
        Release-ComObject $document
    }
    if ($null -ne $application) {
        Set-NativeStage "primary_application_quit_begin"
        $closed = Close-SafePublisherApplication -Application $application -AutomationSecurityBefore $securityBefore
        Set-NativeStage "primary_application_quit_complete"
        $receipt.automation_security.restore = $closed.security_restore
    }
}

Set-NativeStage "primary_process_exit_wait_begin"
$primaryExited = Wait-PublisherExit -GraceSeconds $graceSeconds
Set-NativeStage "primary_process_exit_wait_complete"
$receipt.process_exit.primary = if ($primaryExited) { "ok" } else { "timeout" }
if (-not $primaryExited) {
    $receipt.status = "process_exit_timeout"
}

if ($operationKind -eq "shape_rotation_delta" -and $receipt.save.state -eq "ok" -and $primaryExited) {
    Assert-PublisherIdle
    $reopenApplication = $null
    $reopenDocument = $null
    $reopenSecurityBefore = $null
    $reopenTarget = $null

    try {
        Set-NativeStage "reopen_application_create_begin"
        $reopenState = New-SafePublisherApplication -Visible:$visible
        $reopenApplication = $reopenState.application
        Set-NativeStage "reopen_application_create_complete"

        $reopenSecurityBefore = $reopenState.automation_security_before

        Set-NativeStage "reopen_open_begin"
        $reopenDocument = $reopenApplication.Open($mutatedPath, $true, $false)
        Set-NativeStage "reopen_open_complete"

        Set-NativeStage "reopen_selector_begin"
        $reopenTarget = Find-TargetShape -Document $reopenDocument -WantedPageId ([int]$operation.selector.page_id) -WantedShapeId ([int]$operation.selector.shape_id)
        if ($null -eq $reopenTarget) {
            Set-NativeStage "reopen_selector_not_found"
            $receipt.reopen = [ordered]@{
                state = "selector_unresolved"
            }
            $receipt.status = "semantic_reopen_unresolved"
        }
        else {
            Set-NativeStage "reopen_selector_complete"
            $receipt.reopen = [ordered]@{
                state = "ok"
                target = Get-ShapeSnapshot -Shape $reopenTarget.shape -PageId ([int]$operation.selector.page_id) -TraversalPath ([string]$reopenTarget.traversal_path)
            }
            $receipt.status = "complete"
        }
    }
    finally {
        if ($null -ne $reopenTarget) { Release-ComObject $reopenTarget.shape }
        if ($null -ne $reopenDocument) {
            Set-NativeStage "reopen_document_close_begin"
            try {
                $reopenDocument.Close()
                Set-NativeStage "reopen_document_close_complete"
            }
            catch {
                Set-NativeStage "reopen_document_close_error"
            }
            Release-ComObject $reopenDocument
        }
        if ($null -ne $reopenApplication) {
            Set-NativeStage "reopen_application_quit_begin"
            [void](Close-SafePublisherApplication -Application $reopenApplication -AutomationSecurityBefore $reopenSecurityBefore)
            Set-NativeStage "reopen_application_quit_complete"
        }
    }

    Set-NativeStage "reopen_process_exit_wait_begin"
    $reopenExited = Wait-PublisherExit -GraceSeconds $graceSeconds
    Set-NativeStage "reopen_process_exit_wait_complete"
    $receipt.process_exit.reopen = if ($reopenExited) { "ok" } else { "timeout" }
    if (-not $reopenExited) {
        $receipt.status = "process_exit_timeout"
    }
}

Set-NativeStage "transaction_complete"
$nativeReceiptPath = Join-Path $evidenceDir "native-receipt.json"
Write-Json -Value $receipt -Path $nativeReceiptPath

$control = [ordered]@{
    has_diff = $false
    officeart_stream = $officeArtStream
    evidence_dir = "evidence"
}
if ($operationKind -eq "shape_rotation_delta" -and $receipt.save.state -eq "ok") {
    $diffManifest = [ordered]@{
        schema = $DiffSchema
        experiment_id = ([string]$manifest.experiment_id) + "--native-diff"
        question = "Which logical CFB ranges changed after the bounded native Publisher operation?"
        before = [ordered]@{
            path = "input.pub"
            expected_sha256 = $sourceFingerprint.sha256
        }
        after = [ordered]@{
            path = "mutated.pub"
            expected_sha256 = [string]$receipt.save.sha256
        }
        policy = [ordered]@{
            max_stream_bytes = 67108864
            max_changed_ranges_per_stream = 256
        }
    }
    Write-Json -Value $diffManifest -Path (Join-Path $privateDir "diff-manifest.json")
    $control.has_diff = $true
}
Write-Json -Value $control -Path (Join-Path $privateDir "control.json")

Write-Host ("PUB_RE_NATIVE status={0} operation={1}" -f $receipt.status, $operationKind)
