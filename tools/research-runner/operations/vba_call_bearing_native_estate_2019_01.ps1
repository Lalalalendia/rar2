param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "VBA-CALL-BEARING-NATIVE-ESTATE-2019-01"
$PbFilePublication = 1
$VbextCtStdModule = 1

Import-Module (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "windows\pub-runtime\PubRuntime.psm1") -Force

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) { throw "Unexpected experiment id: $($packet.id)" }
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) { throw "PUB_RESEARCH_FIXTURE missing" }

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private\vba-call-bearing-native-estate-2019-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

$resultPath = Join-Path $analysisDir "vba-call-bearing-native-estate-2019-01.json"
$scanPath = Join-Path $analysisDir "vba-call-bearing-native-estate-scan.json"
$logPath = Join-Path $logDir "vba-call-bearing-native-estate-2019-01.txt"

$arms = @(
    [ordered]@{
        id = "document-text"
        module_name = "ChapteraDocumentText"
        output_leaf = "document-text.pub"
        required_families = @("documents","pages","shapes","text")
        source = @"
Option Explicit
Public Sub ChapteraDocumentTextProbe()
    Dim d As Document
    Dim p As Page
    Dim s As Shape
    Dim tr As TextRange
    Set d = ThisDocument
    Set p = d.Pages(1)
    Set s = p.Shapes(1)
    Set tr = s.TextFrame.TextRange
    Debug.Print tr.Text
End Sub
"@
    },
    [ordered]@{
        id = "table-layout"
        module_name = "ChapteraTableLayout"
        output_leaf = "table-layout.pub"
        required_families = @("documents","pages","shapes","tables","layout")
        source = @"
Option Explicit
Public Sub ChapteraTableLayoutProbe()
    Dim d As Document
    Dim p As Page
    Dim tbl As Table
    Set d = ActiveDocument
    Set p = d.Pages(1)
    Set tbl = p.Shapes(1).Table
    Debug.Print tbl.Rows.Count
    Debug.Print tbl.Columns.Count
    Debug.Print d.LayoutGuides.Rows
End Sub
"@
    },
    [ordered]@{
        id = "output-ole-metadata"
        module_name = "ChapteraOutputOle"
        output_leaf = "output-ole-metadata.pub"
        required_families = @("documents","pages","shapes","output","ole_links","metadata_selectors")
        source = @"
Option Explicit
Public Sub ChapteraOutputOleProbe()
    Dim d As Document
    Dim o As Object
    Set d = ThisDocument
    Set o = d.Pages(1).Shapes(1).OLEFormat
    Debug.Print d.Pages(1).Tags.Count
    d.ExportAsFixedFormat Format:=pbFixedFormatTypePDF, Filename:="chaptera-probe.pdf"
End Sub
"@
    }
)

function Release-Com($obj) {
    if ($null -eq $obj) { return }
    try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($obj) } catch {}
}

function Close-Document($doc) {
    if ($null -eq $doc) { return }
    try { $doc.Saved = $true } catch {}
    try { $doc.Close() } catch {}
    Release-Com $doc
}

function Get-BoundProject($app, $doc, [string]$inputPath) {
    $vbe = $null
    $project = $null
    $accessPath = $null

    try {
        $candidate = $doc.VBProject
        if ($null -ne $candidate) {
            $project = $candidate
            $accessPath = "Document.VBProject"
        }
    } catch {}

    if ($null -eq $project) {
        try {
            $vbe = $app.VBE
            $candidate = $vbe.ActiveVBProject
            if ($null -ne $candidate) {
                $project = $candidate
                $accessPath = "Application.VBE.ActiveVBProject"
            }
        } catch {
            Release-Com $vbe
            throw "Publisher VBE access is unavailable or blocked by Trust Center policy."
        }
    }

    if ($null -eq $project) {
        Release-Com $vbe
        throw "Publisher exposed no active VBProject for the opened publication."
    }

    if ($accessPath -eq "Application.VBE.ActiveVBProject") {
        $projectFileName = $null
        try { $projectFileName = [string]$project.FileName } catch {}
        $bound = $false
        if (-not [string]::IsNullOrWhiteSpace($projectFileName)) {
            try {
                $bound = [string]::Equals(
                    [IO.Path]::GetFullPath($projectFileName),
                    [IO.Path]::GetFullPath($inputPath),
                    [StringComparison]::OrdinalIgnoreCase
                )
            } catch {
                $bound = $false
            }
        }
        if (-not $bound) {
            Release-Com $project
            Release-Com $vbe
            throw "ActiveVBProject is not provably bound to the exact input publication."
        }
    }

    return [ordered]@{
        project = $project
        vbe = $vbe
        access_path = $accessPath
    }
}

