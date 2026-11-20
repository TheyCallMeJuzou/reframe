# Starts the app with the fake claude in a throwaway profile, to look at the window without real
# sessions. Usage: powershell -File scripts\try-fake.ps1 [-Sessions 3] [-Shots <folder>] [-Seconds 6]
# With -Shots it captures the windows after -Seconds and closes the app again.
param([int]$Sessions = 3, [string]$Shots = '', [int]$Seconds = 6, [switch]$Release)

$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $env:TEMP ("reframe-try-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force "$dir\project" | Out-Null
$env:REFRAME_USER_DATA = "$dir\user-data"
$env:REFRAME_CLAUDE_HOME = "$dir\claude-home"
$env:REFRAME_CLAUDE_BIN = "$root\test\fake-claude\fake-claude.mjs"
$env:REFRAME_NODE = (Get-Command node).Source
$env:REFRAME_LOCALE = 'en'
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue

$exe = Join-Path $root ("target\" + $(if ($Release) { 'release' } else { 'debug' }) + "\reframe.exe")
$arguments = @()
for ($i = 0; $i -lt $Sessions; $i++) { $arguments += @('--new', "$dir\project") }
$app = Start-Process $exe -ArgumentList $arguments -PassThru
"profile: $dir (pid $($app.Id))"
if ($Shots) {
    Start-Sleep -Seconds $Seconds
    powershell -File "$PSScriptRoot\capture.ps1" $Shots
    $null = $app.CloseMainWindow()
    if (-not $app.WaitForExit(20000)) { "the app did not quit" }
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}
