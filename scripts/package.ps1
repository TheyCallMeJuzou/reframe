# Builds what a release publishes, into dist\: the installer (reframe-Setup-<version>.exe), latest.yml for the
# updaters (the native one and the 0.x copies' electron-updater read it), and the portable ZIP
# (reframe-<version>-win-portable.zip, with a folder "data" next to reframe.exe).
# Usage: powershell -File scripts\package.ps1 [-SkipBuild] [-Defines @{ PRODUCT = '...'; ... }] [-Out <folder>]
# -Defines overrides the installer's names (scripts\test-installer.ps1 builds one that cannot touch a real install).
# Needs NSIS 3 (makensis): MAKENSIS, the PATH, its usual folder, or electron-builder's download of it.
param(
    [switch]$SkipBuild,
    [hashtable]$Defines = @{},
    [string]$Out = 'dist'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

function Find-Makensis {
    if ($env:MAKENSIS -and (Test-Path $env:MAKENSIS)) { return $env:MAKENSIS }
    $onPath = Get-Command makensis -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    foreach ($candidate in @("${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:ProgramFiles\NSIS\makensis.exe")) {
        if (Test-Path $candidate) { return $candidate }
    }
    $cache = Join-Path $env:LOCALAPPDATA 'electron-builder\Cache\nsis'
    $found = Get-ChildItem $env:LOCALAPPDATA\electron-builder\Cache -Recurse -Filter makensis.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Name -ne 'Bin' } | Select-Object -First 1
    if ($found) { return $found.FullName }
    throw "makensis not found: install NSIS 3 or set MAKENSIS ($cache)"
}

# The version, and the names the app has (the installer has to use the same).
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$app = Get-Content crates\reframe\src\app.rs -Raw
$appId = [regex]::Match($app, 'pub const APP_ID: &str = "([^"]+)"').Groups[1].Value
$profileFolder = [regex]::Match($app, 'const PROFILE_FOLDER: &str = "([^"]+)"').Groups[1].Value
if (-not $appId -or -not $profileFolder) { throw 'APP_ID or PROFILE_FOLDER not found in app.rs' }
$numbers = (($version -split '[-+]')[0] -split '\.') + @('0')
$versionNumbers = ($numbers[0..3]) -join '.'

if (-not $SkipBuild) {
    & cargo build --release -p reframe
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}

$out = Join-Path $root $Out
New-Item -ItemType Directory -Force $out | Out-Null
# What gets installed, under the names it is installed as.
$stage = Join-Path $out 'stage'
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item target\release\reframe.exe $stage
Copy-Item target\release\conpty.dll, target\release\OpenConsole.exe $stage
Copy-Item LICENSE (Join-Path $stage 'LICENSE.txt')

# The installer.
$setupName = "reframe-Setup-$version.exe"
$setup = Join-Path $out $setupName
$all = @{
    VERSION = $version
    VERSION_NUMBERS = $versionNumbers
    SOURCE = $stage
    ICON = (Join-Path $root 'crates\reframe\assets\reframe.ico')
    OUTFILE = $setup
    APP_ID = $appId
    PROFILE_FOLDER = $profileFolder
}
foreach ($key in $Defines.Keys) { $all[$key] = $Defines[$key] }
$arguments = @('/V2') + ($all.GetEnumerator() | Sort-Object Name | ForEach-Object { "/D$($_.Name)=$($_.Value)" }) +
    @((Join-Path $root 'installer\reframe.nsi'))
$makensis = Find-Makensis
& $makensis @arguments
if ($LASTEXITCODE -ne 0) { throw 'makensis failed' }

# latest.yml, as electron-builder writes it: the 0.x copies' updater checks the installer by this SHA-512.
$bytes = [System.IO.File]::ReadAllBytes($setup)
$sha512 = [Convert]::ToBase64String([System.Security.Cryptography.SHA512]::Create().ComputeHash($bytes))
$date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'")
$yml = "version: $version`nfiles:`n  - url: $setupName`n    sha512: $sha512`n    size: $($bytes.Length)`npath: $setupName`nsha512: $sha512`nreleaseDate: '$date'`n"
[System.IO.File]::WriteAllText((Join-Path $out 'latest.yml'), $yml)

# The portable copy: everything reframe keeps goes into "data" next to it.
$portableName = "reframe-$version-win-portable"
$portable = Join-Path $out $portableName
Remove-Item -Recurse -Force $portable -ErrorAction SilentlyContinue
Remove-Item -Force "$portable.zip" -ErrorAction SilentlyContinue
Copy-Item -Recurse $stage $portable
New-Item -ItemType Directory (Join-Path $portable 'data') | Out-Null
$readme = @(
    'reframe keeps everything here: its sessions, windows, settings and log.',
    'Keep this folder next to reframe.exe. Without it, reframe keeps its data where an installed one does',
    '(%APPDATA%\reframe).',
    ''
) -join "`r`n"
[System.IO.File]::WriteAllText((Join-Path $portable 'data\README.txt'), $readme)
# Not just "tar": Git's GNU tar can come first on the PATH, and it does not make zip files.
& "$env:SystemRoot\System32\tar.exe" -c -f "$portable.zip" --format zip -C $out $portableName
if ($LASTEXITCODE -ne 0) { throw 'zip failed' }
Remove-Item -Recurse -Force $portable

foreach ($file in @($setup, (Join-Path $out 'latest.yml'), "$portable.zip")) {
    $size = (Get-Item $file).Length
    Write-Host ("{0}: {1:N1} MB" -f (Resolve-Path -Relative $file), ($size / 1MB))
}
