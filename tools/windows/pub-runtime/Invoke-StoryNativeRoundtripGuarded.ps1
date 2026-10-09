param(
    [Parameter(Mandatory = $true)]
    [string]$BundleDir,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot,
    [ValidateRange(60, 600)]
    [int]$TimeoutSeconds = 360,
    [ValidateSet('WindowsPowerShell51', 'PowerShell7')]
    [string]$ChildPowerShell = 'WindowsPowerShell51'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$root = (Resolve-Path -LiteralPath $OutputRoot).Path
$statusPath = Join-Path $root 'native-roundtrip-status.json'
$receiptPath = Join-Path $root 'native-roundtrip-receipt.json'
$stdoutPath = Join-Path $root 'child.stdout.private.txt'
$stderrPath = Join-Path $root 'child.stderr.private.txt'

$status = [ordered]@{
    schema = 'chaptera.pub-native-story-runner-status.v1'
    source_commit_sha = [string]$env:GITHUB_SHA
    github_run_id = [string]$env:GITHUB_RUN_ID
    result = 'not_evaluated'
    failure_code = $null
    native_publisher_acceptance = $false
    source_safe = $true
}
$child = $null
$startedUtc = [DateTime]::UtcNow

try {
    $preexisting = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
    if ($preexisting.Count -gt 0) {
        $status.failure_code = 'publisher_busy_before_launch'
        throw 'publisher_busy_before_launch'
    }

    $bundle = (Resolve-Path -LiteralPath $BundleDir).Path
    $manifestPath = Join-Path $bundle 'handoff.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        $status.failure_code = 'missing_handoff_manifest'
        throw 'missing_handoff_manifest'
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([string]$manifest.schema -ne 'chaptera.pub-native-story-handoff.v1' -or
        [string]$manifest.candidate_sha256 -notmatch '^[0-9a-f]{64}$') {
        $status.failure_code = 'invalid_handoff_manifest'
        throw 'invalid_handoff_manifest'
    }

    $env:CHAPTERA_STORY_NATIVE_SCRIPT = Join-Path $PSScriptRoot 'run_story_native_roundtrip.ps1'
    $env:CHAPTERA_STORY_NATIVE_BUNDLE = $bundle
    $env:CHAPTERA_STORY_NATIVE_OUTPUT = $root
    $env:CHAPTERA_STORY_NATIVE_CHILD_SHELL = $ChildPowerShell
    $command = @'
$ErrorActionPreference = 'Stop'
try {
    if ($env:CHAPTERA_STORY_NATIVE_CHILD_SHELL -eq 'PowerShell7' -and
        ([string]$PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -ne 7)) {
        exit 78
    }
    & $env:CHAPTERA_STORY_NATIVE_SCRIPT -BundleDir $env:CHAPTERA_STORY_NATIVE_BUNDLE -OutputRoot $env:CHAPTERA_STORY_NATIVE_OUTPUT
    exit 0
}
catch {
    exit 77
}
'@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $powershellExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    if ($ChildPowerShell -eq 'PowerShell7') {
        # Controlled historical-runtime discriminator. Never resolve a runner PATH alias.
        $powershellExe = Join-Path ${env:ProgramFiles} 'PowerShell\7\pwsh.exe'
        if (-not (Test-Path -LiteralPath $powershellExe -PathType Leaf)) {
            $status.failure_code = 'pwsh7_executable_absent'
            throw 'pwsh7_executable_absent'
        }
        # Source-safe metadata only: no executable paths, user/profile names or raw COM logs.
        $probe = @(& $powershellExe -NoLogo -NoProfile -NonInteractive -Command '$v=$PSVersionTable.PSVersion.ToString();$e=[string]$PSVersionTable.PSEdition;$a=[System.Threading.Thread]::CurrentThread.GetApartmentState().ToString();$b=[int][Environment]::Is64BitProcess;Write-Output "$v|$e|$a|$b"')
        if ($LASTEXITCODE -ne 0 -or $probe.Count -ne 1) {
            $status.failure_code = 'pwsh7_preflight_failed'
            throw 'pwsh7_preflight_failed'
        }
        $probeText = [string]$probe[0]
        if ($probeText -notmatch '^7\.[0-9]+\.[0-9]+(\.[0-9]+)?\|Core\|(STA|MTA|Unknown)\|[01] -FilePath $powershellExe `
        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded) `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath -PassThru

    if (-not $child.WaitForExit($TimeoutSeconds * 1000)) {
        $status.failure_code = 'native_child_timeout'
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop } catch {}
            try { [void]$child.WaitForExit(10000) } catch {}
        }
        # No Publisher existed before launch. Only clean instances started by this job.
        $owned = @(
            Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
                try { $_.StartTime.ToUniversalTime() -ge $startedUtc.AddSeconds(-2) }
                catch { $false }
            }
        )
        foreach ($process in $owned) {
            try { Stop-Process -Id $process.Id -Force -ErrorAction Stop } catch {}
        }
        throw 'native_child_timeout'
    }
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) {
        $status.result = 'fail'
        $status.failure_code = 'native_roundtrip_script_failed'
        throw 'native_roundtrip_script_failed'
    }
    if (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
        $status.result = 'fail'
        $status.failure_code = 'missing_native_roundtrip_receipt'
        throw 'missing_native_roundtrip_receipt'
    }

    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    $checks = @(
        ([string]$receipt.schema -eq 'chaptera.pub-native-story-roundtrip-receipt.v1'),
        ($receipt.lifecycle.candidate_open -eq $true),
        ($receipt.lifecycle.save_as -eq $true),
        ($receipt.lifecycle.fresh_reopen -eq $true),
        ($receipt.preservation.source_pub_unchanged -eq $true),
        ($receipt.preservation.candidate_pub_unchanged -eq $true),
        ($receipt.semantic.verified_by_current_rar -eq $true),
        ([string]$receipt.handoff.candidate_sha256 -eq [string]$manifest.candidate_sha256),
        ([string]$receipt.semantic.story_text_sha256 -eq [string]$manifest.after_text_sha256),
        ([int]$receipt.semantic.story_utf16_len -eq [int]$manifest.after_utf16_len)
    )
    if ($checks -contains $false) {
        $status.result = 'fail'
        $status.failure_code = 'native_roundtrip_receipt_contract_failed'
        throw 'native_roundtrip_receipt_contract_failed'
    }

    $status.result = 'pass'
    $status.native_publisher_acceptance = $true
    $status.publisher_version = [string]$receipt.publisher.version
    $status.publisher_build = [string]$receipt.publisher.build
    $status.native_saved_sha256 = [string]$receipt.preservation.native_saved_sha256
}
catch {
    if ($null -eq $status.failure_code) { $status.failure_code = 'native_environment_or_script_error' }
    Write-Warning ("Publisher native Story experiment did not pass: {0}" -f $status.failure_code)
}
finally {
    $stagePath = Join-Path $root 'native-roundtrip-stage.json'
    if (Test-Path -LiteralPath $stagePath -PathType Leaf) {
        try {
            $stage = Get-Content -LiteralPath $stagePath -Raw | ConvertFrom-Json
            if ([string]$stage.schema -eq 'chaptera.pub-native-story-stage.v1' -and
                [string]$stage.phase -match '^[a-z][a-z0-9_]{0,63}$' -and
                [string]$stage.candidate_sha256 -match '^[0-9a-f]{64}$') {
                $status.last_native_phase = [string]$stage.phase
                $status.candidate_sha256 = [string]$stage.candidate_sha256
            }
            else {
                $status.stage_receipt_valid = $false
            }
        }
        catch { $status.stage_receipt_valid = $false }
    }
    $status | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $statusPath -Encoding utf8
    foreach ($path in @($stdoutPath, $stderrPath)) {
        Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    }
}
if ($status.result -ne 'pass') {
    throw ("publisher_native_story_not_passed: {0}" -f $status.failure_code)
}
Write-Host ("Publisher Story Open-SaveAs-Reopen and Reader verification PASS: {0}" -f $status.native_saved_sha256)
) {
            $status.failure_code = 'pwsh7_identity_mismatch'
            throw 'pwsh7_identity_mismatch'
        }
        $parts = $probeText.Split('|')
        $status.child_shell_mode = 'PowerShell7'
        $status.child_powershell_version = $parts[0]
        $status.child_apartment = $parts[2]
        $status.child_is_64_bit_process = ($parts[3] -eq '1')
    }
    $child = Start-Process -FilePath $powershellExe `
        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded) `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath -PassThru

    if (-not $child.WaitForExit($TimeoutSeconds * 1000)) {
        $status.failure_code = 'native_child_timeout'
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop } catch {}
            try { [void]$child.WaitForExit(10000) } catch {}
        }
        # No Publisher existed before launch. Only clean instances started by this job.
        $owned = @(
            Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
                try { $_.StartTime.ToUniversalTime() -ge $startedUtc.AddSeconds(-2) }
                catch { $false }
            }
        )
        foreach ($process in $owned) {
            try { Stop-Process -Id $process.Id -Force -ErrorAction Stop } catch {}
        }
        throw 'native_child_timeout'
    }
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) {
        $status.result = 'fail'
        $status.failure_code = 'native_roundtrip_script_failed'
        throw 'native_roundtrip_script_failed'
    }
    if (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
        $status.result = 'fail'
        $status.failure_code = 'missing_native_roundtrip_receipt'
        throw 'missing_native_roundtrip_receipt'
    }

    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    $checks = @(
        ([string]$receipt.schema -eq 'chaptera.pub-native-story-roundtrip-receipt.v1'),
        ($receipt.lifecycle.candidate_open -eq $true),
        ($receipt.lifecycle.save_as -eq $true),
        ($receipt.lifecycle.fresh_reopen -eq $true),
        ($receipt.preservation.source_pub_unchanged -eq $true),
        ($receipt.preservation.candidate_pub_unchanged -eq $true),
        ($receipt.semantic.verified_by_current_rar -eq $true),
        ([string]$receipt.handoff.candidate_sha256 -eq [string]$manifest.candidate_sha256),
        ([string]$receipt.semantic.story_text_sha256 -eq [string]$manifest.after_text_sha256),
        ([int]$receipt.semantic.story_utf16_len -eq [int]$manifest.after_utf16_len)
    )
    if ($checks -contains $false) {
        $status.result = 'fail'
        $status.failure_code = 'native_roundtrip_receipt_contract_failed'
        throw 'native_roundtrip_receipt_contract_failed'
    }

    $status.result = 'pass'
    $status.native_publisher_acceptance = $true
    $status.publisher_version = [string]$receipt.publisher.version
    $status.publisher_build = [string]$receipt.publisher.build
    $status.native_saved_sha256 = [string]$receipt.preservation.native_saved_sha256
}
catch {
    if ($null -eq $status.failure_code) { $status.failure_code = 'native_environment_or_script_error' }
    Write-Warning ("Publisher native Story experiment did not pass: {0}" -f $status.failure_code)
}
finally {
    $stagePath = Join-Path $root 'native-roundtrip-stage.json'
    if (Test-Path -LiteralPath $stagePath -PathType Leaf) {
        try {
            $stage = Get-Content -LiteralPath $stagePath -Raw | ConvertFrom-Json
            if ([string]$stage.schema -eq 'chaptera.pub-native-story-stage.v1' -and
                [string]$stage.phase -match '^[a-z][a-z0-9_]{0,63}$' -and
                [string]$stage.candidate_sha256 -match '^[0-9a-f]{64}$') {
                $status.last_native_phase = [string]$stage.phase
                $status.candidate_sha256 = [string]$stage.candidate_sha256
            }
            else {
                $status.stage_receipt_valid = $false
            }
        }
        catch { $status.stage_receipt_valid = $false }
    }
    $status | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $statusPath -Encoding utf8
    foreach ($path in @($stdoutPath, $stderrPath)) {
        Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    }
}
if ($status.result -ne 'pass') {
    throw ("publisher_native_story_not_passed: {0}" -f $status.failure_code)
}
Write-Host ("Publisher Story Open-SaveAs-Reopen and Reader verification PASS: {0}" -f $status.native_saved_sha256)
