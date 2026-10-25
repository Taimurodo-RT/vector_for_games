@echo off
rem Updates Motif in this folder to the latest build from GitHub.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0update.ps1"
pause
