Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$StackSha = "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade"
$Surface029Sha = "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e"
$PbFilePublication = 1

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Write-Json($Value, [string]$Path) {
    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $Value | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $Path -Encoding UTF8
}

function Assert-NotElevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Do not run this bundle as Administrator. The proven T866 path is the current interactive user session."
    }
}

function Assert-BundleManifest([string]$Root) {
    $manifestPath = Join-Path $Root "bundle-manifest.json"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw "bundle-manifest.json missing" }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([string]$manifest.schema -ne "chaptera.pub2019-portable-p0.v1") { throw "Unexpected bundle manifest schema." }
    if ([string]$manifest.paragraph_probe_crt -ne "static-msvc") { throw "Portable paragraph probe is not declared static-msvc." }
    if ([string]$manifest.paragraph_probe_dependency_audit -ne "no-vcruntime-msvcp-ucrt-imports") { throw "Portable paragraph probe dependency audit is missing or unexpected." }
    foreach ($item in @($manifest.files)) {
        $relative = [string]$item.path
        $candidate = Join-Path $Root ($relative.Replace("/", "\"))
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "Bundle file missing: $relative" }
        $size = (Get-Item -LiteralPath $candidate).Length
        if ([int64]$size -ne [int64]$item.bytes) { throw "Bundle byte-length mismatch: $relative" }
        $actual = Get-Sha256 $candidate
        if ($actual -ne [string]$item.sha256) { throw "Bundle SHA-256 mismatch: $relative" }
    }

    $probeDepsPath = Join-Path $Root "runtime\paragraph-metrics-probe-dependencies.txt"
    if (-not (Test-Path -LiteralPath $probeDepsPath -PathType Leaf)) { throw "Paragraph probe dependency audit missing." }
    $probeDeps = Get-Content -LiteralPath $probeDepsPath -Raw
    foreach ($forbidden in @("VCRUNTIME", "MSVCP", "api-ms-win-crt", "ucrtbase.dll")) {
        if ($probeDeps -match [regex]::Escape($forbidden)) {
            throw "Portable paragraph probe dependency audit contains external CRT component: $forbidden"
        }
    }

    return $manifest
}

function Assert-Publisher2019([string]$Root) {
    $runtime = Join-Path $Root "tools\windows\pub-runtime\PubRuntime.psm1"
    Import-Module $runtime -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher COM is unavailable." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) { throw "Publisher Version mismatch." }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) { throw "Publisher Build mismatch." }
    if ($publisher.path.state -ne "value") { throw "Publisher executable directory is unavailable." }
    $exe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "MSPUB.EXE missing at COM-reported path." }
    $fv = [Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion
    if ([string]$fv -ne $ExpectedPublisherFileVersion) { throw "MSPUB.EXE file version mismatch: $fv" }
    $sha = Get-Sha256 $exe
    if ($sha -ne $ExpectedPublisherExeSha256) { throw "MSPUB.EXE SHA-256 mismatch: $sha" }
    return [ordered]@{ version = $ExpectedPublisherVersion; build = $ExpectedPublisherBuild; file_version = $fv; exe_sha256 = $sha }
}

function Assert-NoPublisherProcess {
    $p = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
    if ($p.Count -gt 0) { throw "MSPUB.EXE is still running after a completed task; refusing to mix Publisher sessions." }
}

function Ensure-PublisherPreflightIdle {
    $p = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
    if ($p.Count -eq 0) { return }

    $app = $null
    try {
        $app = [Runtime.InteropServices.Marshal]::GetActiveObject("Publisher.Application")
    } catch {
        $app = $null
    }

    if ($null -eq $app) {
        $details = @($p | ForEach-Object {
            [ordered]@{
                id = [int]$_.Id
                main_window = ([int64]$_.MainWindowHandle -ne 0)
                started = $(try { $_.StartTime.ToUniversalTime().ToString("o") } catch { "unknown" })
            }
        })
        $summary = ($details | ConvertTo-Json -Compress -Depth 4)
        throw "MSPUB.EXE is already running, but no safely controllable active Publisher COM session was found. No process was killed. Close Publisher/its background MSPUB process manually and rerun. Processes: $summary"
    }

    try {
        $documentCount = [int]$app.Documents.Count
        if ($documentCount -gt 0) {
            throw "Publisher is already running with $documentCount open document(s). No document was closed automatically. Save/close them and rerun."
        }

        Write-Host "Publisher preflight: empty existing session detected; closing it safely before the authority run."
        $app.Quit()
    } finally {
        Release-Com $app
    }

    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -eq 0) { return }
    }

    throw "Publisher preflight asked an empty COM session to quit, but MSPUB.EXE is still running. No process was killed; close it manually and rerun."
}

