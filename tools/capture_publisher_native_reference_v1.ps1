[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$NewsletterPub,

    [Parameter(Mandatory = $true)]
    [string]$BrochurePub,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    [string]$ExpectedPublisherVersion = "16.0",
    [string]$ExpectedPublisherBuild = "12527",
    [string]$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedNewsletterSha256 = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
$ExpectedBrochureSha256 = "ffed034ac87e679f0bd08ff9cf74ad11c0e510a42b1bc1a7502415f6c29c87"
$ExpectedNewsletterBytes = 291840
$ExpectedBrochureBytes = 161792
$ExpectedNewsletterPages = 4
$ExpectedBrochurePages = 2
$PbFixedFormatTypePdf = 2

function Get-Sha256Lower {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]$Actual,
        [Parameter(Mandatory = $true)]$Expected,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($Actual -ne $Expected) {
        throw "$Label mismatch"
    }
}

function Get-PublisherExecutablePath {
    $processes = @(Get-Process -Name MSPUB -ErrorAction Stop)
    if ($processes.Count -ne 1) {
        throw "Expected exactly one Publisher process created by the oracle harness"
    }

    $path = $processes[0].Path
    if ([string]::IsNullOrWhiteSpace($path)) {
        $path = $processes[0].MainModule.FileName
    }
    if ([string]::IsNullOrWhiteSpace($path)) {
        throw "Publisher executable path is unavailable"
    }
    return $path
}

if ([Environment]::Is64BitOperatingSystem -and [Environment]::Is64BitProcess) {
    throw "Run this harness from 32-bit Windows PowerShell: %WINDIR%\SysWOW64\WindowsPowerShell\v1.0\powershell.exe"
}

$newsletterPath = [IO.Path]::GetFullPath($NewsletterPub)
$brochurePath = [IO.Path]::GetFullPath($BrochurePub)
$outputPath = [IO.Path]::GetFullPath($OutputDir)

foreach ($path in @($newsletterPath, $brochurePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required PUB fixture is missing"
    }
}

$existingPublisher = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
if ($existingPublisher.Count -ne 0) {
    throw "Close all Publisher processes before running the native oracle capture"
}

