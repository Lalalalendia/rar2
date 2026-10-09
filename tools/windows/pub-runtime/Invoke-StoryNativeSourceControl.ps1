param(
    [Parameter(Mandatory = $true)][string]$BundleDir,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [ValidateRange(60, 600)][int]$TimeoutSeconds = 240
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'PubRuntime.psm1') -Force

if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force -ErrorAction Stop).Count -gt 0) {
        throw 'source_control_output_root_not_empty'
    }
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$root = (Resolve-Path -LiteralPath $OutputRoot).Path
$stageRoot = Join-Path $root 'phases'
New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null
$statusPath = Join-Path $root 'source-control-status.json'
$stdoutPath = Join-Path $root 'source-child.stdout.private.txt'
$stderrPath = Join-Path $root 'source-child.stderr.private.txt'
$savePath = Join-Path $root 'source-control-saveas.pub'
$phaseOrder = @(
    'source_verified', 'publisher_identity_begin', 'publisher_identity_ok',
    'source_application_begin', 'source_application_ready',
    'source_open_begin', 'source_open_ok',
    'source_saveas_begin', 'source_saveas_ok',
    'source_application_closed', 'native_file_verified',
    'native_reopen_application_begin', 'native_reopen_begin', 'native_reopen_ok',
    'source_immutable', 'pass'
)
$status = [ordered]@{
    schema = 'chaptera.pub-native-saveas-source-control.v1'
    code_commit_sha = [string]$env:GITHUB_SHA
    github_run_id = [string]$env:GITHUB_RUN_ID
    result = 'not_evaluated'
    failure_code = $null
    last_phase = $null
    source_sha256 = $null
    native_source_saveas_control = $false
    source_safe = $true
}
$child = $null
$startedUtc = [DateTime]::UtcNow
try {
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -gt 0) {
        $status.failure_code = 'publisher_busy_before_control'
        throw 'publisher_busy_before_control'
    }
    $bundle = (Resolve-Path -LiteralPath $BundleDir).Path
    $sourcePath = Join-Path $bundle 'source.pub'
    $manifestPath = Join-Path $bundle 'handoff.json'
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        $status.failure_code = 'missing_source_or_manifest'
        throw 'missing_source_or_manifest'
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $expectedHash = [string]$manifest.source_sha256
    if ([string]$manifest.schema -ne 'chaptera.pub-native-story-handoff.v1' -or
        $expectedHash -cnotmatch '^[0-9a-f]{64}$') {
        $status.failure_code = 'invalid_source_manifest'
        throw 'invalid_source_manifest'
    }
    $record = Get-PubFileRecord $sourcePath
    if ($record.sha256 -cne $expectedHash) {
        $status.failure_code = 'source_manifest_sha_mismatch'
        throw 'source_manifest_sha_mismatch'
    }
    $status.source_sha256 = $expectedHash
    $env:CHAPTERA_NATIVE_SOURCE_CONTROL_CHILD = Join-Path $PSScriptRoot 'Test-StoryNativeSourceSaveAs.ps1'
    $env:CHAPTERA_NATIVE_SOURCE_CONTROL_SOURCE = $sourcePath
    $env:CHAPTERA_NATIVE_SOURCE_CONTROL_SAVE = $savePath
    $env:CHAPTERA_NATIVE_SOURCE_CONTROL_STAGES = $stageRoot
    $command = @'
$ErrorActionPreference = 'Stop'
& $env:CHAPTERA_NATIVE_SOURCE_CONTROL_CHILD -SourcePath $env:CHAPTERA_NATIVE_SOURCE_CONTROL_SOURCE -SavePath $env:CHAPTERA_NATIVE_SOURCE_CONTROL_SAVE -StageRoot $env:CHAPTERA_NATIVE_SOURCE_CONTROL_STAGES
if (-not $?) { exit 77 }
'@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $powershellExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $child = Start-Process -FilePath $powershellExe -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded
    ) -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
    $status.result = 'inconclusive'
    if (-not $child.WaitForExit($TimeoutSeconds * 1000)) {
        $status.failure_code = 'source_control_child_timeout'
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop } catch {}
            try { [void]$child.WaitForExit(10000) } catch {}
        }
        $owned = @(
            Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
                try { $_.StartTime.ToUniversalTime() -ge $startedUtc.AddSeconds(-2) }
                catch { $false }
            }
        )
        foreach ($process in $owned) {
            try { Stop-Process -Id $process.Id -Force -ErrorAction Stop } catch {}
        }
        throw 'source_control_child_timeout'
    }
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) {
        $status.failure_code = 'source_control_child_failed'
        throw 'source_control_child_failed'
    }
    if (-not (Test-Path -LiteralPath $savePath -PathType Leaf)) {
        $status.failure_code = 'source_control_saveas_missing'
        throw 'source_control_saveas_missing'
    }
    if ((Get-PubFileRecord $sourcePath).sha256 -cne $expectedHash) {
        $status.failure_code = 'source_control_modified_input'
        throw 'source_control_modified_input'
    }
    $status.result = 'pass'
    $status.native_source_saveas_control = $true
}
catch {
    if ($null -eq $status.failure_code) { $status.failure_code = 'source_control_infrastructure_error' }
    Write-Warning ('Source SaveAs control did not pass: {0}' -f $status.failure_code)
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
    $status | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $statusPath -Encoding utf8
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
}
if ($status.result -ne 'pass') {
    throw ('native_source_saveas_control_not_passed: {0}' -f $status.failure_code)
}
Write-Host 'Native original-PUB SaveAs + reopen control PASS'
