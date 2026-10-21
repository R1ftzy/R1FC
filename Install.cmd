@echo off
setlocal
pushd "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
set "exit_code=%errorlevel%"
echo.
if not "%exit_code%"=="0" (
    echo Installation failed with exit code %exit_code%.
) else (
    echo Installation finished.
)
pause
popd
exit /b %exit_code%
