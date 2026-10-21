@echo off
setlocal
pushd "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %*
set "exit_code=%errorlevel%"
echo.
if not "%exit_code%"=="0" (
    echo Uninstall failed with exit code %exit_code%.
) else (
    echo R1fc Convert was uninstalled.
)
pause
popd
exit /b %exit_code%
