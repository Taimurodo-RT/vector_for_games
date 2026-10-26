@echo off
rem Пересобирает интерфейс приложений из их .msc: app\app.msc -> app\ui\*.tscn и тема.
rem motif.exe берётся из переменной MOTIF, иначе из папки Motif уровнем выше, иначе из PATH.
cd /d "%~dp0"
if "%MOTIF%"=="" if exist "..\motif.exe" set MOTIF=%~dp0..\motif.exe
if "%MOTIF%"=="" set MOTIF=motif.exe
for %%a in (calculator focus) do (
  if exist "%%a\ui" rmdir /s /q "%%a\ui"
  "%MOTIF%" screen "%%a\%%a.msc" -o "%%a\ui" || exit /b 1
)
