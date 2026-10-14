# Research-only WinPS 5.1 guarded Save() on a disposable exact-SHA PUB copy.
param(
    [Parameter(Mandatory = $true)][string]$BundleDir,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [Parameter(Mandatory = $true)][ValidateSet('Original', 'Candidate')][string]$Arm,
    [ValidateRange(60, 600)][int]$TimeoutSeconds = 360
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'PubRuntime.psm1') -Force

if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop).Count -gt 0) {
        throw 'save_copy_output_root_not_empty'
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$root = (Resolve-Path -LiteralPath $OutputRoot).Path
$stageRoot = Join-Path $root 'phases'
New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null
$statusPath = Join-Path $root 'save-copy-status.json'
$receiptPath = Join-Path $root 'save-copy-receipt.json'
$workingPath = Join-Path $root 'working-copy.pub'
$stdoutPath = Join-Path $root 'save-child.stdout.private.txt'
$stderrPath = Join-Path $root 'save-child.stderr.private.txt'
$expectedSource = '424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc'
$expectedCandidate = 'a92543b6f2b6ac3a8ae2481e15a2188a338ddc2a92832580f8987079fa4f70f8'
$phaseOrder = @(
    'work_copy_verified', 'publisher_identity_begin', 'publisher_identity_ok',
    'application_begin', 'application_ready', 'open_begin', 'open_ok',
    'save_begin', 'save_ok', 'application_closed', 'saved_file_verified',
    'reopen_application_begin', 'reopen_begin', 'reopen_ok', 'reopen_application_closed',
    'reader_begin', 'reader_ok', 'input_immutable', 'pass'
)
$status = [ordered]@{
    schema = 'chaptera.pub-native-story-save-copy-status.v1'
    arm = $Arm.ToLowerInvariant()
    code_commit_sha = [string]$env:GITHUB_SHA
    github_run_id = [string]$env:GITHUB_RUN_ID
    result = 'not_evaluated'
    failure_code = $null
    last_phase = $null
    source_sha256 = $expectedSource
    candidate_sha256 = $expectedCandidate
    native_saved_sha256 = $null
    native_save_copy_research_pass = $false
    saveas_evaluated = $false
    native_publisher_acceptance = $false
    product_save_authorized = $false
    source_safe = $true
}
$child = $null
$startedUtc = [DateTime]::UtcNow
try {
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -gt 0) {
        $status.failure_code = 'publisher_busy_before_save_copy'
        throw 'publisher_busy_before_save_copy'
    }
    $bundle = (Resolve-Path -LiteralPath $BundleDir).Path
    $manifestPath = Join-Path $bundle 'handoff.json'
    $sourcePath = Join-Path $bundle 'source.pub'
    $candidatePath = Join-Path $bundle 'candidate.pub'
    foreach ($item in @($manifestPath, $sourcePath, $candidatePath)) {
        if (-not (Test-Path -LiteralPath $item -PathType Leaf)) {
            $status.failure_code = 'save_copy_bundle_missing_input'
            throw 'save_copy_bundle_missing_input'
        }
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([string]$manifest.schema -cne 'chaptera.pub-native-story-handoff.v1' -or
        [string]$manifest.source_sha256 -cne $expectedSource -or
        [string]$manifest.candidate_sha256 -cne $expectedCandidate) {
        $status.failure_code = 'save_copy_manifest_identity_mismatch'
        throw 'save_copy_manifest_identity_mismatch'
    }
    if ((Get-PubFileRecord $sourcePath).sha256 -cne $expectedSource -or
        (Get-PubFileRecord $candidatePath).sha256 -cne $expectedCandidate) {
        $status.failure_code = 'save_copy_pinned_sha_mismatch'
        throw 'save_copy_pinned_sha_mismatch'
    }
    $selected = if ($Arm -eq 'Original') { $sourcePath } else { $candidatePath }
    $expectedWorking = if ($Arm -eq 'Original') { $expectedSource } else { $expectedCandidate }
    Copy-Item -LiteralPath $selected -Destination $workingPath -ErrorAction Stop
    $workingBefore = Get-PubFileRecord $workingPath
    if ($workingBefore.sha256 -cne $expectedWorking -or $workingBefore.size -ne 72192) {
        $status.failure_code = 'save_copy_working_sha_mismatch'
        throw 'save_copy_working_sha_mismatch'
    }

    $env:CHAPTERA_SAVE_COPY_CHILD = Join-Path $PSScriptRoot 'Test-StoryNativeSaveCopy.ps1'
    $env:CHAPTERA_SAVE_COPY_BUNDLE = $bundle
    $env:CHAPTERA_SAVE_COPY_WORKING = $workingPath
    $env:CHAPTERA_SAVE_COPY_STAGES = $stageRoot
    $env:CHAPTERA_SAVE_COPY_RECEIPT = $receiptPath
    $env:CHAPTERA_SAVE_COPY_ARM = $Arm
    $command = @'
$ErrorActionPreference = 'Stop'
try {
    & $env:CHAPTERA_SAVE_COPY_CHILD -BundleDir $env:CHAPTERA_SAVE_COPY_BUNDLE -WorkingPath $env:CHAPTERA_SAVE_COPY_WORKING -StageRoot $env:CHAPTERA_SAVE_COPY_STAGES -ReceiptPath $env:CHAPTERA_SAVE_COPY_RECEIPT -Arm $env:CHAPTERA_SAVE_COPY_ARM
    exit 0
}
catch {
    exit 77
}
'@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $powershellExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $child = Start-Process -FilePath $powershellExe -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded
    ) -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
    $status.result = 'inconclusive'
    if (-not $child.WaitForExit($TimeoutSeconds * 1000)) {
        $status.failure_code = 'save_copy_child_timeout'
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop } catch {}
            try { [void]$child.WaitForExit(10000) } catch {}
        }
        # No MSPUB existed at entry; clean only experiment-started processes.
        $owned = @(
            Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
                try { $_.StartTime.ToUniversalTime() -ge $startedUtc.AddSeconds(-2) }
                catch { $false }
            }
        )
        foreach ($process in $owned) {
            try { Stop-Process -Id $process.Id -Force -ErrorAction Stop } catch {}
        }
        throw 'save_copy_child_timeout'
    }
    $child.WaitForExit()
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) {
        $status.failure_code = 'save_copy_child_failed'
        throw 'save_copy_child_failed'
    }
    if (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
        $status.failure_code = 'save_copy_receipt_missing'
        throw 'save_copy_receipt_missing'
    }
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    $checks = @(
        ([string]$receipt.schema -ceq 'chaptera.pub-native-story-save-copy-research.v1'),
        ([string]$receipt.arm -ceq $Arm.ToLowerInvariant()),
        ([string]$receipt.source_sha256 -ceq $expectedSource),
        ([string]$receipt.candidate_sha256 -ceq $expectedCandidate),
        ([string]$receipt.working_before_sha256 -ceq $expectedWorking),
        ([string]$receipt.publisher_version -ceq '16.0'),
        ([string]$receipt.publisher_build -like '12527*'),
        ($receipt.save_completed -eq $true),
        ($receipt.fresh_reopen -eq $true),
        ($receipt.original_inputs_unchanged -eq $true),
        ($receipt.saveas_evaluated -eq $false),
        ($receipt.product_save_authorized -eq $false),
        ([string]$receipt.native_saved_sha256 -cmatch '^[0-9a-f]{64}$'),
        ([int64]$receipt.native_saved_bytes -gt 0),
        ([int]$receipt.initial_snapshot.pages -eq [int]$receipt.reopen_snapshot.pages),
        ([int]$receipt.initial_snapshot.shapes -eq [int]$receipt.reopen_snapshot.shapes),
        ((Get-PubFileRecord $sourcePath).sha256 -ceq $expectedSource),
        ((Get-PubFileRecord $candidatePath).sha256 -ceq $expectedCandidate)
    )
    if ($Arm -eq 'Candidate') {
        $checks += ($receipt.current_reader_verified -eq $true)
        $checks += ([string]$receipt.semantic_story_sha256 -ceq [string]$manifest.after_text_sha256)
    }
    else {
        $checks += ($receipt.current_reader_verified -eq $false)
    }
    if ($checks -contains $false) {
        $status.failure_code = 'save_copy_receipt_contract_failed'
        throw 'save_copy_receipt_contract_failed'
    }
    $status.result = 'pass'
    $status.native_saved_sha256 = [string]$receipt.native_saved_sha256
    $status.native_save_copy_research_pass = $true
}
catch {
    if ($null -eq $status.failure_code) { $status.failure_code = 'save_copy_environment_or_script_error' }
    Write-Warning ('Publisher research Save copy did not pass: {0}' -f $status.failure_code)
}
finally {
    for ($index = $phaseOrder.Count - 1; $index -ge 0; $index--) {
        $phase = [string]$phaseOrder[$index]
        $path = Join-Path $stageRoot ('phase-{0:D2}-{1}.marker' -f $index, $phase)
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $status.last_phase = $phase
            break
        }
    }
    $status | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $statusPath -Encoding utf8
    Remove-Item -LiteralPath @($stdoutPath, $stderrPath) -Force -ErrorAction SilentlyContinue
}
if ($status.result -ne 'pass') {
    throw ('publisher_research_save_copy_not_passed: {0}' -f $status.failure_code)
}
Write-Host ('Publisher research-only Save(copy) + reopen PASS, arm={0}' -f $Arm)
