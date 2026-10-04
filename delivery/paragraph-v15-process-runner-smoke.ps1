Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

function Start-NamedPowerShell([string]$Script,[hashtable]$NamedArgs,[string]$Stdout,[string]$Stderr){
    $scriptB64=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Script))
    $json=ConvertTo-Json -InputObject $NamedArgs -Compress
    $argsB64=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
    $command = @"
`$ErrorActionPreference='Stop'
try {
  `$script=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$scriptB64'))
  `$json=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$argsB64'))
  `$obj=ConvertFrom-Json -InputObject `$json
  `$named=@{}
  foreach(`$p in `$obj.PSObject.Properties){ `$named[[string]`$p.Name]=`$p.Value }
  & `$script @named
  if(-not `$?) { exit 1 }
  exit 0
} catch {
  [Console]::Error.WriteLine(`$_.Exception.ToString())
  exit 1
}
"@
    $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $psi=New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName=(Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe')
    $psi.Arguments='-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand '+$encoded
    $psi.UseShellExecute=$false
    $psi.CreateNoWindow=$true
    $psi.RedirectStandardOutput=$true
    $psi.RedirectStandardError=$true
    $p=New-Object System.Diagnostics.Process
    $p.StartInfo=$psi
    [void]$p.Start()
    $outTask=$p.StandardOutput.ReadToEndAsync()
    $errTask=$p.StandardError.ReadToEndAsync()
    $p | Add-Member -NotePropertyName ChapteraStdoutTask -NotePropertyValue $outTask
    $p | Add-Member -NotePropertyName ChapteraStderrTask -NotePropertyValue $errTask
    $p | Add-Member -NotePropertyName ChapteraStdoutPath -NotePropertyValue $Stdout
    $p | Add-Member -NotePropertyName ChapteraStderrPath -NotePropertyValue $Stderr
    return $p
}

function Complete-NamedPowerShell($Process){
    $Process.WaitForExit()
    $Process.ChapteraStdoutTask.Wait()
    $Process.ChapteraStderrTask.Wait()
    [IO.File]::WriteAllText([string]$Process.ChapteraStdoutPath,[string]$Process.ChapteraStdoutTask.Result,(New-Object Text.UTF8Encoding($false)))
    [IO.File]::WriteAllText([string]$Process.ChapteraStderrPath,[string]$Process.ChapteraStderrTask.Result,(New-Object Text.UTF8Encoding($false)))
    return [int]$Process.ExitCode
}

$base=Join-Path $env:TEMP ('paragraph-v18-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $base | Out-Null
try {
  $ok=Join-Path $base 'ok child.ps1'
  @(
    'param([string]$PacketPath,[string]$OutputRoot)',
    "if(`$PacketPath -ne 'path with spaces' -or `$OutputRoot -ne 'second value'){exit 7}",
    'exit 0'
  ) | Set-Content -LiteralPath $ok -Encoding ASCII
  $o=Join-Path $base 'ok.out';$e=Join-Path $base 'ok.err'
  $p=Start-NamedPowerShell $ok @{PacketPath='path with spaces';OutputRoot='second value'} $o $e
  $code=Complete-NamedPowerShell $p
  if($code -ne 0){throw "positive failed exit=$code err=$(Get-Content $e -Raw -ErrorAction SilentlyContinue)"}

  $exit7=Join-Path $base 'exit7 child.ps1'
  @('param([string]$X)','exit 7') | Set-Content -LiteralPath $exit7 -Encoding ASCII
  $o2=Join-Path $base 'e7.out';$e2=Join-Path $base 'e7.err'
  $p2=Start-NamedPowerShell $exit7 @{X='x'} $o2 $e2
  $code2=Complete-NamedPowerShell $p2
  if($code2 -eq 0){throw "explicit child failure incorrectly returned success: $code2 err=$(Get-Content $e2 -Raw -ErrorAction SilentlyContinue)"}

  $fail=Join-Path $base 'throw child.ps1'
  @('param([string]$X)','throw ''intentional smoke failure''') | Set-Content -LiteralPath $fail -Encoding ASCII
  $o3=Join-Path $base 'f.out';$e3=Join-Path $base 'f.err'
  $p3=Start-NamedPowerShell $fail @{X='x'} $o3 $e3
  $code3=Complete-NamedPowerShell $p3
  if($code3 -eq 0){throw 'throw propagation returned zero'}

  Write-Host "PARAGRAPH V19 PROCESS RUNNER PASS positive=$code explicit_failure=$code2 throw=$code3"
} finally { Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue }
