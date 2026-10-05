Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$ExpectedCarltonSha256 = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
$PbFilePublication = 1
$PbTextOrientationHorizontal = 1
$DirectRgbControl = 0x00332211

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
    $Value | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $Path -Encoding UTF8
}

function Get-BytesSha256([byte[]]$Bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace("-", "").ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Get-StringSha256([string]$Value) {
    return Get-BytesSha256 ([Text.Encoding]::UTF8.GetBytes($Value))
}

function Assert-NotElevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Do not run this bundle as Administrator."
    }
}

function Assert-BundleManifest([string]$Root) {
    $manifestPath = Join-Path $Root "bundle-manifest.json"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw "bundle-manifest.json missing" }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([string]$manifest.schema -ne "chaptera.quill-scheme-color-portable.v1") { throw "Unexpected bundle manifest schema" }
    foreach ($item in @($manifest.files)) {
        $candidate = Join-Path $Root ([string]$item.path).Replace("/", "\")
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "Bundle file missing: $($item.path)" }
        if ((Get-Item -LiteralPath $candidate).Length -ne [int64]$item.bytes) { throw "Bundle size mismatch: $($item.path)" }
        if ((Get-Sha256 $candidate) -ne [string]$item.sha256) { throw "Bundle SHA mismatch: $($item.path)" }
    }
    return $manifest
}

function Ensure-PublisherPreflightIdle {
    $running = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
    if ($running.Count -eq 0) { return }

    $app = $null
    try {
        try { $app = [Runtime.InteropServices.Marshal]::GetActiveObject("Publisher.Application") } catch { $app = $null }
        if ($null -eq $app) {
            throw "MSPUB.EXE is running but no safely controllable Publisher COM session was found. Close Publisher manually."
        }
        $count = [int]$app.Documents.Count
        if ($count -gt 0) {
            throw "Publisher is running with $count open document(s). No document was closed automatically."
        }
        Write-Host "Publisher preflight: closing empty existing COM session safely."
        $app.Quit()
    } finally {
        Release-Com $app
    }

    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -eq 0) { return }
    }
    throw "Publisher was asked to quit but MSPUB.EXE remains running. Close it manually; no process was killed."
}

function Assert-NoPublisherProcess {
    for ($i = 0; $i -lt 80; $i++) {
        if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -eq 0) { return }
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
        Start-Sleep -Milliseconds 250
    }
    throw "MSPUB.EXE is still running 20 seconds after a completed experiment stage. This indicates a leaked COM reference or an external Publisher session; no process was killed."
}

function Assert-Publisher2019([string]$Root) {
    Import-Module (Join-Path $Root "tools\windows\pub-runtime\PubRuntime.psm1") -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher COM is unavailable." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) { throw "Publisher Version mismatch." }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) { throw "Publisher Build mismatch." }
    if ($publisher.path.state -ne "value") { throw "Publisher executable path unavailable." }
    $exe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "MSPUB.EXE missing." }
    $fv = [Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion
    if ([string]$fv -ne $ExpectedPublisherFileVersion) { throw "MSPUB.EXE file version mismatch: $fv" }
    $sha = Get-Sha256 $exe
    if ($sha -ne $ExpectedPublisherExeSha256) { throw "MSPUB.EXE SHA mismatch: $sha" }
    return [ordered]@{ version=$ExpectedPublisherVersion; build=$ExpectedPublisherBuild; file_version=$fv; exe_sha256=$sha }
}

function Close-Doc($Doc) {
    if ($null -eq $Doc) { return }
    try { $Doc.Saved = $true } catch {}
    try { $Doc.Close() } catch {}
    Release-Com $Doc
}

function Close-App($App) {
    if ($null -eq $App) { return }
    try { $App.Quit() } catch {}
    Release-Com $App
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}

function Get-ColorValue($Color, [string]$Member) {
    try {
        switch ($Member) {
            "SchemeColor" { return [ordered]@{ state="value"; value=[int]$Color.SchemeColor } }
            "RGB" { return [ordered]@{ state="value"; value=[int64]$Color.RGB } }
            default { throw "Unknown color member $Member" }
        }
    } catch {
        return [ordered]@{ state="error"; message=$_.Exception.Message }
    }
}

