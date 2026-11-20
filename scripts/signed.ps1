# After SignPath signed a release's Windows files (see .github/workflows/release.yml): checks the signatures, puts the
# signed installer and portable ZIP into dist\ and writes latest.yml anew (the signed installer has another
# checksum).
# Usage: powershell -File scripts\signed.ps1 <folder with the signed files>
param([Parameter(Mandatory = $true)][string]$Signed)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root 'dist'
$setup = Join-Path $Signed "reframe-Setup-$version.exe"
$zip = Join-Path $Signed "reframe-$version-win-portable.zip"

function Assert-Signed($file) {
    $signature = Get-AuthenticodeSignature $file
    if ($signature.Status -ne 'Valid') { throw "$file is not signed ($($signature.Status))" }
}
Assert-Signed $setup
# reframe.exe in the portable copy.
$check = Join-Path $env:TEMP "reframe-signed-$version"
Remove-Item -Recurse -Force $check -ErrorAction SilentlyContinue
New-Item -ItemType Directory $check | Out-Null
& "$env:SystemRoot\System32\tar.exe" -x -f $zip -C $check
Assert-Signed (Join-Path $check "reframe-$version-win-portable\reframe.exe")
Remove-Item -Recurse -Force $check

Copy-Item $setup, $zip $dist -Force
& (Join-Path $PSScriptRoot 'latest-yml.ps1') -Setup (Join-Path $dist "reframe-Setup-$version.exe") -Version $version
Write-Host "signed files in place, latest.yml written for reframe $version"
