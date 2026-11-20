# Tries the installer out under names of its own (product "reframe-installer-test", its own uninstall entry, app id,
# updater folder and profile), so it cannot touch an installed reframe:
#  1. over a stand-in of the Electron version, run as that version's updater runs it on quit (--updated /S), while
#     the stand-in is still on its way out;
#  2. uninstalled;
#  3. installed fresh into a folder of its own with --force-run: shortcuts with reframe's app id, reframe starts;
#  4. installed by hand over that running reframe: it is asked to quit;
#  5. uninstalled.
# Puts shortcuts into the Start menu and onto the desktop for a few seconds, and removes everything it made.
# Usage: powershell -File scripts\test-installer.ps1 [-SkipBuild]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$product = 'reframe-installer-test'
$guid = 'reframe-installer-test-5b0e'
$appId = 'dev.reframe.installer-test'
$cache = 'reframe-installer-test-updater'
$profileFolder = 'reframe-installer-test'
# In long form: TEMP can be a short 8.3 path (C:\Users\RUNNER~1\... on CI), and shortcuts give
# their target in long form, so the two would not compare equal.
Add-Type -Namespace Native -Name LongPath -MemberDefinition '[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint size);'
$long = New-Object System.Text.StringBuilder 1024
$temp = if ([Native.LongPath]::GetLongPathName($env:TEMP, $long, 1024) -gt 0) { $long.ToString() } else { $env:TEMP }
$work = Join-Path $temp 'rf-installer-test'
$out = 'dist-installer-test'

$installKey = "HKCU:\Software\$guid"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$guid"
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$aumidKey = "HKCU:\Software\Classes\AppUserModelId\$appId"
$startMenuLink = Join-Path ([Environment]::GetFolderPath('Programs')) "$product.lnk"
$desktopLink = Join-Path ([Environment]::GetFolderPath('Desktop')) "$product.lnk"
$cacheDir = Join-Path $env:LOCALAPPDATA $cache
$profileDir = Join-Path $env:APPDATA $profileFolder

$failures = New-Object System.Collections.Generic.List[string]
function Check($ok, $what) {
    if ($ok) { Write-Host "  ok   $what" } else { Write-Host "  FAIL $what"; $failures.Add($what) }
}

