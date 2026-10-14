param(
    [Parameter(Mandatory = $true)]
    [string]$PubPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-Sha256Text {
    param([AllowNull()][string]$Text)
    if ($null -eq $Text) { return $null }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Read-ComValue {
    param([scriptblock]$Getter)
    try { return (& $Getter) } catch { return $null }
}

function Read-Font {
    param($Font)
    if ($null -eq $Font) { return $null }
    $name = Read-ComValue { [string]$Font.Name }
    return [ordered]@{
        name_sha256 = Get-Sha256Text $name
        size_pt = Read-ComValue { [double]$Font.Size }
        size_bi_pt = Read-ComValue { [double]$Font.SizeBi }
        bold = Read-ComValue { [int]$Font.Bold }
        bold_bi = Read-ComValue { [int]$Font.BoldBi }
        italic = Read-ComValue { [int]$Font.Italic }
        italic_bi = Read-ComValue { [int]$Font.ItalicBi }
        kerning_pt = Read-ComValue { [double]$Font.Kerning }
        tracking_pt = Read-ComValue { [double]$Font.Tracking }
        scaling = Read-ComValue { [double]$Font.Scaling }
    }
}

function Read-ParagraphFormat {
    param($ParagraphFormat)
    if ($null -eq $ParagraphFormat) { return $null }
    $textStyle = Read-ComValue { [string]$ParagraphFormat.TextStyle }
    return [ordered]@{
        text_style_sha256 = Get-Sha256Text $textStyle
        alignment = Read-ComValue { [int]$ParagraphFormat.Alignment }
        line_spacing = Read-ComValue { [double]$ParagraphFormat.LineSpacing }
        line_spacing_rule = Read-ComValue { [int]$ParagraphFormat.LineSpacingRule }
        space_before_pt = Read-ComValue { [double]$ParagraphFormat.SpaceBefore }
        space_after_pt = Read-ComValue { [double]$ParagraphFormat.SpaceAfter }
        first_line_indent_pt = Read-ComValue { [double]$ParagraphFormat.FirstLineIndent }
        left_indent_pt = Read-ComValue { [double]$ParagraphFormat.LeftIndent }
        right_indent_pt = Read-ComValue { [double]$ParagraphFormat.RightIndent }
        lock_to_baseline = Read-ComValue { [int]$ParagraphFormat.LockToBaseLine }
        start_in_next_text_box = Read-ComValue { [int]$ParagraphFormat.StartInNextTextBox }
        widow_control = Read-ComValue { [int]$ParagraphFormat.WidowControl }
    }
}

$resolvedPub = (Resolve-Path -LiteralPath $PubPath).Path
$outputFullPath = [System.IO.Path]::GetFullPath($OutputPath)
$outputDirectory = [System.IO.Path]::GetDirectoryName($outputFullPath)
if ($outputDirectory -and -not (Test-Path -LiteralPath $outputDirectory)) {
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
}

$sourceInfo = Get-Item -LiteralPath $resolvedPub
$sourceHash = (Get-FileHash -LiteralPath $resolvedPub -Algorithm SHA256).Hash.ToLowerInvariant()
$app = $null
$doc = $null

try {
    $app = New-Object -ComObject Publisher.Application
    $doc = $app.Open($resolvedPub, $true, $false)
    $pageRows = @()

    for ($pageIndex = 1; $pageIndex -le [int]$doc.Pages.Count; $pageIndex++) {
        $page = $doc.Pages.Item($pageIndex)
        $shapeRows = @()

        for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
            $shape = $page.Shapes.Item($shapeIndex)
            try {
                $textFrame = $shape.TextFrame
                $textRange = $textFrame.TextRange
            }
            catch {
                continue
            }

            $text = Read-ComValue { [string]$textRange.Text }
            $storyText = Read-ComValue { [string]$textFrame.Story.TextRange.Text }
            $font = $null
            $paragraph = $null
            try { $font = $textRange.Font } catch {}
            try { $paragraph = $textRange.ParagraphFormat } catch {}
            $shapeName = Read-ComValue { [string]$shape.Name }

            $shapeRows += [ordered]@{
                shape_index = $shapeIndex
                shape_id = Read-ComValue { [int64]$shape.ID }
                shape_name_sha256 = Get-Sha256Text $shapeName
                shape_type = Read-ComValue { [int]$shape.Type }
                bounds_pt = [ordered]@{
                    left = Read-ComValue { [double]$shape.Left }
                    top = Read-ComValue { [double]$shape.Top }
                    width = Read-ComValue { [double]$shape.Width }
                    height = Read-ComValue { [double]$shape.Height }
                }
                text_length = if ($null -eq $text) { $null } else { $text.Length }
                text_sha256 = Get-Sha256Text $text
                story_text_length = if ($null -eq $storyText) { $null } else { $storyText.Length }
                story_text_sha256 = Get-Sha256Text $storyText
                font = Read-Font $font
                paragraph = Read-ParagraphFormat $paragraph
                text_frame = [ordered]@{
                    has_text = Read-ComValue { [int]$textFrame.HasText }
                    margin_left_pt = Read-ComValue { [double]$textFrame.MarginLeft }
                    margin_right_pt = Read-ComValue { [double]$textFrame.MarginRight }
                    margin_top_pt = Read-ComValue { [double]$textFrame.MarginTop }
                    margin_bottom_pt = Read-ComValue { [double]$textFrame.MarginBottom }
                    columns = Read-ComValue { [int]$textFrame.Columns }
                    column_spacing_pt = Read-ComValue { [double]$textFrame.ColumnSpacing }
                    auto_fit_text = Read-ComValue { [int]$textFrame.AutoFitText }
                    overflowing = Read-ComValue { [int]$textFrame.Overflowing }
                    orientation = Read-ComValue { [int]$textFrame.Orientation }
                    vertical_text_alignment = Read-ComValue { [int]$textFrame.VerticalTextAlignment }
                    has_next_link = Read-ComValue { [int]$textFrame.HasNextLink }
                    has_previous_link = Read-ComValue { [int]$textFrame.HasPreviousLink }
                }
            }
        }

        $pageRows += [ordered]@{
            page_index = $pageIndex
            page_id = Read-ComValue { [int64]$page.PageID }
            page_number = Read-ComValue { [int]$page.PageNumber }
            page_type = Read-ComValue { [int]$page.PageType }
            width_pt = Read-ComValue { [double]$page.Width }
            height_pt = Read-ComValue { [double]$page.Height }
            text_frames = $shapeRows
        }
    }

    $styleRows = @()
    for ($styleIndex = 1; $styleIndex -le [int]$doc.TextStyles.Count; $styleIndex++) {
        $style = $doc.TextStyles.Item($styleIndex)
        $styleName = Read-ComValue { [string]$style.Name }
        $baseStyle = Read-ComValue { [string]$style.BaseStyle }
        $styleFont = $null
        $styleParagraph = $null
        try { $styleFont = $style.Font } catch {}
        try { $styleParagraph = $style.ParagraphFormat } catch {}

        $styleRows += [ordered]@{
            style_index = $styleIndex
            name_sha256 = Get-Sha256Text $styleName
            base_style_sha256 = Get-Sha256Text $baseStyle
            font = Read-Font $styleFont
            paragraph = Read-ParagraphFormat $styleParagraph
        }
    }

    $receipt = [ordered]@{
        schema_version = "chaptera.publisher-semantic-dump.v1"
        source_sha256 = $sourceHash
        source_byte_len = [int64]$sourceInfo.Length
        publisher = [ordered]@{
            version = Read-ComValue { [string]$app.Version }
            build = Read-ComValue { [string]$app.Build }
        }
        page_count = [int]$doc.Pages.Count
        story_count = [int]$doc.Stories.Count
        text_style_count = [int]$doc.TextStyles.Count
        pages = $pageRows
        text_styles = $styleRows
    }

    $json = $receipt | ConvertTo-Json -Depth 12
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($outputFullPath, $json + [Environment]::NewLine, $utf8NoBom)
    Write-Host ("publisher semantic dump: pages={0} stories={1} text_styles={2} output={3}" -f $receipt.page_count, $receipt.story_count, $receipt.text_style_count, $outputFullPath)
}
finally {
    if ($null -ne $doc) {
        try { $doc.Close() } catch {}
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($doc) } catch {}
    }
    if ($null -ne $app) {
        try { $app.Quit() } catch {}
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($app) } catch {}
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
