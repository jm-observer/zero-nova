@echo off
setlocal
set "URL=https://raw.githubusercontent.com/jm-observer/tool-template-rust/main/AGENTS.md"
set "TARGET=%~dp0..\..\AGENTS.md"
rem Download to target file in parent directory
powershell -Command "Invoke-WebRequest -Uri \"%URL%\" -OutFile \"%TARGET%\" -ErrorAction Stop"
if errorlevel 1 (
  echo Download failed
  exit /b 1
)

echo AGENTS.md updated
pause