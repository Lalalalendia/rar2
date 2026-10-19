param(
    [Parameter(Mandatory = $true)][string]$TrustedCommitSha,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [switch]$CheckOnly,
    [switch]$AcknowledgeIsolatedLab
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$packetRelative = 'tools/research-runner/experiments/auth-wrap-01.packet.json'
$operationRelative = 'tools/research-runner/operations/auth_wrap_01.ps1'
$packetPath = Join-Path $repo $packetRelative
$operationPath = Join-Path $repo $operationRelative
$paths = @(
    $packetRelative, $operationRelative,
    'tools/research-runner/auth_wrap_direct_1188.ps1',
    'tools/research-runner/auth_wrap_arm_1188.ps1',
    'tools/research-runner/analysis/auth_wrap_diff_1188.py',
    'tools/research-runner/validate_packet.py',
    'tools/research-runner/verify_reset_receipt.py',
    'tools/research-runner/prepare_native_run.ps1',
    'tools/research-runner/finalize_native_run.ps1',
    'tools/validate_auth_wrap_native_receipt.py',
    'tools/windows/pub-runtime/PubRuntime.psm1',
    'packages/research/auth-wrap/v1/native-receipt.schema.json',
    'vendor/producer-a/Cargo.toml',
    'vendor/producer-a/crates/pub-reader/Cargo.toml',
    'vendor/producer-a/crates/pub-reader/src/bin/auth_wrap_oracle_tool.rs'
)
function ExactGit([string[]]$arguments) {
    $result = & git @arguments
    if ($LASTEXITCODE -ne 0) { throw "Git checkout provenance check failed: $($arguments -join ' ')" }
    return ([string]$result).Trim()
}
function RequireProcessFree {
    $found = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
    if ($found.Count -ne 0) { throw 'Publisher is already running; no exclusive native research lease' }
}
function InvokeTool([string]$exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "Native research command failed: $exe ($LASTEXITCODE)" }
}

