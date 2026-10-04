Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

function Read-SharedProgressTail([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $stream = $null
    $reader = $null
    try {
        $stream = New-Object IO.FileStream(
            $Path,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite
        )
        $reader = New-Object IO.StreamReader($stream, [Text.Encoding]::UTF8, $true)
        $text = $reader.ReadToEnd()
        $lines = @($text -split "`r?`n" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
        if ($lines.Count -eq 0) { return $null }
        return [string]$lines[$lines.Count - 1]
    }
    catch { return $null }
    finally {
        if ($null -ne $reader) { $reader.Dispose() }
        elseif ($null -ne $stream) { $stream.Dispose() }
    }
}

$base=Join-Path $env:TEMP ('paragraph-v22-progress-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $base | Out-Null
try {
    $progress=Join-Path $base 'progress file.txt'
    $writer=Join-Path $base 'writer.ps1'
    @'
param([string]$Path)
$ErrorActionPreference='Stop'
for($i=1;$i -le 500;$i++){
  ((Get-Date).ToString("o") + "`tstage_" + $i) | Add-Content -LiteralPath $Path -Encoding UTF8
  Start-Sleep -Milliseconds 4
}
exit 0
'@ | Set-Content -LiteralPath $writer -Encoding ASCII

    $p=Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList @('-NoLogo','-NoProfile','-ExecutionPolicy','Bypass','-File',$writer,'-Path',$progress) -PassThru -WindowStyle Hidden
    $reads=0
    while(-not $p.HasExited){
        [void](Read-SharedProgressTail $progress)
        $reads++
        Start-Sleep -Milliseconds 2
        $p.Refresh()
    }
    $p.WaitForExit()
    if([int]$p.ExitCode -ne 0){ throw "writer failed exit=$($p.ExitCode)" }
    $tail=[string](Read-SharedProgressTail $progress)
    if($tail -notmatch 'stage_500$'){ throw "unexpected final tail: $tail" }
    Write-Host "PARAGRAPH V22 PROGRESS SHARE PASS reads=$reads tail=$tail"
}
finally {
    Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue
}
