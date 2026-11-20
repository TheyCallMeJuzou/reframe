# What has to pass before a push: formatting, clippy, the unit tests and the end-to-end tests.
# Usage: powershell -File scripts\check.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

function Step($name, $command) {
    Write-Host "== $name"
    & cargo @command
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $name"; exit 1 }
}

Step 'format' @('fmt', '--all', '--check')
Step 'clippy' @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
Step 'tests' @('test', '--workspace')
Write-Host 'all checks passed'