function New-NativeBlank {
    param([string]$Root, [string]$Path)
    Import-Module (Join-Path $Root "tools\windows\pub-runtime\PubRuntime.psm1") -Force
    $app = $null; $doc = $null
    try {
        $app = New-PubPublisherApplication
        try { $doc = $app.NewDocument() } catch { $doc = $app.Documents.Add() }
        if ($null -eq $doc) { throw "Publisher did not create a new document." }
        if ([int]$doc.Pages.Count -ne 1) { throw "Native blank must start with exactly one page." }
        if ([int]$doc.Pages.Item(1).Shapes.Count -ne 0) { throw "Native blank must start with zero shapes." }
        $parent = Split-Path -Parent $Path
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
        $doc.SaveAs($Path, $PbFilePublication, $false)
    } finally {
        if ($null -ne $doc) { try { $doc.Saved = $true } catch {}; try { $doc.Close() } catch {}; Release-Com $doc }
        if ($null -ne $app) { try { Close-PubPublisherApplication $app } catch {} }
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Native blank SaveAs did not create a PUB." }
    return [ordered]@{ sha256 = Get-Sha256 $Path; bytes = (Get-Item -LiteralPath $Path).Length; provenance = "publisher-new-document-portable-seed" }
}

function Invoke-PowerShellStep {
    param([string]$Label, [string]$Script, [string[]]$Arguments, [string]$ConsoleLog)
    Write-Host ""
    Write-Host ("=" * 72)
    Write-Host $Label
    Write-Host ("=" * 72)
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $Script @Arguments *>&1 | Tee-Object -FilePath $ConsoleLog -Append
    if ($LASTEXITCODE -ne 0) { throw "$Label failed with exit code $LASTEXITCODE" }
}

function Copy-SafeTree {
    param([string]$SourceRoot, [string]$DestRoot)
    New-Item -ItemType Directory -Force -Path $DestRoot | Out-Null
    foreach ($name in @("analysis","logs")) {
        $src = Join-Path $SourceRoot $name
        if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination $DestRoot -Recurse -Force }
    }
    foreach ($name in @("environment.json","evidence-manifest.json")) {
        $src = Join-Path $SourceRoot $name
        if (Test-Path -LiteralPath $src -PathType Leaf) { Copy-Item -LiteralPath $src -Destination (Join-Path $DestRoot $name) -Force }
    }
}

$Root = $PSScriptRoot
$stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
$WorkRoot = Join-Path $Root ("work-" + $stamp)
$ReturnRoot = Join-Path $Root ("return-" + $stamp)
$TopLog = Join-Path $ReturnRoot "RUN_NATIVE.log"
New-Item -ItemType Directory -Force -Path $WorkRoot,$ReturnRoot | Out-Null

$summary = [ordered]@{
    schema = "chaptera.pub2019-portable-p0-run.v1"
    started_at_utc = [DateTime]::UtcNow.ToString("o")
    bundle = $null
    publisher = $null
    tasks = @()
    final_status = "running"
}

try { Start-Transcript -LiteralPath $TopLog -Force | Out-Null } catch {}

