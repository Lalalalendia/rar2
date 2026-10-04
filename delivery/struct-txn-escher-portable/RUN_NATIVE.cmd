@echo off
setlocal
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0RUN_NATIVE.ps1"
set RC=%ERRORLEVEL%
echo.
if not "%RC%"=="0" echo FAILED with exit code %RC%.
if "%RC%"=="0" echo Completed. Upload the RETURN-TO-CHAT-STRUCT-TXN-ESCHER-*.zip file.
echo.
pause
exit /b %RC%
