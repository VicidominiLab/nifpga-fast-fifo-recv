@echo off
setlocal

set "PATH=C:\msys64\ucrt64\bin;%PATH%"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\release.ps1" %*
if errorlevel 1 (
    echo.
    echo Wheel build failed.
    exit /b 1
)

echo.
echo Wheel build and validation completed successfully.
exit /b 0
