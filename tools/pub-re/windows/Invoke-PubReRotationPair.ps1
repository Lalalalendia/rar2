param(
    [Parameter(Mandatory = $true)][string]$SourcePath,
    [Parameter(Mandatory = $true)][string]$ExpectedSha256,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [Parameter(Mandatory = $true)][int]$PageId,
    [Parameter(Mandatory = $true)][int]$ShapeId,
    [Parameter(Mandatory = $true)][double]$DeltaDegrees
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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

function Get-Fingerprint {
    param([Parameter(Mandatory = $true)][string]$Path)
    $item = Get-Item -LiteralPath $Path
    return [ordered]@{
        sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
        byte_len = [int64]$item.Length
    }
}

function Assert-CompleteReceipt {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedOperation
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "rotation_pair_native_receipt_missing:$ExpectedOperation"
    }
    $receipt = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    if ([string]$receipt.schema -ne "chaptera.pub-re-native-receipt.v1") {
        throw "rotation_pair_native_receipt_schema:$ExpectedOperation"
    }
    if ([string]$receipt.status -ne "complete") {
        throw "rotation_pair_native_receipt_incomplete:$ExpectedOperation"
    }
    if ([string]$receipt.operation.kind -ne $ExpectedOperation) {
        throw "rotation_pair_native_receipt_operation:$ExpectedOperation"
    }
    if ([string]$receipt.save.state -ne "ok") {
        throw "rotation_pair_native_save_incomplete:$ExpectedOperation"
    }
    if ([string]$receipt.process_exit.primary -ne "ok" -or [string]$receipt.process_exit.reopen -ne "ok") {
        throw "rotation_pair_native_process_exit:$ExpectedOperation"
    }
    return $receipt
}

if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) {
    throw "rotation_pair_source_missing"
}
$ExpectedSha256 = $ExpectedSha256.ToLowerInvariant()
if ($ExpectedSha256 -notmatch '^[0-9a-f]{64}$') {
    throw "rotation_pair_expected_sha_invalid"
}
$source = Get-Fingerprint $SourcePath
if ([string]$source.sha256 -ne $ExpectedSha256) {
    throw "rotation_pair_source_sha_mismatch"
}
if ($PageId -le 0 -or $ShapeId -le 0) {
    throw "rotation_pair_selector_invalid"
}
if ([double]::IsNaN($DeltaDegrees) -or [double]::IsInfinity($DeltaDegrees) -or
    [math]::Abs($DeltaDegrees) -lt 0.000001 -or [math]::Abs($DeltaDegrees) -gt 45.0) {
    throw "rotation_pair_delta_invalid"
}
if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
    throw "publisher_busy_before_rotation_pair"
}

New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$private = Join-Path $OutputRoot "pair-private"
$evidence = Join-Path $OutputRoot "evidence"
$controlRoot = Join-Path $OutputRoot "pair-control"
$mutationRoot = Join-Path $OutputRoot "pair-mutation"
New-Item -ItemType Directory -Force -Path $private | Out-Null
New-Item -ItemType Directory -Force -Path $evidence | Out-Null

$harness = Join-Path $PSScriptRoot "Invoke-PubReNativeExperiment.ps1"
if (-not (Test-Path -LiteralPath $harness -PathType Leaf)) {
    throw "rotation_pair_generic_harness_missing"
}

$common = [ordered]@{
    schema = "chaptera.pub-re-native-experiment.v1"
    source = [ordered]@{
        path = (Resolve-Path -LiteralPath $SourcePath).Path
        expected_sha256 = $ExpectedSha256
    }
    policy = [ordered]@{
        writer = "current"
        visible = $false
        process_exit_grace_seconds = 60
    }
    attribution = [ordered]@{
        officeart_stream = "/Escher/EscherStm"
    }
}

$controlManifest = [ordered]@{
    schema = $common.schema
    experiment_id = "rotation-pair-control"
    source = $common.source
    operation = [ordered]@{ kind = "save_in_place_noop" }
    policy = $common.policy
    attribution = $common.attribution
}
$mutationManifest = [ordered]@{
    schema = $common.schema
    experiment_id = "rotation-pair-mutation"
    source = $common.source
    operation = [ordered]@{
        kind = "shape_rotation_delta_in_place_save"
        selector = [ordered]@{
            page_id = $PageId
            shape_id = $ShapeId
        }
        delta_degrees = $DeltaDegrees
    }
    policy = $common.policy
    attribution = $common.attribution
}

$controlManifestPath = Join-Path $private "control-manifest.json"
$mutationManifestPath = Join-Path $private "mutation-manifest.json"
Write-Json -Value $controlManifest -Path $controlManifestPath
Write-Json -Value $mutationManifest -Path $mutationManifestPath

