param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "FALSE-OMISSION-01"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_PUBLISHER_EXE)) {
    throw "PUB_RESEARCH_PUBLISHER_EXE was not resolved by prepare_native_run.ps1."
}
$publisherHash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_PUBLISHER_EXE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($publisherHash -ne $ExpectedPublisherExeSha256) {
    throw "Publisher executable SHA-256 mismatch: $publisherHash"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/false-omission-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Get-FileReceipt {
    param([Parameter(Mandatory = $true)][string]$Path)
    $item = Get-Item -LiteralPath $Path
    return [ordered]@{
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        size = [int64]$item.Length
    }
}

function Build-DumpTool {
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
            & cargo build --locked --offline --release --target-dir $targetDir --manifest-path $manifestPath -p pub-reader --bin false_omission_dump
            if ($LASTEXITCODE -ne 0) {
                throw "false_omission_dump offline build failed with exit code $LASTEXITCODE"
            }
        }
        finally {
            Pop-Location
        }

        $exe = Join-Path $targetDir "release/false_omission_dump.exe"
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
            throw "false_omission_dump.exe missing after offline private-target build"
        }

        return [pscustomobject]@{
            path = $exe
            sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
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

function Invoke-Dump {
    param(
        [Parameter(Mandatory = $true)][string]$Tool,
        [Parameter(Mandatory = $true)][string]$PubPath,
        [Parameter(Mandatory = $true)][string]$ContentsPath,
        [Parameter(Mandatory = $true)][string]$EscherPath
    )
    & $Tool $PubPath $ContentsPath $EscherPath
    if ($LASTEXITCODE -ne 0) {
        throw "false_omission_dump failed with exit code $LASTEXITCODE"
    }
    foreach ($path in @($ContentsPath,$EscherPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "false_omission_dump did not emit $path"
        }
    }
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$Role,
        [Parameter(Mandatory = $true)][string]$CaseId,
        [Parameter(Mandatory = $true)][string]$SourcePub
    )

    $armRoot = Join-Path $privateDir $Role
    $outputDir = Join-Path $armRoot "output"
    $oracleDir = Join-Path $armRoot "oracle"
    New-Item -ItemType Directory -Force -Path $outputDir,$oracleDir | Out-Null

    $contextPath = Join-Path $armRoot "run-context.json"
    [ordered]@{
        experiment_id = $ExpectedExperiment
        case_id = $CaseId
        source_pub = $SourcePub
        output_dir = $outputDir
        oracle_dir = $oracleDir
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $contextPath -Encoding UTF8

    $adapter = Join-Path $repoRoot "tools/windows/pub-runtime/adapters/Invoke-FalseOmission01Adapter.ps1"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $adapter -RunContextPath $contextPath
    if ($LASTEXITCODE -ne 0) {
        throw "FALSE-OMISSION adapter failed for $Role with exit code $LASTEXITCODE"
    }

    $runtimePath = Join-Path $oracleDir "false-omission.json"
    if (-not (Test-Path -LiteralPath $runtimePath -PathType Leaf)) {
        throw "FALSE-OMISSION adapter did not emit runtime oracle for $Role"
    }
    $runtime = Get-Content -LiteralPath $runtimePath -Raw | ConvertFrom-Json
    $expectedSource = Get-FileReceipt -Path $SourcePub
    if ([string]$runtime.source_sha256 -ne [string]$expectedSource.sha256) {
        throw "$Role arm did not start from the exact source bytes"
    }
    if ([string]$runtime.save.state -ne "ok") {
        throw "$Role arm SaveAs failed"
    }
    if ([string]$runtime.reopen_validation.state -ne "ok") {
        throw "$Role arm fresh-reopen validation failed"
    }
    $outputPub = [string]$runtime.save.path
    if (-not (Test-Path -LiteralPath $outputPub -PathType Leaf)) {
        throw "$Role arm output PUB is missing"
    }

    return [pscustomobject]@{
        role = $Role
        case_id = $CaseId
        runtime_path = $runtimePath
        runtime = $runtime
        output_pub = $outputPub
    }
}

function Safe-StateValue {
    param($Record)
    if ($null -eq $Record) { return $null }
    if ([string]$Record.state -ne "value") { return $null }
    return $Record.value
}

$python = (Get-Command python -ErrorAction Stop).Source
$rawAuditTool = Join-Path $repoRoot "tools/false_omission_materialization_audit.py"
$identityAuditTool = Join-Path $repoRoot "tools/false_omission_com_escher_identity.py"
foreach ($tool in @($rawAuditTool,$identityAuditTool)) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) {
        throw "Required audit tool is missing: $tool"
    }
}

