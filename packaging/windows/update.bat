@echo off
setlocal
rem Windows PowerShell must rebuild its own module path when called through cmd from pwsh.
set "PSModulePath="
rem Updates Motif in this folder to the latest build from GitHub.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0update.ps1" %*
set "motif_update_exit=%errorlevel%"
pause
exit /b %motif_update_exit%
