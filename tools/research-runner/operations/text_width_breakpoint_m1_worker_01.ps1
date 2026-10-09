param(
    [Parameter(Mandatory=$true)][ValidateSet("seed","arm")][string]$Mode,
    [Parameter(Mandatory=$true)][string]$SourcePath,
    [Parameter(Mandatory=$true)][string]$OutputRoot,
    [ValidateSet("none","control-before","narrow","wide","control-after")][string]$ArmId = "none",
    [double]$WidthPt = 160,
    [string]$ExpectedSourceSha = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$ExpectedText = "The quick brown fox crosses narrow publisher text frames while the same physical font and paragraph policy determine exactly where each visible line ends."
$FontName = "Arial"
$FontSize = 12.0
$InitialWidth = 160.0
$Height = 330.0
$PrivateDir = Join-Path $OutputRoot "private/text-width-m1"
$AnalysisDir = Join-Path $OutputRoot "analysis"
$SeedPath = Join-Path $PrivateDir "seed.pub"
$SeedMetaPath = Join-Path $AnalysisDir "text-width-m1-seed.json"
New-Item -ItemType Directory -Force -Path $PrivateDir,$AnalysisDir | Out-Null

$Stage = "preflight"
$Status = "invalid"
$stageName = if ($Mode -eq "seed") { "seed" } else { $ArmId }
$StagePath = Join-Path $AnalysisDir ("text-width-m1-" + $stageName + "-stage.json")

function File-Sha([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Text-Sha([string]$Text) {
    $bytes = [Text.Encoding]::Unicode.GetBytes($Text)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-","").ToLowerInvariant()
    } finally { $sha.Dispose() }
}
function Release-Com($Object) {
    if ($null -ne $Object -and [Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object) } catch {}
    }
}
function Close-Document($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Saved = $true } catch {}
    try { $Document.Close() } catch {}
    Release-Com $Document
}
function Write-Stage([string]$State, [string]$Phase, [string]$HresultHex = "") {
    # Persist nested Snapshot subphases through the outer catch handler.
    if ($State -eq "running") { $script:Stage = $Phase }
    Write-PubJson -Path $StagePath -Value ([ordered]@{
        schema = "chaptera.text-width-m1-stage.v1"
        experiment_id = "TEXT-WIDTH-BREAKPOINT-M1-01"
        case = $stageName
        state = $State
        phase = $Phase
        hresult_hex = $HresultHex
    })
}
function Get-ShapeByIdentity($Document, $Meta) {
    $page = $Document.Pages.Item(1)
    if ([int64]$page.PageID -ne [int64]$Meta.page_id) {
        throw "m1_page_identity_drift"
    }
    $shape = $page.Shapes.Item([int]$Meta.shape_index)
    if ([int64]$shape.ID -ne [int64]$Meta.shape_id) {
        throw "m1_shape_identity_drift"
    }
    return $shape
}
function Snapshot($Shape, [string]$PhaseTag = "") {
    $frame = $null
    $range = $null
    $font = $null
    $lines = @()
    try {
        if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_frame") }
        $frame = $Shape.TextFrame
        if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_range") }
        $range = $frame.TextRange
        if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_font") }
        $font = $range.Font
        if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_lines_count") }
        $count = [int]$range.LinesCount
        if ($count -lt 2 -or $count -gt 32) {
            throw "m1_unexpected_line_count"
        }
        for ($index=1; $index -le $count; $index++) {
            $line = $null
            try {
                if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_line_" + $index) }
                $line = $range.Lines($index,1)
                $lines += [ordered]@{
                    index = $index
                    start = [int]$line.Start
                    end = [int]$line.End
                    bound_left_pt = [double]$line.BoundLeft
                    bound_top_pt = [double]$line.BoundTop
                    bound_width_pt = [double]$line.BoundWidth
                    bound_height_pt = [double]$line.BoundHeight
                }
            } finally { Release-Com $line }
        }
        if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_text") }
        $text = [string]$range.Text
        if ($PhaseTag) { Write-Stage "running" ($PhaseTag + "_properties") }
        return [ordered]@{
            width_pt = [double]$Shape.Width
            height_pt = [double]$Shape.Height
            font_name = [string]$font.Name
            font_size_pt = [double]$font.Size
            auto_fit_mode = [int]$frame.AutoFitText
            margin_left_pt = [double]$frame.MarginLeft
            margin_right_pt = [double]$frame.MarginRight
            margin_top_pt = [double]$frame.MarginTop
            margin_bottom_pt = [double]$frame.MarginBottom
            text_utf16_units = [int]$text.Length
            text_utf16le_sha256 = Text-Sha $text
            line_count = $count
            lines = $lines
        }
    } finally {
        Release-Com $font
        Release-Com $range
        Release-Com $frame
    }
}
function Assert-FixedSource($Snapshot) {
    if ([string]$Snapshot.font_name -ne $FontName -or
        [math]::Abs([double]$Snapshot.font_size_pt - $FontSize) -gt 0.001 -or
        [int]$Snapshot.auto_fit_mode -ne 0 -or
        [int]$Snapshot.text_utf16_units -lt $ExpectedText.Length -or
        [int]$Snapshot.text_utf16_units -gt ($ExpectedText.Length + 2)) {
        throw "m1_font_or_text_or_autofit_drift"
    }
}