& $python $rawAuditTool --self-test
if ($LASTEXITCODE -ne 0) { throw "FALSE-OMISSION raw-audit self-test failed" }
& $python $identityAuditTool --self-test
if ($LASTEXITCODE -ne 0) { throw "FALSE-OMISSION identity-audit self-test failed" }

$sourcePub = Join-Path $privateDir "source.pub"
$generator = Join-Path $repoRoot "tools/windows/pub-runtime/New-FalseOmissionShapeFixture.ps1"
$generatorText = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $generator -OutputPath $sourcePub 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0) {
    throw "FALSE-OMISSION source fixture generation failed with exit code $LASTEXITCODE"
}
$generatorText | Set-Content -LiteralPath (Join-Path $privateDir "source-fixture-receipt.txt") -Encoding UTF8
if (-not (Test-Path -LiteralPath $sourcePub -PathType Leaf)) {
    throw "FALSE-OMISSION source fixture was not created"
}
$source = Get-FileReceipt -Path $sourcePub

$dumpTool = Build-DumpTool
$sourceContents = Join-Path $privateDir "source.contents.json"
$sourceEscher = Join-Path $privateDir "source.escher.json"
Invoke-Dump -Tool $dumpTool.path -PubPath $sourcePub -ContentsPath $sourceContents -EscherPath $sourceEscher

$control = Invoke-Arm -Role "control" -CaseId "resave-control--current" -SourcePub $sourcePub
$fill = Invoke-Arm -Role "fill" -CaseId "fill-visible-true--current" -SourcePub $sourcePub
$line = Invoke-Arm -Role "line" -CaseId "line-visible-true--current" -SourcePub $sourcePub

$armMap = [ordered]@{
    control = $control
    fill = $fill
    line = $line
}
foreach ($role in @("control","fill","line")) {
    $arm = $armMap[$role]
    $contentsPath = Join-Path $privateDir "$role.contents.json"
    $escherPath = Join-Path $privateDir "$role.escher.json"
    Invoke-Dump -Tool $dumpTool.path -PubPath $arm.output_pub -ContentsPath $contentsPath -EscherPath $escherPath
    $arm | Add-Member -NotePropertyName contents_path -NotePropertyValue $contentsPath
    $arm | Add-Member -NotePropertyName escher_path -NotePropertyValue $escherPath
}

$privateRawAudit = Join-Path $privateDir "false-omission-materialization.private.json"
$privateRawTsv = Join-Path $privateDir "false-omission-materialization.private.tsv"
& $python $rawAuditTool --source $sourceContents --control $control.contents_path --fill $fill.contents_path --line $line.contents_path --out-json $privateRawAudit --out-tsv $privateRawTsv
if ($LASTEXITCODE -ne 0) {
    throw "FALSE-OMISSION raw materialization audit failed"
}
$rawAudit = Get-Content -LiteralPath $privateRawAudit -Raw | ConvertFrom-Json
$rawAudit.summary.roles.source.path = "<private:source.contents.json>"
$rawAudit.summary.roles.control.path = "<private:control.contents.json>"
$rawAudit.summary.roles.fill.path = "<private:fill.contents.json>"
$rawAudit.summary.roles.line.path = "<private:line.contents.json>"
$publicRawAudit = Join-Path $analysisDir "false-omission-materialization.json"
Write-PubJson -Value $rawAudit -Path $publicRawAudit

$identityAudits = [ordered]@{}
foreach ($role in @("control","fill","line")) {
    $arm = $armMap[$role]
    $privateIdentity = Join-Path $privateDir "identity-$role.private.json"
    & $python $identityAuditTool --runtime-json $arm.runtime_path --escher-json $arm.escher_path --out-json $privateIdentity
    if ($LASTEXITCODE -ne 0) {
        throw "FALSE-OMISSION COM-Escher identity audit failed for $role"
    }
    $identity = Get-Content -LiteralPath $privateIdentity -Raw | ConvertFrom-Json
    $identityAudits[$role] = $identity
    Write-PubJson -Value $identity -Path (Join-Path $analysisDir "false-omission-identity-$role.json")
}