if ($TrustedCommitSha -cnotmatch '^[0-9a-f]{40}$') { throw 'TrustedCommitSha must be a pinned 40-digit lowercase SHA-1' }
Push-Location $repo
try {
    $actualCommit = ExactGit -arguments @('rev-parse','HEAD')
    if ($actualCommit -cne $TrustedCommitSha) { throw 'Checkout HEAD is not the independently trusted commit' }
    foreach ($path in $paths) {
        $blob = ExactGit -arguments @('rev-parse',"HEAD:$path")
        $disk = ExactGit -arguments @('hash-object','--',$path)
        if ($blob -cne $disk) { throw "Local source differs from trusted Git blob: $path" }
    }
    # Pin all executed first-party scripts and the native schema in addition to
    # the oracle. Ignore old phantom GitHub Actions rows, not local source drift.
    $packet = Get-Content -LiteralPath $packetPath -Raw | ConvertFrom-Json
    if ([string]$packet.id -ne 'AUTH-WRAP-01' -or -not [bool]$packet.reset.required) {
        throw 'AUTH-WRAP packet must require verified native reset before every arm'
    }
    if ([string]$packet.fixture.sha256 -ne '6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf') {
        throw 'Unpinned AUTH-WRAP source fixture'
    }
    InvokeTool 'python' @('tools/research-runner/validate_packet.py','--packet',$packetRelative,
        '--expected-environment','publisher-2019')
    $packetHash = (Get-FileHash -LiteralPath $packetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($CheckOnly) {
        Write-Host "AUTH-WRAP 1188 trusted offline static preflight OK: HEAD=$TrustedCommitSha"
        return
    }
    # This explicit acknowledgement reduces accidental native execution.
    # It is NOT evidence of host exclusivity or Publisher COM PID ownership.
    if (-not $AcknowledgeIsolatedLab) {
        throw 'Native AUTH-WRAP requires explicit -AcknowledgeIsolatedLab; CheckOnly remains non-native'
    }
    if (-not $IsWindows) { throw 'Native AUTH-WRAP may only run on an authorized Windows Publisher2019 lab' }
    if (-not $env:PUB_RESEARCH_RESET_PROVIDER -or
        -not (Test-Path -LiteralPath $env:PUB_RESEARCH_RESET_PROVIDER -PathType Leaf)) {
        throw 'A real configured reset provider is required; no mock or process restart is authority'
    }
    if (-not $env:PUB_RESEARCH_FIXTURE_ROOT) { throw 'Registered fixture root is required' }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust cargo required for source-pinned oracle tool' }
    if (-not (Get-Command pwsh -ErrorAction SilentlyContinue)) { throw 'PowerShell 7 required' }
    # Advisory lease only: independent exclusive Publisher host/PID proof needed.
    $lease = [Threading.Mutex]::new($false, 'Global\Chaptera_AUTH_WRAP_01')
    $leaseHeld = $false
    try {
        $leaseHeld = $lease.WaitOne(0)
        if (-not $leaseHeld) { throw 'AUTH-WRAP exclusive native lease already held' }
        RequireProcessFree
    $root = [IO.Path]::GetFullPath($OutputRoot)
    $repoPrefix = [IO.Path]::GetFullPath($repo).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    if ($root.StartsWith($repoPrefix,[StringComparison]::OrdinalIgnoreCase)) {
        throw 'OutputRoot cannot be inside source checkout'
    }
    if (Test-Path -LiteralPath $root) { throw 'OutputRoot already exists; fresh native output required' }
    New-Item -ItemType Directory -Path $root | Out-Null

    # Build once against the validated exact Git checkout; no downloads or
    # runner registration are performed by this launcher.
    InvokeTool 'cargo' @('build','--offline','--manifest-path','vendor/producer-a/Cargo.toml',
        '-p','pub-reader','--bin','auth_wrap_oracle_tool','--quiet')
    # Cargo honors CARGO_TARGET_DIR and workspace config; never assume the
    # locally built oracle lives in vendor/producer-a/target.
    $metadataJson = & cargo metadata --offline --no-deps --format-version 1 --manifest-path 'vendor/producer-a/Cargo.toml'
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed for source-pinned oracle output' }
    $metadata = $metadataJson | ConvertFrom-Json
    if (-not $metadata.target_directory) { throw 'Cargo target directory unavailable' }
    $oracle = Join-Path ([string]$metadata.target_directory) 'debug/auth_wrap_oracle_tool.exe'
    if (-not (Test-Path -LiteralPath $oracle -PathType Leaf)) { throw 'Built AUTH-WRAP oracle binary missing' }
    $env:PUB_RESEARCH_AUTH_WRAP_ORACLE_EXE = $oracle
    $env:PUB_RESEARCH_PACKET_SHA256 = $packetHash
    $armScript = Join-Path $repo 'tools/research-runner/auth_wrap_arm_1188.ps1'
    $env:PUB_RESEARCH_AUTH_WRAP_CHILD_SCRIPT = $armScript
    $env:PUB_RESEARCH_AUTH_WRAP_CHILD_PACKET = $packetPath
    $env:PUB_RESEARCH_AUTH_WRAP_CHILD_SHA = $TrustedCommitSha
    foreach ($family in @('A','B')) {
        RequireProcessFree
        $armRoot = Join-Path $root "family-$family"
        $private = Join-Path $armRoot 'private'
        New-Item -ItemType Directory -Force -Path $private | Out-Null
        $stdout = Join-Path $private 'bounded-child.stdout.txt'
        $stderr = Join-Path $private 'bounded-child.stderr.txt'
        $env:PUB_RESEARCH_AUTH_WRAP_CHILD_OUTPUT = $armRoot
        $env:PUB_RESEARCH_AUTH_WRAP_CHILD_FAMILY = $family
        $childCommand = @'
$ErrorActionPreference = 'Stop'
& $env:PUB_RESEARCH_AUTH_WRAP_CHILD_SCRIPT -TrustedCommitSha $env:PUB_RESEARCH_AUTH_WRAP_CHILD_SHA -PacketPath $env:PUB_RESEARCH_AUTH_WRAP_CHILD_PACKET -OutputRoot $env:PUB_RESEARCH_AUTH_WRAP_CHILD_OUTPUT -Family $env:PUB_RESEARCH_AUTH_WRAP_CHILD_FAMILY
if (-not $?) { exit 1 }
'@
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
        $child = Start-Process -FilePath (Get-Command pwsh).Source -ArgumentList @(
            '-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',$encoded
        ) -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
        if ($null -eq $child) { throw 'AUTH-WRAP bounded child PID unavailable' }
        try {
            $completed = $child.WaitForExit(600000)
            if (-not $completed) {
                # Only the exact owned child HANDLE may be killed, not MSPUB.
                try { if (-not $child.HasExited) { $child.Kill() } } catch {}
                try { [void]$child.WaitForExit(10000) } catch {}
                'timeout_requires_manual_snapshot_restore' |
                    Set-Content -LiteralPath (Join-Path $private 'native-timeout-quarantine.txt') -Encoding ASCII
                throw 'AUTH-WRAP arm timeout; child terminated only; restore dedicated VM before retry'
            }
            $child.Refresh()
            if (-not $child.HasExited) { throw 'AUTH-WRAP child exit state unavailable' }
            if ([int]$child.ExitCode -ne 0) {
                throw "AUTH-WRAP bounded native arm failed; inspect private logs for family $family"
            }
        } finally {
            $child.Dispose()
        }
        # A remaining Publisher invalidates the lease; never kill unowned PIDs.
        RequireProcessFree
    }
    $receiptA = Join-Path $root 'family-A/analysis/reset-receipt.json'
    $receiptB = Join-Path $root 'family-B/analysis/reset-receipt.json'
    InvokeTool 'python' @('tools/research-runner/verify_reset_receipt.py',
        '--receipt',$receiptA,'--compare-receipt',$receiptB,
        '--expected-baseline',([string]$packet.reset.baseline_id),
        '--expected-snapshot',([string]$packet.reset.snapshot_id),
        '--expected-experiment','AUTH-WRAP-01','--expected-packet-sha256',$packetHash)
    $reset = Get-Content -LiteralPath $receiptA -Raw | ConvertFrom-Json
    $A = Get-Content -LiteralPath (Join-Path $root 'family-A/analysis/auth-wrap-1188.json') -Raw | ConvertFrom-Json
    $B = Get-Content -LiteralPath (Join-Path $root 'family-B/analysis/auth-wrap-1188.json') -Raw | ConvertFrom-Json
    if ($A.family -ne 'A' -or $B.family -ne 'B') { throw 'Mixed family receipts' }
    if ($A.fixture_sha256 -ne $packet.fixture.sha256 -or $B.fixture_sha256 -ne $packet.fixture.sha256) {
        throw 'Different source SHA across native families'
    }
    if ($A.publisher_exe_sha256 -ne $packet.publisher.exe_sha256 -or
        $B.publisher_exe_sha256 -ne $packet.publisher.exe_sha256) { throw 'Publisher executable drift' }
    # Deliberately never claim layout policy/FOPT authority here; this is an
    # execution binding. PUB-T-150 later decides the format law from receipts.
    $native = [ordered]@{
        receipt_version = 'chaptera.auth-wrap-native-receipt.v1'
        task_id = 'PUB-T-150'
        receipt_kind = 'native_observation'
        scope = [ordered]@{
            publisher_family = 'Publisher2019_build12527'
            universal_claim = $false
            closure_candidate = $false
        }
        environment = [ordered]@{
            publisher_version = '16.0'
            publisher_build = 12527
            reset_contract = 'pub-research-reset-receipt.v1'
            reset_provider_receipt_verified = $true
            cold_restore_pair_verified = $true
            environment_fingerprint_sha256 = [string]$reset.environment_fingerprint_sha256
            host_identity_redacted = $true
        }
        fixture = [ordered]@{
            lineage = 'apache_poi_sample_newsletter'
            public_fixture = $true
            sha256 = [string]$packet.fixture.sha256
        }
        family_a = [ordered]@{
            contract = 'change_0x47_only'
            capture = $A.capture
            one_0x47_target_ref_changed = [bool]$A.diff.one_0x47_target_ref_changed
            '0x46_count_kept_consistent' = [bool]$A.diff.'0x46_count_kept_consistent'
            geometry_unchanged_before_open = [bool]$A.diff.geometry_unchanged_before_open
            textwrap_fopt_unchanged_before_open = [bool]$A.diff.textwrap_fopt_unchanged_before_open
            post_save_0x47_outcome = [string]$A.diff.post_save_0x47_outcome
            post_save_fopt_outcome = [string]$A.diff.post_save_fopt_outcome
            layout_outcome = 'not_observed'
        }
        family_b = [ordered]@{
            contract = 'change_textwrap_fopt_only'
            capture = $B.capture
            one_wrap_property_changed_via_com = [bool]$B.one_wrap_property_changed_via_com
            geometry_unchanged_before_save = [bool]$B.diff.geometry_unchanged_before_save
            old_0x47_left_untouched_before_save = [bool]$B.diff.old_0x47_left_untouched_before_save
            post_save_0x47_outcome = [string]$B.diff.post_save_0x47_outcome
            post_save_fopt_outcome = 'not_observed'
            layout_outcome = 'not_observed'
        }
        conclusion = [ordered]@{
            authority_class = 'inconclusive'
            both_conflict_families_executed = $true
            save_reopen_evidence_complete = $true
            needs_additional_native_discriminator = $true
        }
        privacy = [ordered]@{
            pub_bytes_in_receipt = $false
            document_text_in_receipt = $false
            local_path_in_receipt = $false
            host_name_in_receipt = $false
            user_identity_in_receipt = $false
            screenshots_in_receipt = $false
        }
    }
    $analysisRoot = Join-Path $root 'analysis'
    New-Item -ItemType Directory -Force -Path $analysisRoot | Out-Null
    $out = Join-Path $analysisRoot 'auth-wrap-native-receipt.json'
    $native | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $out -Encoding utf8
    InvokeTool 'python' @('tools/validate_auth_wrap_native_receipt.py',$out)
    Write-Host 'Native A/B saved and verified; authority still INCONCLUSIVE pending PUB-T-150 layout discriminator.'
    } finally {
        if ($leaseHeld) { [void]$lease.ReleaseMutex() }
        $lease.Dispose()
    }
} finally {
    Pop-Location
}