try {
    Write-Stage "running" $Stage
    $sourceSha = File-Sha $SourcePath
    if ($ExpectedSourceSha -and $sourceSha -ne $ExpectedSourceSha) {
        throw "m1_source_fingerprint_mismatch"
    }

    if ($Mode -eq "seed") {
        if ($ArmId -ne "none") { throw "m1_seed_arm_id_invalid" }
        Copy-Item -LiteralPath $SourcePath -Destination $SeedPath -Force
        $app = $null; $doc = $null; $shape = $null; $frame = $null; $range = $null
        try {
            $Stage = "seed_application_create"
            Write-Stage "running" $Stage
            $app = New-PubPublisherApplication
            $Stage = "seed_document_open"
            Write-Stage "running" $Stage
            $doc = $app.Open($SeedPath,$false,$false)
            $Stage = "seed_page_lookup"
            Write-Stage "running" $Stage
            $page = $doc.Pages.Item(1)
            $Stage = "seed_textbox_add"
            Write-Stage "running" $Stage
            $shape = $page.Shapes.AddTextbox(1,72,72,$InitialWidth,$Height)
            $Stage = "seed_frame_policy"
            Write-Stage "running" $Stage
            $frame = $shape.TextFrame
            $frame.AutoFitText = 0
            $Stage = "seed_text_write"
            Write-Stage "running" $Stage
            $range = $frame.TextRange
            $range.Text = $ExpectedText
            $Stage = "seed_font_apply"
            Write-Stage "running" $Stage
            $range.Font.Name = $FontName
            $range.Font.Size = $FontSize
            $Stage = "seed_layout_snapshot"
            Write-Stage "running" $Stage
            $snapshot = Snapshot $shape "seed_layout"
            $Stage = "seed_fixed_source_assert"
            Write-Stage "running" $Stage
            Assert-FixedSource $snapshot
            $Stage = "seed_identity"
            Write-Stage "running" $Stage
            $shapeId = [int64]$shape.ID
            $pageId = [int64]$page.PageID
            $index = [int]$page.Shapes.Count
            $Stage = "seed_save"
            Write-Stage "running" $Stage
            $doc.Save()
        } finally {
            Release-Com $range
            Release-Com $frame
            Release-Com $shape
            Close-Document $doc
            Close-PubPublisherApplication $app
        }
        $Stage = "seed_fresh_reopen"
        Write-Stage "running" $Stage
        $app2 = $null; $doc2 = $null; $shape2 = $null
        try {
            $app2 = New-PubPublisherApplication
            $doc2 = $app2.Open($SeedPath,$true,$false)
            $shape2 = Get-ShapeByIdentity $doc2 ([pscustomobject]@{page_id=$pageId;shape_id=$shapeId;shape_index=$index})
            $snapshotFresh = Snapshot $shape2
            Assert-FixedSource $snapshotFresh
        } finally {
            Release-Com $shape2
            Close-Document $doc2
            Close-PubPublisherApplication $app2
        }
        $Stage = "seed_receipt"
        Write-Stage "running" $Stage
        if ((File-Sha $SourcePath) -ne $sourceSha) {
            throw "m1_source_mutated_during_seed"
        }
        Write-PubJson -Path $SeedMetaPath -Value ([ordered]@{
            schema = "chaptera.text-width-m1-seed.v1"
            source_sha256 = $sourceSha
            seed_sha256 = File-Sha $SeedPath
            page_id = $pageId
            shape_id = $shapeId
            shape_index = $index
            snapshot_before_save = $snapshot
            snapshot_fresh_reopen = $snapshotFresh
            synthetic_text_only = $true
        })
    } else {
        $expectedWidths = @{
            "control-before" = 160.0
            "narrow" = 148.0
            "wide" = 172.0
            "control-after" = 160.0
        }
        if (-not $expectedWidths.ContainsKey($ArmId) -or
            [math]::Abs($WidthPt - [double]$expectedWidths[$ArmId]) -gt 0.00001) {
            throw "m1_arm_not_allowlisted"
        }
        $seed = Get-Content -LiteralPath $SeedMetaPath -Raw | ConvertFrom-Json
        if ($seed.schema -ne "chaptera.text-width-m1-seed.v1" -or
            [string]$seed.seed_sha256 -ne $sourceSha) {
            throw "m1_seed_identity_invalid"
        }
        $armDir = Join-Path $PrivateDir $ArmId
        New-Item -ItemType Directory -Force -Path $armDir | Out-Null
        $armPath = Join-Path $armDir "output.pub"
        Copy-Item -LiteralPath $SourcePath -Destination $armPath -Force
        $Stage = "arm_publisher_open"
        Write-Stage "running" $Stage
        $app = $null; $doc = $null; $shape = $null
        try {
            $app = New-PubPublisherApplication
            $doc = $app.Open($armPath,$false,$false)
            $shape = Get-ShapeByIdentity $doc $seed
            $before = Snapshot $shape
            Assert-FixedSource $before
            if ($before.text_utf16le_sha256 -ne [string]$seed.snapshot_fresh_reopen.text_utf16le_sha256) {
                throw "m1_seed_text_changed_before_arm"
            }
            $shape.Width = $WidthPt
            $after = Snapshot $shape
            Assert-FixedSource $after
            if ($after.text_utf16le_sha256 -ne $before.text_utf16le_sha256) {
                throw "m1_width_mutation_changed_text"
            }
            $Stage = "arm_save"
            Write-Stage "running" $Stage
            $doc.Save()
        } finally {
            Release-Com $shape
            Close-Document $doc
            Close-PubPublisherApplication $app
        }
        $Stage = "arm_fresh_reopen"
        Write-Stage "running" $Stage
        $app2 = $null; $doc2 = $null; $shape2 = $null
        try {
            $app2 = New-PubPublisherApplication
            $doc2 = $app2.Open($armPath,$true,$false)
            $shape2 = Get-ShapeByIdentity $doc2 $seed
            $fresh = Snapshot $shape2
            Assert-FixedSource $fresh
            if ($fresh.text_utf16le_sha256 -ne $before.text_utf16le_sha256) {
                throw "m1_save_reopen_changed_text"
            }
        } finally {
            Release-Com $shape2
            Close-Document $doc2
            Close-PubPublisherApplication $app2
        }
        $Stage = "arm_receipt"
        Write-Stage "running" $Stage
        if ([math]::Abs([double]$fresh.width_pt - $WidthPt) -gt 0.001 -or
            [math]::Abs([double]$after.width_pt - $WidthPt) -gt 0.001 -or
            (File-Sha $SourcePath) -ne $sourceSha) {
            throw "m1_width_or_seed_drift"
        }
        Write-PubJson -Path (Join-Path $AnalysisDir ("text-width-m1-" + $ArmId + ".json")) -Value ([ordered]@{
            schema = "chaptera.text-width-m1-arm.v1"
            experiment_id = "TEXT-WIDTH-BREAKPOINT-M1-01"
            arm_id = $ArmId
            requested_width_pt = $WidthPt
            source_sha256 = [string]$seed.source_sha256
            seed_sha256 = $sourceSha
            output_sha256 = File-Sha $armPath
            before = $before
            after = $after
            fresh_reopen = $fresh
            original_seed_preserved = $true
            raw_pub_uploaded = $false
        })
    }
    $Status = "complete"
    Write-Stage $Status "complete"
} catch {
    # Public diagnostics contain only the current phase and numeric HRESULT,
    # never exception text, private paths, or user document contents.
    $hresult = ""
    if ($null -ne $_.Exception) {
        $hresult = ('0x{0:X8}' -f (([long]$_.Exception.HResult) -band 4294967295L))
    }
    Write-Stage "invalid" $Stage $hresult
    throw
}
