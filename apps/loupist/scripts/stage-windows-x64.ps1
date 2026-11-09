$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

& (Join-Path $PSScriptRoot "stage-pi-windows-x64.ps1")
& (Join-Path $PSScriptRoot "stage-openbb-windows-x64.ps1")
& (Join-Path $PSScriptRoot "..\openbb\.venv\Scripts\python.exe") (Join-Path $PSScriptRoot "stage-embed-assets.py")
if ($LASTEXITCODE -ne 0) { throw "Embedding asset staging failed." }
& (Join-Path $PSScriptRoot "..\openbb\.venv\Scripts\python.exe") (Join-Path $PSScriptRoot "stage_python_runtime.py") "--ensure"
if ($LASTEXITCODE -ne 0) { throw "Python runtime staging failed." }
& (Join-Path $PSScriptRoot "check-package-prerequisites.ps1")

Write-Host "Loupist Windows x64 staging is complete."