Write-Host "PUB_RE_ROTATION_PAIR arm=control begin"
& $harness -ManifestPath $controlManifestPath -OutputRoot $controlRoot
if (-not $?) { throw "rotation_pair_control_failed" }
Write-Host "PUB_RE_ROTATION_PAIR arm=control complete"

Write-Host "PUB_RE_ROTATION_PAIR arm=mutation begin"
& $harness -ManifestPath $mutationManifestPath -OutputRoot $mutationRoot
if (-not $?) { throw "rotation_pair_mutation_failed" }
Write-Host "PUB_RE_ROTATION_PAIR arm=mutation complete"

$controlReceiptPath = Join-Path $controlRoot "evidence\native-receipt.json"
$mutationReceiptPath = Join-Path $mutationRoot "evidence\native-receipt.json"
$controlReceipt = Assert-CompleteReceipt -Path $controlReceiptPath -ExpectedOperation "save_in_place_noop"
$mutationReceipt = Assert-CompleteReceipt -Path $mutationReceiptPath -ExpectedOperation "shape_rotation_delta_in_place_save"

if ([int]$mutationReceipt.operation.selector.page_id -ne $PageId -or
    [int]$mutationReceipt.operation.selector.shape_id -ne $ShapeId) {
    throw "rotation_pair_selector_receipt_mismatch"
}
if ([math]::Abs([double]$mutationReceipt.operation.delta_degrees - $DeltaDegrees) -gt 0.0000001) {
    throw "rotation_pair_delta_receipt_mismatch"
}
if ([string]$mutationReceipt.reopen.state -ne "ok") {
    throw "rotation_pair_mutation_reopen_unresolved"
}

$controlPub = Join-Path $controlRoot "private\mutated.pub"
$mutationPub = Join-Path $mutationRoot "private\mutated.pub"
if (-not (Test-Path -LiteralPath $controlPub -PathType Leaf) -or
    -not (Test-Path -LiteralPath $mutationPub -PathType Leaf)) {
    throw "rotation_pair_private_output_missing"
}
$controlFile = Get-Fingerprint $controlPub
$mutationFile = Get-Fingerprint $mutationPub
if ([string]$controlFile.sha256 -ne [string]$controlReceipt.save.sha256 -or
    [string]$mutationFile.sha256 -ne [string]$mutationReceipt.save.sha256) {
    throw "rotation_pair_private_output_binding_failed"
}

Copy-Item -LiteralPath $controlReceiptPath -Destination (Join-Path $evidence "control-native-receipt.json") -Force
Copy-Item -LiteralPath $mutationReceiptPath -Destination (Join-Path $evidence "mutation-native-receipt.json") -Force

$summary = [ordered]@{
    schema = "chaptera.pub-re-rotation-pair.v1"
    source = $source
    selector = [ordered]@{
        page_id = $PageId
        shape_id = $ShapeId
    }
    delta_degrees = $DeltaDegrees
    control = [ordered]@{
        operation = "save_in_place_noop"
        sha256 = [string]$controlFile.sha256
        byte_len = [int64]$controlFile.byte_len
        status = [string]$controlReceipt.status
    }
    mutation = [ordered]@{
        operation = "shape_rotation_delta_in_place_save"
        sha256 = [string]$mutationFile.sha256
        byte_len = [int64]$mutationFile.byte_len
        status = [string]$mutationReceipt.status
        before_rotation_degrees = [double]$mutationReceipt.mutation.before_rotation_degrees
        after_rotation_degrees = [double]$mutationReceipt.mutation.after_rotation_degrees
        reopen_rotation_degrees = [double]$mutationReceipt.reopen.target.rotation_degrees.value
    }
    invariants = [ordered]@{
        same_source_sha = $true
        matched_noop_control = $true
        both_fresh_reopen_complete = $true
        source_pub_immutable = $true
        public_receipt_contains_raw_document_bytes = $false
        native_pub_writer_capability_granted = $false
    }
}
Write-Json -Value $summary -Path (Join-Path $evidence "rotation-pair-summary.json")

$privateControl = [ordered]@{
    source_path = (Resolve-Path -LiteralPath $SourcePath).Path
    control_path = (Resolve-Path -LiteralPath $controlPub).Path
    mutation_path = (Resolve-Path -LiteralPath $mutationPub).Path
    source_sha256 = [string]$source.sha256
    control_sha256 = [string]$controlFile.sha256
    mutation_sha256 = [string]$mutationFile.sha256
}
Write-Json -Value $privateControl -Path (Join-Path $private "pair-paths.json")

Write-Host ("PUB_RE_ROTATION_PAIR status=complete control_sha={0} mutation_sha={1}" -f $controlFile.sha256, $mutationFile.sha256)