$generatedRows = @()
foreach ($arm in $arms) {
    $input = Join-Path $privateDir ("input-" + [string]$arm.id + ".pub")
    $output = Join-Path $privateDir ([string]$arm.output_leaf)
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $input -Force
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Force }

    $source = [string]$arm.source
    $sourceSha256 = [Convert]::ToHexString(
        [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($source))
    ).ToLowerInvariant()

    $app = $null
    $doc = $null
    $project = $null
    $vbe = $null
    $component = $null
    $codeModule = $null
    $accessPath = $null

    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($input, $false, $false)
        try { $doc.Activate() } catch {}

        $boundProject = Get-BoundProject $app $doc $input
        $project = $boundProject.project
        $vbe = $boundProject.vbe
        $accessPath = [string]$boundProject.access_path

        $component = $project.VBComponents.Add($VbextCtStdModule)
        $component.Name = [string]$arm.module_name
        $codeModule = $component.CodeModule
        $codeModule.AddFromString($source)

        # Materialize only. Never execute VBA, call Application.Run, or invoke
        # any procedure from the document-derived project.
        $doc.SaveAs($output, $PbFilePublication, $false)
        if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
            throw "Publisher did not materialize arm $($arm.id)."
        }

        $item = Get-Item -LiteralPath $output
        $generatedRows += [ordered]@{
            id = [string]$arm.id
            output_leaf = [string]$arm.output_leaf
            source_sha256 = $sourceSha256
            required_families = @($arm.required_families)
            vbproject_access_path = $accessPath
            pub_sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
            byte_len = [int64]$item.Length
        }
    }
    finally {
        Release-Com $codeModule
        Release-Com $component
        Release-Com $project
        Release-Com $vbe
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

python tools/pub_vba_estate_scan.py scan --input $privateDir --output $scanPath
if ($LASTEXITCODE -ne 0) { throw "Inert VBA scanner failed with exit code $LASTEXITCODE" }

$scan = Get-Content -LiteralPath $scanPath -Raw | ConvertFrom-Json
if ([string]$scan.call_classifier_version -ne "v2.1") {
    throw "Expected classifier v2.1, got $($scan.call_classifier_version)"
}
if ([int]$scan.totals.file_count -ne 6) {
    # The private directory also contains the three pristine input copies.
    throw "Expected six PUB files in the private directory, got $($scan.totals.file_count)"
}

$rowsBySha = @{}
foreach ($row in @($scan.files)) {
    if ($null -ne $row.sha256) {
        $rowsBySha[[string]$row.sha256] = $row
    }
}

$armReceipts = @()
foreach ($generated in $generatedRows) {
    if (-not $rowsBySha.ContainsKey([string]$generated.pub_sha256)) {
        throw "Scanner receipt missing generated arm $($generated.id)"
    }
    $row = $rowsBySha[[string]$generated.pub_sha256]
    if ([string]$row.vba_state -notin @("source_extracted","source_partial")) {
        throw "Arm $($generated.id) has unexpected VBA state $($row.vba_state)"
    }
    if ([int]$row.vba_project_count -lt 1) {
        throw "Arm $($generated.id) has no structurally admitted VBA project"
    }

    $missing = @()
    foreach ($family in @($generated.required_families)) {
        $prop = $row.call_families.PSObject.Properties[[string]$family]
        if ($null -eq $prop -or [int]$prop.Value -lt 1) {
            $missing += [string]$family
        }
    }
    if ($missing.Count -ne 0) {
        throw "Arm $($generated.id) missing required families: $($missing -join ', ')"
    }

    $armReceipts += [ordered]@{
        id = [string]$generated.id
        pub_sha256 = [string]$generated.pub_sha256
        byte_len = [int64]$generated.byte_len
        source_sha256 = [string]$generated.source_sha256
        vbproject_access_path = [string]$generated.vbproject_access_path
        required_families = @($generated.required_families)
        observed_call_families = $row.call_families
        observed_symbols = $row.symbols
        vba_state = [string]$row.vba_state
        vba_project_count = [int]$row.vba_project_count
    }
}

$uniqueHashes = @($armReceipts | ForEach-Object { $_.pub_sha256 } | Sort-Object -Unique)
$pass = (
    $armReceipts.Count -eq 3 -and
    $uniqueHashes.Count -eq 3 -and
    [bool]$scan.claims.vba_executed -eq $false -and
    [bool]$scan.claims.ole_com_activated -eq $false -and
    [bool]$scan.claims.source_text_emitted -eq $false
)

$result = [ordered]@{
    schema = "pub-vba-call-bearing-native-estate-2019-01/v1"
    experiment_id = $ExpectedExperiment
    verdict = if ($pass) { "call-bearing-native-estate-confirmed" } else { "call-bearing-native-estate-inconclusive" }
    call_classifier_version = [string]$scan.call_classifier_version
    generated_pub_private_local = $true
    generated_pub_count = $armReceipts.Count
    hash_unique_pub_count = $uniqueHashes.Count
    macro_execution_invoked = $false
    trust_settings_modified = $false
    source_text_emitted = $false
    arms = $armReceipts
}

Write-PubJson -Value $result -Path $resultPath
@(
    "experiment=$ExpectedExperiment",
    "verdict=$($result.verdict)",
    "classifier=$($result.call_classifier_version)",
    "generated_pub_count=$($result.generated_pub_count)",
    "hash_unique_pub_count=$($result.hash_unique_pub_count)",
    "macro_execution_invoked=false",
    "trust_settings_modified=false",
    "source_text_emitted=false",
    "private_pub_uploaded=false"
) | Set-Content -LiteralPath $logPath -Encoding ASCII

if (-not $pass) {
    throw "Controlled call-bearing Publisher estate did not satisfy its source-free contract."
}
