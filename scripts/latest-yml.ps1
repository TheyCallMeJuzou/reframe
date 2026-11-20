# Writes latest.yml for an installer, as electron-builder writes it: the updaters (reframe's own, and the
# electron-updater of installed 0.x copies) read the version from it and check the downloaded installer by its
# SHA-512 and size.
# Usage: powershell -File scripts\latest-yml.ps1 -Setup dist\reframe-Setup-1.0.0.exe -Version 1.0.0 [-Out dist\latest.yml]
param(
    [Parameter(Mandatory = $true)][string]$Setup,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Out = ''
)
$ErrorActionPreference = 'Stop'
if (-not $Out) { $Out = Join-Path (Split-Path $Setup -Parent) 'latest.yml' }
$name = Split-Path $Setup -Leaf
$bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $Setup))
$sha512 = [Convert]::ToBase64String([System.Security.Cryptography.SHA512]::Create().ComputeHash($bytes))
$date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'")
$yml = "version: $Version`nfiles:`n  - url: $name`n    sha512: $sha512`n    size: $($bytes.Length)`npath: $name`nsha512: $sha512`nreleaseDate: '$date'`n"
[System.IO.File]::WriteAllText($Out, $yml)