function Get-TextRoleSnapshot($Shape, [string]$Phase) {
    $range = $null
    $rows = @()
    try {
        $range = $Shape.TextFrame.TextRange
        for ($role = 1; $role -le 8; $role++) {
            $char = $null; $font = $null; $color = $null
            try {
                $char = $range.Characters($role, 1)
                $font = $char.Font
                $color = $font.Color
                $rows += [ordered]@{
                    position = $role
                    expected_scheme_role = $role
                    scheme_color = Get-ColorValue $color "SchemeColor"
                    rgb = Get-ColorValue $color "RGB"
                }
            } finally {
                Release-Com $color; Release-Com $font; Release-Com $char
            }
        }
        $char = $null; $font = $null; $color = $null
        try {
            $char = $range.Characters(9, 1)
            $font = $char.Font
            $color = $font.Color
            $control = [ordered]@{
                position = 9
                scheme_color = Get-ColorValue $color "SchemeColor"
                rgb = Get-ColorValue $color "RGB"
            }
        } finally {
            Release-Com $color; Release-Com $font; Release-Com $char
        }
        return [ordered]@{ phase=$Phase; roles=$rows; direct_rgb_control=$control }
    } finally {
        Release-Com $range
    }
}

function Get-DocumentSchemeTuple($Doc) {
    $scheme = $null
    $rows = @()
    try {
        $scheme = $Doc.ColorScheme
        for ($role = 1; $role -le 8; $role++) {
            $color = $null
            try {
                $color = $scheme.Colors($role)
                $rows += [int64]$color.RGB
            } finally {
                Release-Com $color
            }
        }
        return $rows
    } finally {
        Release-Com $scheme
    }
}

function Get-SchemeTupleFingerprint([array]$Tuple) {
    $canonical = (@($Tuple | ForEach-Object { [string][int64]$_ }) -join ",")
    return Get-StringSha256 $canonical
}

function Assert-RoleSnapshot([object]$Snapshot) {
    $roles = @($Snapshot.roles)
    if ($roles.Count -ne 8) { throw "Expected eight scheme-bound text ranges." }
    foreach ($row in $roles) {
        if ([string]$row.scheme_color.state -ne "value") { throw "SchemeColor unreadable at role $($row.expected_scheme_role)" }
        if ([int]$row.scheme_color.value -ne [int]$row.expected_scheme_role) {
            throw "SchemeColor role drift at text position $($row.position): got $($row.scheme_color.value)"
        }
        if ([string]$row.rgb.state -ne "value") { throw "RGB unreadable at scheme role $($row.expected_scheme_role)" }
    }
    if ([string]$Snapshot.direct_rgb_control.rgb.state -ne "value") { throw "Direct RGB control unreadable." }
    if ([int64]$Snapshot.direct_rgb_control.rgb.value -ne [int64]$DirectRgbControl) {
        throw "Direct RGB control drift: $($Snapshot.direct_rgb_control.rgb.value)"
    }
}

function New-SyntheticSchemePublication([string]$Path) {
    $app=$null; $doc=$null; $pages=$null; $page=$null; $shapes=$null; $shape=$null; $range=$null
    try {
        $app = New-PubPublisherApplication
        try { $doc = $app.NewDocument() } catch { $doc = $app.Documents.Add() }
        if ($null -eq $doc) { throw "Publisher did not create a new document." }
        $pages = $doc.Pages
        if ([int]$pages.Count -ne 1) { throw "Synthetic seed must have one page." }
        $page = $pages.Item(1)
        $shapes = $page.Shapes
        if ([int]$shapes.Count -ne 0) { throw "Synthetic seed must start with zero shapes." }

        $shape = $shapes.AddTextbox($PbTextOrientationHorizontal, 72, 72, 420, 72)
        $range = $shape.TextFrame.TextRange
        $range.Text = "ABCDEFGHI"
        $range.Font.Name = "Arial"
        $range.Font.Size = 12

        for ($role = 1; $role -le 8; $role++) {
            $char=$null; $font=$null; $color=$null
            try {
                $char = $range.Characters($role,1)
                $font = $char.Font
                $color = $font.Color
                $color.SchemeColor = $role
            } finally {
                Release-Com $color; Release-Com $font; Release-Com $char
            }
        }
        $char=$null; $font=$null; $color=$null
        try {
            $char = $range.Characters(9,1)
            $font = $char.Font
            $color = $font.Color
            $color.RGB = $DirectRgbControl
        } finally {
            Release-Com $color; Release-Com $font; Release-Com $char
        }

        $beforeSave = Get-TextRoleSnapshot $shape "before_save"
        Assert-RoleSnapshot $beforeSave
        $parent = Split-Path -Parent $Path
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
        $doc.SaveAs($Path, $PbFilePublication, $false)
        return [ordered]@{ before_save=$beforeSave }
    } finally {
        Release-Com $range
        Release-Com $shape
        Release-Com $shapes
        Release-Com $page
        Release-Com $pages
        Close-Doc $doc
        Close-App $app
    }
}

