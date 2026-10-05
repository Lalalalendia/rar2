@echo off
setlocal
cd /d "%~dp0"
echo.
echo QUILL SCHEME TEXT COLOR AUTHORITY - PORTABLE V2
echo No repo, Git, Python, Cargo, PS7, network, or GitHub runner required.
echo Save and close Publisher documents before continuing.
echo.
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-native.ps1"
set RC=%ERRORLEVEL%
echo.
if not "%RC%"=="0" (
  echo FAILED OR PARTIAL. Send RETURN-TO-CHAT zip if one was produced.
) else (
  echo COMPLETE. Send RETURN-TO-CHAT zip back to ChatGPT.
)
echo.
pause
exit /b %RC%