New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
$workRoot = Join-Path ([IO.Path]::GetTempPath()) ("chaptera-publisher-native-oracle-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $workRoot | Out-Null

$fixtures = @(
    [pscustomobject]@{
        name = "SampleNewsletter"
        source = $newsletterPath
        expected_sha256 = $ExpectedNewsletterSha256
        expected_bytes = $ExpectedNewsletterBytes
        expected_pages = $ExpectedNewsletterPages
    },
    [pscustomobject]@{
        name = "SampleBrochure"
        source = $brochurePath
        expected_sha256 = $ExpectedBrochureSha256
        expected_bytes = $ExpectedBrochureBytes
        expected_pages = $ExpectedBrochurePages
    }
)

$publisherApp = $null
$results = @()
$exitCode = 0
$failureType = $null
$publisherVersion = $null
$publisherBuild = $null
$publisherExeSha256 = $null
$cleanProcessExit = $false

try {
    foreach ($fixture in $fixtures) {
        $sourceItem = Get-Item -LiteralPath $fixture.source
        Assert-Equal $sourceItem.Length $fixture.expected_bytes "$($fixture.name) byte length"
        Assert-Equal (Get-Sha256Lower $fixture.source) $fixture.expected_sha256 "$($fixture.name) SHA-256"
    }

    $publisherApp = New-Object -ComObject Publisher.Application
    $publisherVersion = [string]$publisherApp.Version
    $publisherBuild = [string]$publisherApp.Build
    Assert-Equal $publisherVersion $ExpectedPublisherVersion "Publisher version"
    Assert-Equal $publisherBuild $ExpectedPublisherBuild "Publisher build"

    $publisherExe = Get-PublisherExecutablePath
    $publisherExeSha256 = Get-Sha256Lower $publisherExe
    Assert-Equal $publisherExeSha256 $ExpectedPublisherExeSha256 "Publisher executable SHA-256"

    foreach ($fixture in $fixtures) {
        $copyPath = Join-Path $workRoot ($fixture.name + ".pub")
        Copy-Item -LiteralPath $fixture.source -Destination $copyPath -Force
        Assert-Equal (Get-Sha256Lower $copyPath) $fixture.expected_sha256 "$($fixture.name) temporary copy SHA-256"

        $pdfPath = Join-Path $outputPath ($fixture.name + ".publisher.pdf")
        if (Test-Path -LiteralPath $pdfPath) {
            Remove-Item -LiteralPath $pdfPath -Force
        }

        $document = $null
        try {
            # Publisher.Application.Open supports ReadOnly and AddToRecentFiles.
            # The source copy is never saved or SaveAs'd; only a fixed-format PDF is emitted.
            $document = $publisherApp.Open($copyPath, $true, $false)
            if ($null -eq $document) {
                $document = $publisherApp.ActiveDocument
            }
            if ($null -eq $document) {
                throw "Publisher did not expose the opened document"
            }

            if (-not [bool]$document.ReadOnly) {
                throw "$($fixture.name) did not open read-only"
            }

            $pageCount = [int]$document.Pages.Count
            Assert-Equal $pageCount $fixture.expected_pages "$($fixture.name) page count"

            $pageWidthPt = [math]::Round([double]$document.PageSetup.PageWidth, 6)
            $pageHeightPt = [math]::Round([double]$document.PageSetup.PageHeight, 6)

            $document.ExportAsFixedFormat($PbFixedFormatTypePdf, $pdfPath)

            if (-not (Test-Path -LiteralPath $pdfPath -PathType Leaf)) {
                throw "$($fixture.name) PDF export was not created"
            }

            $pdfItem = Get-Item -LiteralPath $pdfPath
            if ($pdfItem.Length -le 0) {
                throw "$($fixture.name) PDF export is empty"
            }

            $sourceAfterSha256 = Get-Sha256Lower $fixture.source
            $copyAfterSha256 = Get-Sha256Lower $copyPath
            Assert-Equal $sourceAfterSha256 $fixture.expected_sha256 "$($fixture.name) source post-open SHA-256"
            Assert-Equal $copyAfterSha256 $fixture.expected_sha256 "$($fixture.name) temporary copy post-open SHA-256"

            $results += [ordered]@{
                fixture = $fixture.name
                source_pub_sha256 = $fixture.expected_sha256
                source_byte_len = $fixture.expected_bytes
                source_modified = $false
                opened_read_only = $true
                add_to_recent_files = $false
                page_count = $pageCount
                page_width_pt = $pageWidthPt
                page_height_pt = $pageHeightPt
                reference_pdf = [IO.Path]::GetFileName($pdfPath)
                reference_pdf_sha256 = Get-Sha256Lower $pdfPath
                reference_pdf_byte_len = $pdfItem.Length
            }
        }
        finally {
            if ($null -ne $document) {
                try {
                    $document.Close()
                }
                catch {
                    # Cleanup continues; process-exit verification below remains authoritative.
                }
                try {
                    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document)
                }
                catch {
                }
                $document = $null
            }
        }
    }
}
catch {
    $exitCode = 1
    $failureType = $_.Exception.GetType().FullName
}
finally {
    if ($null -ne $publisherApp) {
        try {
            $publisherApp.Quit()
        }
        catch {
            $exitCode = 1
            if ($null -eq $failureType) {
                $failureType = $_.Exception.GetType().FullName
            }
        }
        try {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($publisherApp)
        }
        catch {
        }
        $publisherApp = $null
    }

    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
    [GC]::Collect()

    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -eq 0) {
            $cleanProcessExit = $true
            break
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $cleanProcessExit) {
        $exitCode = 1
        if ($null -eq $failureType) {
            $failureType = "PublisherProcessDidNotExit"
        }
    }

    $receipt = [ordered]@{
        schema = "chaptera.cloud-reader-native-publisher-reference.v1"
        status = $(if ($exitCode -eq 0) { "success" } else { "failed" })
        publisher = [ordered]@{
            version = $publisherVersion
            build = $publisherBuild
            executable_sha256 = $publisherExeSha256
            expected_executable_sha256 = $ExpectedPublisherExeSha256
            clean_process_exit = $cleanProcessExit
        }
        capture = [ordered]@{
            fixed_format = "PDF"
            fixed_format_type = $PbFixedFormatTypePdf
            source_open_mode = "read_only"
            source_save_or_save_as_called = $false
            source_paths_emitted = $false
            raw_pub_bytes_emitted = $false
        }
        fixtures = $results
        failure_type = $failureType
    }

    $receiptPath = Join-Path $outputPath "publisher-native-reference-receipt.json"
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $receiptPath -Encoding UTF8

    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force
    }
}

if ($exitCode -ne 0) {
    Write-Error "Publisher native oracle capture failed; inspect publisher-native-reference-receipt.json"
    exit $exitCode
}

Write-Output "PUBLISHER NATIVE ORACLE CAPTURE COMPLETE"
Write-Output (Join-Path $outputPath "publisher-native-reference-receipt.json")
Write-Output (Join-Path $outputPath "SampleNewsletter.publisher.pdf")
Write-Output (Join-Path $outputPath "SampleBrochure.publisher.pdf")