function Read-SyntheticSchemePublication([string]$Path, [string]$Phase) {
    $app=$null; $doc=$null; $pages=$null; $page=$null; $shapes=$null; $shape=$null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($Path, $true, $false)
        $pages = $doc.Pages
        if ([int]$pages.Count -ne 1) { throw "Synthetic publication page topology drift after reopen." }
        $page = $pages.Item(1)
        $shapes = $page.Shapes
        if ([int]$shapes.Count -ne 1) { throw "Synthetic publication shape topology drift after reopen." }
        $shape = $shapes.Item(1)
        $snapshot = Get-TextRoleSnapshot $shape $Phase
        Assert-RoleSnapshot $snapshot
        $tuple = Get-DocumentSchemeTuple $doc
        return [ordered]@{ snapshot=$snapshot; scheme_tuple=$tuple; scheme_fingerprint=(Get-SchemeTupleFingerprint $tuple) }
    } finally {
        Release-Com $shape
        Release-Com $shapes
        Release-Com $page
        Release-Com $pages
        Close-Doc $doc
        Close-App $app
    }
}

function Find-DifferentApplicationScheme($App, [array]$CurrentTuple) {
    $schemes=$null
    try {
        $schemes = $App.ColorSchemes
        $count = [int]$schemes.Count
        for ($i = 1; $i -le $count; $i++) {
            $scheme=$null
            try {
                $scheme = $schemes.Item($i)
            $tuple=@()
            for ($role=1; $role -le 8; $role++) {
                $color=$null
                try { $color=$scheme.Colors($role); $tuple += [int64]$color.RGB } finally { Release-Com $color }
            }
            $changed=0
            for ($role=0; $role -lt 8; $role++) {
                if ([int64]$tuple[$role] -ne [int64]$CurrentTuple[$role]) { $changed++ }
            }
            if ($changed -ge 2) {
                return [ordered]@{ index=$i; changed_slots=$changed; tuple=$tuple; name=$(try { [string]$scheme.Name } catch { "" }) }
            }
            } finally {
                Release-Com $scheme
            }
        }
        throw "No installed Publisher ColorScheme differs from current publication in at least two roles."
    } finally {
        Release-Com $schemes
    }
}

function Apply-SchemeSwitch([string]$Path, [array]$CurrentTuple) {
    $app=$null; $doc=$null; $schemes=$null; $scheme=$null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($Path, $false, $false)
        $candidate = Find-DifferentApplicationScheme $app $CurrentTuple
        $schemes = $app.ColorSchemes
        $scheme = $schemes.Item([int]$candidate.index)
        $doc.ColorScheme = $scheme
        $doc.Save()
        return $candidate
    } finally {
        Release-Com $scheme
        Release-Com $schemes
        Close-Doc $doc
        Close-App $app
    }
}

function Read-CarltonScheme([string]$Path) {
    $before = Get-Sha256 $Path
    if ($before -ne $ExpectedCarltonSha256) { throw "Carlton source SHA mismatch: $before" }
    $app=$null; $doc=$null; $scheme=$null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($Path, $true, $false)
        $tuple = Get-DocumentSchemeTuple $doc
        $fingerprint = Get-SchemeTupleFingerprint $tuple
        $schemeName = ""
        try {
            $scheme = $doc.ColorScheme
            $schemeName = [string]$scheme.Name
        } catch {
            $schemeName = ""
        }
        return [ordered]@{
            private_tuple = $tuple
            public_fingerprint = $fingerprint
            scheme_name = $schemeName
        }
    } finally {
        Release-Com $scheme
        Close-Doc $doc
        Close-App $app
        $after = Get-Sha256 $Path
        if ($after -ne $before) { throw "Exact Carlton source changed during read-only probe." }
    }
}