function Stop-From($folder) {
    Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith("$folder\", 'OrdinalIgnoreCase') } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Clean-Up {
    Stop-From $work
    foreach ($key in @($installKey, $uninstallKey, $aumidKey)) { Remove-Item -Recurse -Force $key -ErrorAction SilentlyContinue }
    Remove-ItemProperty -Path $runKey -Name $appId -ErrorAction SilentlyContinue
    foreach ($path in @($startMenuLink, $desktopLink, $cacheDir, $profileDir, $work)) {
        Remove-Item -Recurse -Force $path -ErrorAction SilentlyContinue
    }
}

function Run-Setup([string[]]$arguments) {
    $process = Start-Process -FilePath $setup -ArgumentList $arguments -PassThru
    if (-not $process.WaitForExit(120000)) { throw 'the installer did not finish' }
    return $process.ExitCode
}

function Shortcut-Target($link) {
    (New-Object -ComObject WScript.Shell).CreateShortcut($link).TargetPath
}

function Shortcut-AppId($link) {
    $folder = (New-Object -ComObject Shell.Application).Namespace((Split-Path $link))
    $folder.ParseName((Split-Path $link -Leaf)).ExtendedProperty('System.AppUserModel.ID')
}

function Uninstall($folder) {
    & (Join-Path $folder 'Uninstall reframe.exe') /S
    # The uninstaller goes on from a copy of itself; it is done when its entry is gone.
    for ($i = 0; $i -lt 120 -and (Test-Path $uninstallKey); $i++) { Start-Sleep -Milliseconds 250 }
    Start-Sleep -Milliseconds 500
}

function Check-Uninstalled($folder) {
    Check (-not (Test-Path $folder)) 'the install folder is gone'
    Check (-not (Test-Path $uninstallKey)) 'the uninstall entry is gone'
    Check (-not (Test-Path $installKey)) "the install folder's key is gone"
    Check (-not (Test-Path $startMenuLink)) 'the Start menu shortcut is gone'
    Check (-not (Test-Path $desktopLink)) 'the desktop shortcut is gone'
    Check ($null -eq (Get-ItemProperty $runKey -Name $appId -ErrorAction SilentlyContinue)) 'it no longer starts with the system'
    Check (-not (Test-Path $aumidKey)) "Windows forgot its notifications' name"
    Check (Test-Path (Join-Path $profileDir 'state.json')) 'the sessions and settings are kept'
}

Clean-Up
try {
    $defines = @{ PRODUCT = $product; GUID = $guid; APP_ID = $appId; UPDATER_CACHE = $cache; PROFILE_FOLDER = $profileFolder }
    $package = @{ Defines = $defines; Out = $out }
    if ($SkipBuild) { $package.SkipBuild = $true }
    & (Join-Path $PSScriptRoot 'package.ps1') @package
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
    $setup = Join-Path $root "$out\reframe-Setup-$version.exe"

    # ---------- 1. over the Electron version, as its updater runs it ----------
    Write-Host '== over the Electron version (--updated /S)'
    $electron = Join-Path $work 'over-electron'
    New-Item -ItemType Directory -Force "$electron\resources", "$electron\locales" | Out-Null
    # The stand-in for Electron's reframe.exe is ping, which runs for a few seconds: reframe on its way out.
    Copy-Item "$env:SystemRoot\System32\PING.EXE" "$electron\reframe.exe"
    foreach ($file in 'Uninstall reframe.exe', 'resources\app.asar', 'resources\app-update.yml', 'locales\en-US.pak',
        'chrome_100_percent.pak', 'resources.pak', 'ffmpeg.dll', 'vulkan-1.dll', 'icudtl.dat', 'snapshot_blob.bin',
        'v8_context_snapshot.bin', 'LICENSES.chromium.html', 'LICENSE.electron.txt', 'vk_swiftshader_icd.json') {
        Set-Content -Path (Join-Path $electron $file) -Value 'electron'
    }
    New-Item -Force $installKey | Out-Null
    Set-ItemProperty $installKey -Name InstallLocation -Value $electron
    Set-ItemProperty $installKey -Name KeepShortcuts -Value 'true'
    Set-ItemProperty $installKey -Name ShortcutName -Value $product
    New-Item -Force $uninstallKey | Out-Null
    Set-ItemProperty $uninstallKey -Name DisplayName -Value "$product 0.10.1"
    Set-ItemProperty $uninstallKey -Name DisplayVersion -Value '0.10.1'
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($startMenuLink)
    $link.TargetPath = "$electron\reframe.exe"
    $link.Save()
    $linkWritten = (Get-Item $startMenuLink).LastWriteTimeUtc
    Set-ItemProperty $runKey -Name $appId -Value "`"$electron\reframe.exe`" --autostart"
    New-Item -ItemType Directory -Force "$cacheDir\pending" | Out-Null
    Set-Content "$cacheDir\pending\reframe-Setup-0.10.1.exe" 'old download'
    New-Item -ItemType Directory -Force $profileDir | Out-Null
    Set-Content (Join-Path $profileDir 'state.json') '{"version":1}'

    $leaving = Start-Process -FilePath "$electron\reframe.exe" -ArgumentList '-n', '4', '127.0.0.1' -WindowStyle Hidden -PassThru
    $started = Get-Date
    $code = Run-Setup @('--updated', '/S')
    Check ($code -eq 0) "the installer succeeded (exit code $code)"
    Check $leaving.HasExited 'it waited for the old reframe to be gone'
    Check (((Get-Date) - $started).TotalSeconds -lt 30) 'without ending it by force'
    foreach ($gone in 'resources', 'locales', 'chrome_100_percent.pak', 'resources.pak', 'ffmpeg.dll', 'vulkan-1.dll',
        'icudtl.dat', 'snapshot_blob.bin', 'v8_context_snapshot.bin', 'LICENSES.chromium.html', 'LICENSE.electron.txt',
        'vk_swiftshader_icd.json') {
        Check (-not (Test-Path (Join-Path $electron $gone))) "the Electron version's $gone is gone"
    }
    foreach ($there in 'reframe.exe', 'conpty.dll', 'OpenConsole.exe', 'LICENSE.txt', 'Uninstall reframe.exe') {
        Check (Test-Path (Join-Path $electron $there)) "$there is installed"
    }
    $info = (Get-Item "$electron\reframe.exe").VersionInfo
    Check ($info.ProductName -eq 'reframe' -and $info.ProductVersion -eq $version) "reframe.exe is reframe $version"
    $entry = Get-ItemProperty $uninstallKey
    Check ($entry.DisplayName -eq "$product $version" -and $entry.DisplayVersion -eq $version) 'the uninstall entry names the new version'
    Check ($entry.UninstallString -eq "`"$electron\Uninstall reframe.exe`" /currentuser") 'the uninstall entry runs the new uninstaller'
    Check ((Get-ItemProperty $installKey).InstallLocation -eq $electron) 'installed where the Electron version was'
    Check ((Get-Item $startMenuLink).LastWriteTimeUtc -eq $linkWritten) 'the Start menu shortcut was kept as it was'
    Check (-not (Test-Path $desktopLink)) 'no desktop shortcut appeared (the user had none)'
    Check ((Get-ItemProperty $runKey).$appId -eq "`"$electron\reframe.exe`" --autostart") 'it still starts with the system'
    Check (-not (Test-Path $cacheDir)) "the updater's downloads are gone"
    Check ((Get-Content (Join-Path $profileDir 'state.json')) -eq '{"version":1}') 'the sessions and settings are kept'
    Check ($null -eq (Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq "$electron\reframe.exe" })) 'installed on quit, reframe does not start'

    # ---------- 2. uninstalled ----------
    Write-Host '== uninstalled'
    Uninstall $electron
    Check-Uninstalled $electron

    # ---------- 3. fresh, started afterwards ----------
    Write-Host '== fresh install with --force-run'
    $fresh = Join-Path $work 'fresh'
    $testProfile = Join-Path $work 'profile'
    # The reframe it starts uses a profile of its own and the test channel, to be told to quit.
    $env:REFRAME_USER_DATA = $testProfile
    $env:REFRAME_E2E = '1'
    $env:REFRAME_DISABLE_NOTIFICATIONS = '1'
    $code = Run-Setup @('/S', '--force-run', "/D=$fresh")
    Check ($code -eq 0) "the installer succeeded (exit code $code)"
    Check ((Shortcut-Target $startMenuLink) -eq "$fresh\reframe.exe") 'a Start menu shortcut starts reframe'
    Check ((Shortcut-Target $desktopLink) -eq "$fresh\reframe.exe") 'a desktop shortcut starts reframe'
    Check ((Shortcut-AppId $startMenuLink) -eq $appId) "the Start menu shortcut carries reframe's app id"
    Check ((Shortcut-AppId $desktopLink) -eq $appId) "the desktop shortcut carries reframe's app id"
    $portFile = Join-Path $testProfile 'e2e-port'
    for ($i = 0; $i -lt 100 -and -not (Test-Path $portFile); $i++) { Start-Sleep -Milliseconds 100 }
    Check (Test-Path $portFile) 'reframe started after the install'
    $running = Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq "$fresh\reframe.exe" } | Select-Object -First 1

    # ---------- 4. by hand, over the running reframe ----------
    Write-Host '== by hand over the running reframe (/S)'
    $code = Run-Setup @('/S')
    Check ($code -eq 0) "the installer succeeded (exit code $code)"
    Check ($running -and -not (Get-Process -Id $running.ProcessId -ErrorAction SilentlyContinue)) 'the running reframe is gone'
    $log = Get-Content (Join-Path $testProfile 'logs\main.log') -Raw -ErrorAction SilentlyContinue
    Check ($log -match 'the installer asks reframe to quit' -and $log -match 'reframe stopped') 'it was asked to quit and quit as the user would'
    Check ((Get-ItemProperty $installKey).InstallLocation -eq $fresh) 'installed into the same folder again'
    Remove-Item Env:REFRAME_USER_DATA, Env:REFRAME_E2E, Env:REFRAME_DISABLE_NOTIFICATIONS

    # ---------- 5. uninstalled ----------
    Write-Host '== uninstalled'
    Uninstall $fresh
    Check-Uninstalled $fresh

    # ---------- 6. the portable copy ----------
    Write-Host '== the portable copy'
    $portable = Join-Path $work 'portable'
    New-Item -ItemType Directory -Force $portable | Out-Null
    & "$env:SystemRoot\System32\tar.exe" -x -f (Join-Path $root "$out\reframe-$version-win-portable.zip") -C $portable
    $copy = Join-Path $portable "reframe-$version-win-portable"
    $data = Join-Path $copy 'data'
    Check (Test-Path (Join-Path $data 'README.txt')) 'the ZIP has a folder "data" next to reframe.exe'
    $ids = 'HKCU:\Software\Classes\AppUserModelId'
    $idsBefore = @(Get-ChildItem $ids -ErrorAction SilentlyContinue | ForEach-Object PSChildName)
    $env:REFRAME_E2E = '1'
    $env:REFRAME_DISABLE_NOTIFICATIONS = '1'
    $app = Start-Process -FilePath (Join-Path $copy 'reframe.exe') -PassThru
    Remove-Item Env:REFRAME_E2E, Env:REFRAME_DISABLE_NOTIFICATIONS
    $portFile = Join-Path $data 'e2e-port'
    for ($i = 0; $i -lt 100 -and -not (Test-Path $portFile); $i++) { Start-Sleep -Milliseconds 100 }
    Check (Test-Path $portFile) 'it keeps its profile in "data"'
    if (Test-Path $portFile) {
        $client = New-Object System.Net.Sockets.TcpClient('127.0.0.1', [int](Get-Content $portFile))
        $stream = $client.GetStream()
        $writer = New-Object System.IO.StreamWriter($stream)
        $reader = New-Object System.IO.StreamReader($stream)
        $writer.WriteLine('{"cmd":"updates"}'); $writer.Flush()
        $updates = $reader.ReadLine() | ConvertFrom-Json
        Check ($updates.mode -eq 'notify') 'it only tells about updates'
        $writer.WriteLine('{"cmd":"quit"}'); $writer.Flush()
        $null = $reader.ReadLine()
        $client.Close()
    }
    Check ($app.WaitForExit(30000) -and $app.ExitCode -eq 0) 'it quits'
    Check (Test-Path (Join-Path $data 'state.json')) 'its state is in "data"'
    foreach ($id in @(Get-ChildItem $ids -ErrorAction SilentlyContinue | ForEach-Object PSChildName)) {
        if ($idsBefore -notcontains $id) { Remove-Item -Recurse -Force (Join-Path $ids $id) }
    }
}
finally {
    Remove-Item Env:REFRAME_USER_DATA, Env:REFRAME_E2E, Env:REFRAME_DISABLE_NOTIFICATIONS -ErrorAction SilentlyContinue
    Clean-Up
    Remove-Item -Recurse -Force (Join-Path $root $out) -ErrorAction SilentlyContinue
}
if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) check(s) failed"
    exit 1
}
Write-Host 'the installer works'
