@echo off
setlocal
cd /d "%~dp0"
echo.
echo PUB2019 PORTABLE P0
echo Self-contained offline bundle. No repo, Git, Python, Cargo, PS7 or network required.
echo Close Publisher before continuing.
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