$runtimePass = $true
$roles = [ordered]@{}
foreach ($role in @("control","fill","line")) {
    $arm = $armMap[$role]
    $runtime = $arm.runtime
    $output = Get-FileReceipt -Path $arm.output_pub
    $roles[$role] = [ordered]@{
        case_id = [string]$runtime.case_id
        operation = [string]$runtime.operation
        source_sha256 = [string]$runtime.source_sha256
        output_sha256 = [string]$output.sha256
        output_size = [long]$output.size
        mutation_state = [string]$runtime.mutation.state
        save_state = [string]$runtime.save.state
        reopen_validation_state = [string]$runtime.reopen_validation.state
        before = [ordered]@{
            fill_visible = Safe-StateValue $runtime.before.fill_visible
            line_visible = Safe-StateValue $runtime.before.line_visible
            shape_id = Safe-StateValue $runtime.before.shape_id
        }
        after = [ordered]@{
            fill_visible = Safe-StateValue $runtime.after.fill_visible
            line_visible = Safe-StateValue $runtime.after.line_visible
            shape_id = Safe-StateValue $runtime.after.shape_id
        }
        reopen = [ordered]@{
            fill_visible = Safe-StateValue $runtime.reopen.fill_visible
            line_visible = Safe-StateValue $runtime.reopen.line_visible
            shape_id = Safe-StateValue $runtime.reopen.shape_id
        }
        candidate_com_escher_bridge = [bool]$identityAudits[$role].match.candidate_bridge
    }
    if (
        [string]$runtime.save.state -ne "ok" -or
        [string]$runtime.reopen_validation.state -ne "ok" -or
        -not [bool]$identityAudits[$role].match.candidate_bridge
    ) {
        $runtimePass = $false
    }
}

$strictFill = [bool]$rawAudit.summary.strict_fill_pattern
$strictLine = [bool]$rawAudit.summary.strict_line_pattern
$rawPass = $strictFill -and $strictLine
$verdict = if ($runtimePass -and $rawPass) {
    "strict-candidate-materialization-pattern"
}
else {
    "not-closed"
}

$result = [ordered]@{
    schema = "chaptera.false-omission-01/v1"
    experiment_id = $ExpectedExperiment
    publisher = [ordered]@{
        exe_sha256 = $publisherHash
        expected_version_prefix = "16.0.12527."
        runtime_version = $control.runtime.publisher.version
        runtime_build = $control.runtime.publisher.build
    }
    source = [ordered]@{
        sha256 = [string]$source.sha256
        size = [long]$source.size
        contract = [ordered]@{
            pages = 1
            top_level_shapes = 1
            oracle_tag = "FALSE_OMISSION_TARGET"
            fill_visible = 0
            line_visible = 0
            fill_rgb = "0x336699"
            line_rgb = "0x663399"
            left_points = 73
            top_points = 91
            width_points = 181
            height_points = 103
        }
    }
    tooling = [ordered]@{
        dump_helper_sha256 = [string]$dumpTool.sha256
        cargo_version = [string]$dumpTool.cargo_version
        cargo_lock_sha256 = [string]$dumpTool.cargo_lock_sha256
        cargo_lock_origin = [string]$dumpTool.cargo_lock_origin
        raw_audit_schema = [string]$rawAudit.schema
    }
    roles = $roles
    raw_materialization = [ordered]@{
        strict_fill_pattern = $strictFill
        strict_line_pattern = $strictLine
        strict_document_unique_patterns = [int]$rawAudit.summary.strict_document_unique_patterns
        fill_bridge = @($rawAudit.identity_bridges | Where-Object { [string]$_.style -eq "fill" })
        line_bridge = @($rawAudit.identity_bridges | Where-Object { [string]$_.style -eq "line" })
    }
    verdict = $verdict
    authority_boundary = "This receipt tests one Publisher16/build12527 current-writer fixture with three fresh arms from identical generated source bytes. A strict-candidate-materialization-pattern verdict requires fresh-reopen COM semantics, one-shape COM-to-Escher candidate bridges, and document-unique/stable Contents candidate patterns in which source/control omit the tested boolean while only the corresponding Fill.Visible or Line.Visible arm materializes it. It remains bounded to this fixture/build/operation and does not by itself establish universal FFilled/FLine writer rules, legacy 0x5A behavior, or generic COM-to-wire identity."
}
Write-PubJson -Value $result -Path (Join-Path $analysisDir "false-omission-01.json")

@(
    "experiment=$ExpectedExperiment",
    "source_sha256=$($result.source.sha256)",
    "source_size=$($result.source.size)",
    "control_output_sha256=$($result.roles.control.output_sha256)",
    "fill_output_sha256=$($result.roles.fill.output_sha256)",
    "line_output_sha256=$($result.roles.line.output_sha256)",
    "control_identity_bridge=$($result.roles.control.candidate_com_escher_bridge)",
    "fill_identity_bridge=$($result.roles.fill.candidate_com_escher_bridge)",
    "line_identity_bridge=$($result.roles.line.candidate_com_escher_bridge)",
    "strict_fill_pattern=$($result.raw_materialization.strict_fill_pattern)",
    "strict_line_pattern=$($result.raw_materialization.strict_line_pattern)",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "false-omission-01.txt") -Encoding ASCII
