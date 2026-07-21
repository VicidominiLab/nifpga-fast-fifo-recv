@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\release.ps1" -Publish %*
if errorlevel 1 (
    echo.
    echo Wheel build or upload failed.
    exit /b 1
)

echo.
echo Wheels published successfully.
exit /b 0
