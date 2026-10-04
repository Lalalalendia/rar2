Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

function Start-EncodedPowerShell([string]$Script,[string[]]$ChildArgs,[string]$Stdout,[string]$Stderr){
    $scriptB64=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Script))
    $argsJson=ConvertTo-Json -InputObject @($ChildArgs) -Compress
    $argsB64=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($argsJson))
    $command = @"
`$ErrorActionPreference='Stop'
try {
  `$script=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$scriptB64'))
  `$json=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$argsB64'))
  [object[]]`$tokens=@(ConvertFrom-Json -InputObject `$json)
  if((`$tokens.Count % 2) -ne 0){ throw 'child argument vector must be name/value pairs' }
  `$named=@{}
  for(`$i=0; `$i -lt `$tokens.Count; `$i+=2){
    `$name=[string]`$tokens[`$i]
    if(`$name -notmatch '^-([A-Za-z][A-Za-z0-9_-]*)$'){ throw ('invalid named parameter token: '+`$name) }
    `$named[`$Matches[1]]=`$tokens[`$i+1]
  }
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
    $psi.Arguments='-NoLogo -NoProfile -ExecutionPolicy Bypass -EncodedCommand '+$encoded
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

function Complete-EncodedPowerShell($Process){
    $Process.WaitForExit()
    $Process.ChapteraStdoutTask.Wait()
    $Process.ChapteraStderrTask.Wait()
    [IO.File]::WriteAllText([string]$Process.ChapteraStdoutPath,[string]$Process.ChapteraStdoutTask.Result,(New-Object Text.UTF8Encoding($false)))
    [IO.File]::WriteAllText([string]$Process.ChapteraStderrPath,[string]$Process.ChapteraStderrTask.Result,(New-Object Text.UTF8Encoding($false)))
    return [int]$Process.ExitCode
}

$base=Join-Path $env:TEMP ('paragraph-v17-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $base | Out-Null
try {
  $ok=Join-Path $base 'ok child.ps1'
  @(
    'param([string]$PacketPath,[string]$OutputRoot)',
    "if(`$PacketPath -ne 'path with spaces' -or `$OutputRoot -ne 'second value'){exit 7}",
    'exit 0'
  ) | Set-Content -LiteralPath $ok -Encoding ASCII
  $o=Join-Path $base 'ok.out';$e=Join-Path $base 'ok.err'
  $p=Start-EncodedPowerShell $ok @('-PacketPath','path with spaces','-OutputRoot','second value') $o $e
  $code=Complete-EncodedPowerShell $p
  if($code -ne 0){throw "positive failed exit=$code err=$(Get-Content $e -Raw -ErrorAction SilentlyContinue)"}

  $exit7=Join-Path $base 'exit7 child.ps1'
  @('param([string]$X)','exit 7') | Set-Content -LiteralPath $exit7 -Encoding ASCII
  $o2=Join-Path $base 'e7.out';$e2=Join-Path $base 'e7.err'
  $p2=Start-EncodedPowerShell $exit7 @('-X','x') $o2 $e2
  $code2=Complete-EncodedPowerShell $p2
  if($code2 -ne 7){throw "exit7 propagation failed: $code2 err=$(Get-Content $e2 -Raw -ErrorAction SilentlyContinue)"}

  $fail=Join-Path $base 'throw child.ps1'
  @('param([string]$X)','throw ''intentional smoke failure''') | Set-Content -LiteralPath $fail -Encoding ASCII
  $o3=Join-Path $base 'f.out';$e3=Join-Path $base 'f.err'
  $p3=Start-EncodedPowerShell $fail @('-X','x') $o3 $e3
  $code3=Complete-EncodedPowerShell $p3
  if($code3 -eq 0){throw 'throw propagation returned zero'}

  Write-Host "PARAGRAPH V17 PROCESS RUNNER PASS positive=$code exit7=$code2 throw=$code3"
} finally { Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue }
