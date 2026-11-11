# Runs the self-test (two real claude sessions, tear-out, clean exit) and captures the windows
# before and after the tear-out.
# Usage: powershell -File scripts\selftest.ps1 <folder for tile 1> [<folder for tile 2>] [-Release]
param(
    [Parameter(Mandatory = $true)][string]$Tile1,
    [string]$Tile2 = $Tile1,
    [switch]$Release,
    [string[]]$Extra = @()
)

$root = Split-Path $PSScriptRoot -Parent
$profile = if ($Release) { 'release' } else { 'debug' }
$exe = Join-Path $root "target\$profile\reframe-native.exe"
$out = Join-Path $env:TEMP 'rfn-selftest'
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null

# An automation shell may set NO_COLOR; claude would then draw without colours.
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue

$arguments = @('--selftest', '--tile', "`"$Tile1`"", '--tile', "`"$Tile2`"") + $Extra
$process = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru `
    -RedirectStandardOutput "$out\stdout.log" -RedirectStandardError "$out\stderr.log"
Start-Sleep 8
& powershell -NoProfile -File "$PSScriptRoot\capture.ps1" "$out\before"
Start-Sleep 9
& powershell -NoProfile -File "$PSScriptRoot\capture.ps1" "$out\after"
$process.WaitForExit(30000) | Out-Null
"exited: $($process.HasExited)"
Get-Content "$out\stdout.log" -Encoding UTF8
"--- stderr"
Get-Content "$out\stderr.log" -Encoding UTF8 | Select-Object -First 20