$Root = $PSScriptRoot
$stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
$WorkRoot = Join-Path $Root ("work-" + $stamp)
$ReturnRoot = Join-Path $Root ("return-" + $stamp)
$Analysis = Join-Path $ReturnRoot "analysis"
$Private = Join-Path $ReturnRoot "private"
$Log = Join-Path $ReturnRoot "RUN_NATIVE.log"
New-Item -ItemType Directory -Force -Path $WorkRoot,$ReturnRoot,$Analysis,$Private | Out-Null

$summary = [ordered]@{
    schema = "chaptera.quill-scheme-color-native-run.v1"
    started_at_utc = [DateTime]::UtcNow.ToString("o")
    final_status = "running"
}

try { Start-Transcript -LiteralPath $Log -Force | Out-Null } catch {}

try {
    Assert-NotElevated
    Ensure-PublisherPreflightIdle
    Assert-NoPublisherProcess
    $manifest = Assert-BundleManifest $Root
    $publisher = Assert-Publisher2019 $Root

    $synthetic = Join-Path $Private "scheme-roles-initial.pub"
    $seed = New-SyntheticSchemePublication $synthetic
    Assert-NoPublisherProcess
    $initial = Read-SyntheticSchemePublication $synthetic "fresh_reopen_initial"
    Assert-NoPublisherProcess

    $switched = Join-Path $Private "scheme-roles-switched.pub"
    Copy-Item -LiteralPath $synthetic -Destination $switched -Force
    $switch = Apply-SchemeSwitch $switched $initial.scheme_tuple
    Assert-NoPublisherProcess
    $afterSwitch = Read-SyntheticSchemePublication $switched "fresh_reopen_after_scheme_switch"
    Assert-NoPublisherProcess

    $changedSlots=0
    for($i=0;$i -lt 8;$i++) {
        if([int64]$initial.scheme_tuple[$i] -ne [int64]$afterSwitch.scheme_tuple[$i]) { $changedSlots++ }
    }
    if($changedSlots -lt 2) { throw "Scheme switch did not change at least two effective scheme RGB roles." }

    $carltonPath = Join-Path $Root ("fixtures\" + $ExpectedCarltonSha256 + ".pub")
    $carlton = Read-CarltonScheme $carltonPath
    Assert-NoPublisherProcess

    $probe = Join-Path $Root "runtime\quill-scheme-color-probe.exe"
    if (-not (Test-Path -LiteralPath $probe -PathType Leaf)) {
        throw "quill-scheme-color-probe.exe missing from portable bundle."
    }
    $carrierPath = Join-Path $Private "quill-carrier-map.json"
    & $probe $synthetic $switched $carltonPath $carrierPath
    if ($LASTEXITCODE -ne 0) {
        throw "Quill scheme-color carrier probe failed with exit code $LASTEXITCODE"
    }
    $carrier = Get-Content -LiteralPath $carrierPath -Raw | ConvertFrom-Json
    if ([string]$carrier.schema -ne "chaptera.quill-scheme-color-carrier-map.v1") {
        throw "Unexpected Quill carrier-map schema."
    }
    if (-not [bool]$carrier.scheme_slot_mapping_stable_across_switch) {
        throw "Quill persisted scheme-slot mapping changed across ColorScheme switch."
    }
    $roleMap = @($carrier.role_to_persisted_scheme_slot)
    if ($roleMap.Count -ne 8) {
        throw "Quill carrier map must bind all eight Publisher SchemeColor roles."
    }
    foreach ($row in $roleMap) {
        $role = [int]$row.com_scheme_role
        $slot = [int]$row.persisted_scheme_slot
        if ($role -lt 1 -or $role -gt 8 -or $slot -lt 0 -or $slot -gt 7) {
            throw "Quill carrier map contains out-of-range role/slot."
        }
    }

    $privateReceipt = [ordered]@{
        schema = "chaptera.quill-scheme-color-native.private.v1"
        publisher = $publisher
        synthetic = [ordered]@{
            initial_pub_sha256 = Get-Sha256 $synthetic
            initial = $initial
            switched_pub_sha256 = Get-Sha256 $switched
            selected_application_scheme = $switch
            after_switch = $afterSwitch
            effective_rgb_changed_slot_count = $changedSlots
            direct_rgb_control = [int64]$DirectRgbControl
        }
        carlton = [ordered]@{
            source_sha256 = $ExpectedCarltonSha256
            ordered_scheme_rgb_tuple = $carlton.private_tuple
            ordered_scheme_fingerprint_sha256 = $carlton.public_fingerprint
            scheme_name = $carlton.scheme_name
            source_unchanged = $true
        }
        quill_carrier_map = $carrier
        claims = [ordered]@{
            save_close_fresh_reopen_initial = $true
            save_close_fresh_reopen_switch = $true
            scheme_roles_assumed_to_equal_quill_slots = $false
            direct_rgb_control_present = $true
            carlton_opened_read_only = $true
            carlton_source_mutated = $false
            network_used = $false
        }
    }
    Write-Json $privateReceipt (Join-Path $Private "native-private-receipt.json")

    $publicReceipt = [ordered]@{
        schema = "chaptera.quill-scheme-color-native.v1"
        experiment_id = "QUILL-SCHEME-TEXT-COLOR-AUTH-01"
        publisher = $publisher
        synthetic = [ordered]@{
            role_count = 8
            direct_rgb_control_present = $true
            initial_scheme_fingerprint_sha256 = [string]$initial.scheme_fingerprint
            switched_scheme_fingerprint_sha256 = [string]$afterSwitch.scheme_fingerprint
            effective_rgb_changed_slot_count = $changedSlots
            scheme_roles_stable_after_reopen = $true
            scheme_roles_stable_after_scheme_switch = $true
            generated_initial_pub_sha256 = Get-Sha256 $synthetic
            generated_switched_pub_sha256 = Get-Sha256 $switched
            role_to_persisted_scheme_slot = $roleMap
            scheme_slot_mapping_stable_across_switch = $true
        }
        carlton = [ordered]@{
            source_sha256 = $ExpectedCarltonSha256
            ordered_scheme_fingerprint_sha256 = [string]$carlton.public_fingerprint
            source_unchanged = $true
            persisted_scheme_slot_counts = $carrier.carlton.scheme_slot_counts
            persisted_scheme_source_counts = $carrier.carlton.scheme_source_counts
        }
        boundary = "COM proves native scheme-binding persistence and scheme-switch effective RGB causality. Persisted Quill PL slot mapping is not granted until the returned synthetic PUBs are parsed."
    }
    Write-Json $publicReceipt (Join-Path $Analysis "native-scheme-color-auth.json")
    Write-Json ([ordered]@{
        schema="chaptera.carlton-scheme-fingerprint.v1"
        source_sha256=$ExpectedCarltonSha256
        ordered_scheme_fingerprint_sha256=[string]$carlton.public_fingerprint
        source_unchanged=$true
        raw_rgb_tuple_retained_private=$true
    }) (Join-Path $Analysis "carlton-scheme-fingerprint.json")

    Copy-Item -LiteralPath (Join-Path $Root "bundle-manifest.json") -Destination (Join-Path $ReturnRoot "bundle-manifest.json") -Force
    $summary.publisher = $publisher
    $summary.final_status = "success"
    $summary.generated_initial_pub_sha256 = Get-Sha256 $synthetic
    $summary.generated_switched_pub_sha256 = Get-Sha256 $switched
    $summary.carlton_scheme_fingerprint_sha256 = [string]$carlton.public_fingerprint
    $summary.effective_rgb_changed_slot_count = $changedSlots
    $summary.role_to_persisted_scheme_slot = $roleMap
} catch {
    $summary.final_status = "failed"
    $summary.error = $_.Exception.Message
} finally {
    $summary.finished_at_utc = [DateTime]::UtcNow.ToString("o")
    Write-Json $summary (Join-Path $ReturnRoot "RUN-SUMMARY.json")
    try { Stop-Transcript | Out-Null } catch {}
    $zip = Join-Path $Root ("RETURN-TO-CHAT-" + $stamp + ".zip")
    try { Compress-Archive -Path (Join-Path $ReturnRoot "*") -DestinationPath $zip -CompressionLevel Optimal -Force } catch {}
    Write-Host ""
    Write-Host "FINAL STATUS: $($summary.final_status)"
    Write-Host "RETURN ZIP: $zip"
}

if($summary.final_status -eq "success") { exit 0 } else { exit 1 }