try {
    Assert-NotElevated
    Ensure-PublisherPreflightIdle
    Assert-NoPublisherProcess
    $manifest = Assert-BundleManifest $Root
    $summary.bundle = [ordered]@{ source_main_sha = [string]$manifest.source_main_sha; corpus_artifact_id = [int64]$manifest.corpus_artifact_id }
    $summary.publisher = Assert-Publisher2019 $Root

    $stackFixture = Join-Path $Root ("fixtures\" + $StackSha + ".pub")
    $surfaceFixture = Join-Path $Root ("fixtures\" + $Surface029Sha + ".pub")
    if ((Get-Sha256 $stackFixture) -ne $StackSha) { throw "033 fixture SHA mismatch." }
    if ((Get-Sha256 $surfaceFixture) -ne $Surface029Sha) { throw "029 fixture SHA mismatch." }

    # Embedded Python is carried as a pinned ZIP and expanded only into this run work directory.
    $pythonZip = Join-Path $Root "runtime\python-3.13.16-embed-amd64.zip"
    $pythonRoot = Join-Path $WorkRoot "python"
    Expand-Archive -LiteralPath $pythonZip -DestinationPath $pythonRoot -Force
    $python = Join-Path $pythonRoot "python.exe"
    if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { throw "Embedded python.exe missing after extraction." }

    # 033
    $stackOut = Join-Path $WorkRoot "033-stack"
    $stackConsole = Join-Path $ReturnRoot "033-stack-console.txt"
    try {
        Invoke-PowerShellStep "033 MASTER/PAGE STACK" (Join-Path $Root "tools\research-runner\operations\master_projection_stack_auth_01.ps1") @("-InputPath",$stackFixture,"-OutputRoot",$stackOut) $stackConsole
        $receipt = Get-Content -LiteralPath (Join-Path $stackOut "analysis\master-projection-stack-auth-01.json") -Raw | ConvertFrom-Json
        if ([string]$receipt.schema -ne "chaptera.master-projection-stack-auth.v1") { throw "Unexpected stack receipt schema: $($receipt.schema)" }
        if ([string]$receipt.experiment_id -ne "MASTER-PROJECTION-STACK-AUTH-01") { throw "Unexpected stack experiment identity: $($receipt.experiment_id)" }
        $stackVerdict = [string]$receipt.verdict
        if ($stackVerdict -notin @("save_as_picture_master_visibility_not_proven","page_local_above_master","master_above_page_local","ambiguous_or_creation_order_sensitive")) { throw "Unexpected stack oracle verdict: $stackVerdict" }
        $stackArms = @($receipt.arms)
        if ($stackArms.Count -ne 2) { throw "Stack oracle must return exactly two creation-order arms." }
        $orders = @($stackArms | ForEach-Object { [string]$_.creation_order } | Sort-Object -Unique)
        if ($orders.Count -ne 2 -or $orders -notcontains "master_first" -or $orders -notcontains "page_first") { throw "Stack oracle creation-order arms are incomplete or duplicated." }
        $winners = @($stackArms | ForEach-Object { [string]$_.overlap.winner })
        foreach ($winner in $winners) { if ($winner -notin @("master","page_local","ambiguous")) { throw "Unexpected stack arm winner: $winner" } }
        $uniqueWinners = @($winners | Sort-Object -Unique)
        $masterVisible = [bool]$receipt.master_visibility_control.master_visible
        switch ($stackVerdict) {
            "save_as_picture_master_visibility_not_proven" { if ($masterVisible) { throw "Visibility-not-proven verdict contradicts positive master visibility control." } }
            "page_local_above_master" { if (-not $masterVisible -or $uniqueWinners.Count -ne 1 -or $uniqueWinners[0] -ne "page_local") { throw "page_local_above_master verdict contradicts arm winners or visibility control." } }
            "master_above_page_local" { if (-not $masterVisible -or $uniqueWinners.Count -ne 1 -or $uniqueWinners[0] -ne "master") { throw "master_above_page_local verdict contradicts arm winners or visibility control." } }
            "ambiguous_or_creation_order_sensitive" { if (-not $masterVisible) { throw "Ambiguous/order-sensitive verdict requires positive master visibility." }; if ($uniqueWinners.Count -eq 1 -and $uniqueWinners[0] -in @("master","page_local")) { throw "Ambiguous/order-sensitive verdict contradicts stable two-arm winner." } }
        }
        if ([string]$receipt.source.sha256 -ne $StackSha -or -not [bool]$receipt.source.unchanged_after_experiment) { throw "Stack receipt source identity/immutability mismatch." }
        if ([bool]$receipt.claims.source_original_mutated -or [bool]$receipt.claims.generated_pub_uploaded -or [bool]$receipt.claims.generated_page_picture_uploaded) { throw "Stack receipt violates source/private-artifact boundary." }
        if (-not [bool]$receipt.claims.save_close_fresh_reopen_per_arm -or -not [bool]$receipt.claims.two_page_master_excluded) { throw "Stack receipt is missing bounded experiment fences." }
        $summary.tasks += [ordered]@{ id="033-stack"; status="success"; verdict=$stackVerdict }
        Copy-SafeTree $stackOut (Join-Path $ReturnRoot "033-stack")
    } catch {
        $summary.tasks += [ordered]@{ id="033-stack"; status="failed"; error=$_.Exception.Message }
    }
    Assert-NoPublisherProcess

    # 029
    $surfaceOut = Join-Path $WorkRoot "029-surface"
    $surfaceConsole = Join-Path $ReturnRoot "029-surface-console.txt"
    try {
        Invoke-PowerShellStep "029 NATIVE PAGE/SPREAD" (Join-Path $Root "tools\research-runner\operations\mature_029_native_page_spread_oracle_01.ps1") @("-InputPath",$surfaceFixture,"-OutputRoot",$surfaceOut) $surfaceConsole
        $native029 = Join-Path $surfaceOut "analysis\mature-029-native-page-spread-oracle-01.json"
        $class029 = Join-Path $surfaceOut "analysis\mature-029-reference-surface-classification.json"
        & $python (Join-Path $Root "tools\classify_mature_029_native_page_spread_v1.py") $native029 --out $class029
        if ($LASTEXITCODE -ne 0) { throw "029 classifier failed with exit code $LASTEXITCODE" }
        $nativeReceipt029 = Get-Content -LiteralPath $native029 -Raw | ConvertFrom-Json
        if (-not [bool]$nativeReceipt029.source.unchanged_after_probe) { throw "029 oracle did not prove source immutability." }
        if ($nativeReceipt029.claims.document_mutation_invoked -or $nativeReceipt029.claims.save_invoked -or $nativeReceipt029.claims.print_invoked -or $nativeReceipt029.claims.export_invoked -or $nativeReceipt029.claims.macro_execution_invoked) { throw "029 native receipt violates the read-only contract." }
        $classified = Get-Content -LiteralPath $class029 -Raw | ConvertFrom-Json
        if ([string]$classified.source_sha256 -ne $Surface029Sha) { throw "029 classification source identity mismatch." }
        $surfaceStage = [string]$classified.reference_surface_stage
        if ($surfaceStage -notin @("unknown","production_sheet")) { throw "Unexpected 029 reference surface stage: $surfaceStage" }
        if (-not [bool]$classified.claims.classification_uses_native_publisher_state -or [bool]$classified.claims.raster_similarity_used -or [bool]$classified.claims.pdf_page_count_used_as_pub_semantics -or -not [bool]$classified.claims.unknown_is_fail_closed) { throw "029 classifier authority contract mismatch." }
        if ($surfaceStage -eq "production_sheet") { if (-not [bool]$classified.booklet_intent.confirmed) { throw "production_sheet classification lacks native booklet intent." }; if ([int]$classified.logical_page_count -ne 4 -or [int]$classified.reference_page_count -ne 2) { throw "production_sheet classification has unexpected logical/reference page cardinality." } }
        $summary.tasks += [ordered]@{ id="029-surface"; status="success"; reference_surface_stage=$surfaceStage }
        Copy-SafeTree $surfaceOut (Join-Path $ReturnRoot "029-surface")
    } catch {
        $summary.tasks += [ordered]@{ id="029-surface"; status="failed"; error=$_.Exception.Message }
    }
    Assert-NoPublisherProcess

    # Paragraph metrics portable variant: Publisher itself creates the zero-shape blank.
    $paragraphOut = Join-Path $WorkRoot "paragraph-metrics"
    $paragraphConsole = Join-Path $ReturnRoot "paragraph-metrics-console.txt"
    try {
        New-Item -ItemType Directory -Force -Path $paragraphOut | Out-Null
        $blank = Join-Path $WorkRoot "paragraph-native-blank.pub"
        $blankIdentity = New-NativeBlank -Root $Root -Path $blank
        $env:PUB_RESEARCH_FIXTURE = $blank
        $packet = Join-Path $Root "tools\research-runner\experiments\paragraph-metrics-auth-01.packet.json"
        Invoke-PowerShellStep "PARAGRAPH METRICS NATIVE MATRIX" (Join-Path $Root "tools\research-runner\operations\paragraph_metrics_auth_01.ps1") @("-PacketPath",$packet,"-OutputRoot",$paragraphOut) $paragraphConsole

        $envDoc = [ordered]@{
            schema = "chaptera.pub2019-portable-paragraph-environment.v1"
            experiment_id = "PARAGRAPH-METRICS-AUTH-01"
            publisher = $summary.publisher
            fixture = $blankIdentity
            authority_note = "Portable variant uses Publisher.NewDocument/Documents.Add native blank rather than historical 5bf605... profile-instantiated blank. Common-seed 11-arm causal matrix remains exact after seed creation."
        }
        Write-Json $envDoc (Join-Path $paragraphOut "environment.json")

        & $python (Join-Path $Root "tools\research-runner\analysis\paragraph_metrics_auth_01_blast_radius.py") --output-root $paragraphOut
        if ($LASTEXITCODE -ne 0) { throw "paragraph blast-radius analysis failed with exit code $LASTEXITCODE" }
        $probe = Join-Path $Root "runtime\paragraph-metrics-probe.exe"
        & $python (Join-Path $Root "tools\research-runner\analysis\paragraph_metrics_auth_01_structural.py") --output-root $paragraphOut --snapshot-tool $probe
        if ($LASTEXITCODE -ne 0) { throw "paragraph structural analysis failed with exit code $LASTEXITCODE" }
        Invoke-PowerShellStep "PARAGRAPH EVIDENCE FINALIZE" (Join-Path $Root "tools\research-runner\finalize_native_run.ps1") @("-PacketPath",$packet,"-OutputRoot",$paragraphOut) $paragraphConsole

        $nativePath = Join-Path $paragraphOut "analysis\paragraph-metrics-auth-01.json"
        $blastPath = Join-Path $paragraphOut "analysis\paragraph-metrics-auth-01-blast-radius.json"
        $structuralPath = Join-Path $paragraphOut "analysis\paragraph-metrics-auth-01-structural.json"
        $manifestPath = Join-Path $paragraphOut "evidence-manifest.json"
        foreach ($required in @($nativePath,$blastPath,$structuralPath,$manifestPath,(Join-Path $paragraphOut "environment.json"),(Join-Path $paragraphOut "logs\paragraph-metrics-auth-01.txt"))) { if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Required paragraph evidence missing: $required" } }
        $nativeParagraph = Get-Content -LiteralPath $nativePath -Raw | ConvertFrom-Json
        $blast = Get-Content -LiteralPath $blastPath -Raw | ConvertFrom-Json
        $structural = Get-Content -LiteralPath $structuralPath -Raw | ConvertFrom-Json
        if ([string]$nativeParagraph.verdict -ne "native-semantic-arms-captured-with-common-seed") { throw "Unexpected paragraph native verdict: $($nativeParagraph.verdict)" }
        if ($null -eq $nativeParagraph.seed -or [string]::IsNullOrWhiteSpace([string]$nativeParagraph.seed.sha256)) { throw "Paragraph native receipt did not record common seed SHA." }
        if (@($nativeParagraph.arms).Count -ne 11) { throw "Expected 11 paragraph native arms." }
        if (@($blast.arms).Count -ne 10) { throw "Expected 10 paragraph mutation blast-radius arms." }
        foreach ($name in @("common_seed_used","matched_noop_control_used","paragraph_before_mutation_identical_across_arms","frame_before_mutation_identical_across_arms","fresh_reopen_text_length_invariant_across_arms")) { if (-not [bool]$blast.causal_baseline.$name) { throw "Paragraph causal baseline invariant failed: $name" } }
        if ([string]$blast.remaining_structural_gap.raw_fdpp_property_decode -ne "required" -or [string]$blast.remaining_structural_gap.quill_text_byte_invariance -ne "required") { throw "Unexpected paragraph pre-structural authority boundary." }
        foreach ($name in @("complete_eleven_arm_matrix","exact_artifact_identity_join","common_pre_mutation_snapshots","quill_text_byte_invariance_all_arms","confirmed_story_partition_invariant","raw_fdpp_framing_known")) { if (-not [bool]$structural.invariants.$name) { throw "Paragraph structural invariant failed: $name" } }
        if ([bool]$structural.invariants.paragraph_metric_semantics_granted) { throw "Portable structural receipt must not grant paragraph metric semantics." }
        if ([string]$structural.remaining_authority.native_rule_to_persisted_carrier_law -ne "requires_semantic_review_of_raw_matrix") { throw "Unexpected paragraph structural carrier-law status." }
        if ([string]$structural.remaining_authority.effective_line_origins_and_heights -ne "not_proven_by_frame_bounds_or_raw_values") { throw "Unexpected paragraph effective-line authority status." }
        $manifestReceipt = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        $manifestPaths = @($manifestReceipt.files | ForEach-Object { [string]$_.path })
        foreach ($requiredPath in @("analysis/paragraph-metrics-auth-01.json","analysis/paragraph-metrics-auth-01-blast-radius.json","analysis/paragraph-metrics-auth-01-structural.json","environment.json","logs/paragraph-metrics-auth-01.txt")) { if ($manifestPaths -notcontains $requiredPath) { throw "Paragraph evidence manifest does not bind required file: $requiredPath" } }
        if ((Get-Sha256 $blank) -ne [string]$blankIdentity.sha256) { throw "Portable paragraph seed changed during the run." }
        $summary.tasks += [ordered]@{ id="paragraph-metrics"; status="success"; text_invariance=[bool]$structural.invariants.quill_text_byte_invariance_all_arms; semantics_granted=[bool]$structural.invariants.paragraph_metric_semantics_granted; seed_provenance=[string]$blankIdentity.provenance }
        Copy-SafeTree $paragraphOut (Join-Path $ReturnRoot "paragraph-metrics")
    } catch {
        $summary.tasks += [ordered]@{ id="paragraph-metrics"; status="failed"; error=$_.Exception.Message }
    }

    $ok = @($summary.tasks | Where-Object { $_.status -eq "success" }).Count
    $summary.final_status = if ($ok -eq 3) { "all_three_success" } elseif ($ok -gt 0) { "partial_success" } else { "failed" }
} catch {
    $summary.final_status = "bootstrap_failed"
    $summary.bootstrap_error = $_.Exception.Message
} finally {
    $summary.finished_at_utc = [DateTime]::UtcNow.ToString("o")
    Write-Json $summary (Join-Path $ReturnRoot "RUN-SUMMARY.json")
    Copy-Item -LiteralPath (Join-Path $Root "bundle-manifest.json") -Destination (Join-Path $ReturnRoot "bundle-manifest.json") -Force
    try { Stop-Transcript | Out-Null } catch {}
    $returnZip = Join-Path $Root ("RETURN-TO-CHAT-" + $stamp + ".zip")
    try { Compress-Archive -Path (Join-Path $ReturnRoot "*") -DestinationPath $returnZip -CompressionLevel Optimal -Force } catch {}
    Write-Host ""
    Write-Host "FINAL STATUS: $($summary.final_status)"
    Write-Host "RETURN ZIP: $returnZip"
}

if ($summary.final_status -eq "all_three_success") { exit 0 } else { exit 1 }
