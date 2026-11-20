# Saves a picture of each window of a reframe built here (target\, so never an installed reframe's), as PNG.
# Usage: powershell -File scripts\capture.ps1 <output folder>
param([Parameter(Mandatory = $true)][string]$OutDir)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Win {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr lparam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public static List<IntPtr> WindowsOf(uint pid) {
        var found = new List<IntPtr>();
        EnumWindows((hwnd, l) => {
            uint owner; GetWindowThreadProcessId(hwnd, out owner);
            if (owner == pid && IsWindowVisible(hwnd)) found.Add(hwnd);
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

[Win]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $OutDir | Out-Null
$index = 0
$built = Join-Path (Split-Path $PSScriptRoot -Parent) 'target'
$processes = Get-Process reframe -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith("$built\", 'OrdinalIgnoreCase') }
foreach ($process in $processes) {
    foreach ($hwnd in [Win]::WindowsOf([uint32]$process.Id)) {
        $rect = New-Object Win+RECT
        [Win]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
        $width = $rect.Right - $rect.Left
        $height = $rect.Bottom - $rect.Top
        if ($width -lt 50 -or $height -lt 50) { continue }
        $bitmap = New-Object System.Drawing.Bitmap $width, $height
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $hdc = $graphics.GetHdc()
        # 2 = PW_RENDERFULLCONTENT: needed for windows drawn with DirectComposition.
        [Win]::PrintWindow($hwnd, $hdc, 2) | Out-Null
        $graphics.ReleaseHdc($hdc)
        $file = Join-Path $OutDir "window-$index.png"
        $bitmap.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
        "$file ($width x $height)"
        $index++
    }
}
if ($index -eq 0) { "no window of a reframe built here found" }
