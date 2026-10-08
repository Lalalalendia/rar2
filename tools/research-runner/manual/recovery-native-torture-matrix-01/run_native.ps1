param(
    [Parameter(Mandatory=$true)]
    [string]$InputRoot,

    [string]$ManifestPath = (Join-Path $PSScriptRoot "manifest.json"),

    [string]$OutputRoot = (Join-Path $PSScriptRoot "out")
)

$ErrorActionPreference = "Stop"

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Find-ExactWitness([string]$Root, [string]$ExpectedSha, [string]$Label) {
    $direct = Join-Path $Root $Label
    if (Test-Path -LiteralPath $direct) {
        if ((Get-Sha256 $direct) -eq $ExpectedSha) { return $direct }
    }

    Get-ChildItem -LiteralPath $Root -File -Filter *.pub -Recurse | ForEach-Object {
        if ((Get-Sha256 $_.FullName) -eq $ExpectedSha) { return $_.FullName }
    }

    return $null
}

function Classify-PublisherError([System.Management.Automation.ErrorRecord]$ErrorRecord) {
    $message = [string]$ErrorRecord.Exception.Message
    $lower = $message.ToLowerInvariant()

    if ($lower -match "serious error|serious problem|protected") {
        return "refused_serious_file_error"
    }
    if ($lower -match "version|created by a newer|cannot be opened by this version|unsupported") {
        return "refused_unsupported_version"
    }
    return "refused_other"
}

$manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

$summary = [ordered]@{
    schema = "chaptera.recovery-native-torture-run.v1"
    experiment_id = $manifest.experiment_id
    publisher_pin = $manifest.publisher_pin
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    rows = @()
}

foreach ($w in $manifest.witnesses) {
    $expectedSha = [string]$w.sha256
    $sourcePath = Find-ExactWitness -Root $InputRoot -ExpectedSha $expectedSha -Label ([string]$w.source_label)

    $row = [ordered]@{
        schema = "chaptera.recovery-native-pair.v1"
        experiment_id = $manifest.experiment_id
        phase = [string]$w.phase
        source_sha256 = $expectedSha
        source_label = [string]$w.source_label
        damage_class = [string]$w.damage_class
        publisher = [ordered]@{
            application_version = $null
            build = $null
            open_mode = "read_only"
            add_to_recent_files = $false
            outcome = $null
            error_class = $null
            error_hresult = $null
            error_message_digest = $null
            page_count = $null
            shape_count = $null
            story_count = $null
        }
        chaptera = [ordered]@{
            canonical_outcome = [string]$w.chaptera_canonical_outcome
            authority = [string]$w.authority
        }
        source_modified = $null
        pre_sha256 = $null
        post_sha256 = $null
        competitive_class = "inconclusive"
    }

    if (-not $sourcePath) {
        $row.publisher.outcome = "environment_failure"
        $row.publisher.error_class = "exact_sha_not_found"
        $summary.rows += [pscustomobject]$row
        continue
    }

    $row.pre_sha256 = Get-Sha256 $sourcePath
    if ($row.pre_sha256 -ne $expectedSha) {
        $row.publisher.outcome = "environment_failure"
        $row.publisher.error_class = "pre_sha_mismatch"
        $summary.rows += [pscustomobject]$row
        continue
    }

    $app = $null
    $doc = $null
    try {
        $app = New-Object -ComObject Publisher.Application
        $row.publisher.application_version = [string]$app.Version
        try { $row.publisher.build = [int]$app.Build } catch { $row.publisher.build = $null }

        if ($manifest.publisher_pin.version -and $row.publisher.application_version -ne [string]$manifest.publisher_pin.version) {
            throw "Publisher version mismatch: expected $($manifest.publisher_pin.version), got $($row.publisher.application_version)"
        }
        if ($manifest.publisher_pin.build -and $row.publisher.build -and $row.publisher.build -ne [int]$manifest.publisher_pin.build) {
            throw "Publisher build mismatch: expected $($manifest.publisher_pin.build), got $($row.publisher.build)"
        }

        try {
            # Publisher.Application.Open(FileName, ReadOnly, AddToRecentFiles, SaveChanges)
            $doc = $app.Open($sourcePath, $true, $false, $false)
            $row.publisher.outcome = "opened"

            try { $row.publisher.page_count = [int]$doc.Pages.Count } catch {}
            try {
                $shapeCount = 0
                foreach ($page in $doc.Pages) { $shapeCount += [int]$page.Shapes.Count }
                $row.publisher.shape_count = $shapeCount
            } catch {}
            try { $row.publisher.story_count = [int]$doc.Stories.Count } catch {}
        }
        catch {
            $row.publisher.outcome = Classify-PublisherError $_
            $row.publisher.error_class = $row.publisher.outcome
            try { $row.publisher.error_hresult = ("0x{0:X8}" -f ($_.Exception.HResult -band 0xffffffff)) } catch {}
            $msgBytes = [Text.Encoding]::UTF8.GetBytes([string]$_.Exception.Message)
            $sha = [Security.Cryptography.SHA256]::Create()
            try {
                $row.publisher.error_message_digest = "sha256:" + ([BitConverter]::ToString($sha.ComputeHash($msgBytes)).Replace("-", "").ToLowerInvariant())
            } finally {
                $sha.Dispose()
            }
        }
    }
    catch {
        $row.publisher.outcome = "environment_failure"
        $row.publisher.error_class = "publisher_environment_or_pin_failure"
        try { $row.publisher.error_hresult = ("0x{0:X8}" -f ($_.Exception.HResult -band 0xffffffff)) } catch {}
    }
    finally {
        if ($doc -ne $null) {
            try { $doc.Close() } catch {}
            try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($doc) } catch {}
        }
        if ($app -ne $null) {
            try { $app.Quit() } catch {}
            try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($app) } catch {}
        }
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
    }

    $row.post_sha256 = Get-Sha256 $sourcePath
    $row.source_modified = ($row.pre_sha256 -ne $row.post_sha256)

    if ($row.source_modified) {
        $row.competitive_class = "inconclusive"
    }
    elseif ($row.publisher.outcome -eq "opened") {
        $row.competitive_class = "B"
    }
    elseif ($row.publisher.outcome -like "refused_*" -and $row.chaptera.canonical_outcome -eq "salvage_open") {
        $row.competitive_class = "A"
    }

    $summary.rows += [pscustomobject]$row
}

$summaryPath = Join-Path $OutputRoot "recovery-native-torture-summary.json"
$summary | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $summaryPath -Encoding UTF8

$phase1a = @($summary.rows | Where-Object { $_.phase -eq "1A" })
$phase1aPath = Join-Path $OutputRoot "phase1a-summary.json"
([ordered]@{
    schema = "chaptera.recovery-native-torture-phase1a.v1"
    experiment_id = $manifest.experiment_id
    rows = $phase1a
}) | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $phase1aPath -Encoding UTF8

Write-Host "Wrote:"
Write-Host "  $summaryPath"
Write-Host "  $phase1aPath"
