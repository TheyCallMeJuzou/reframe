# Measures one running app: memory and idle CPU of its own processes, and of everything under it.
# Usage: powershell -File scripts\measure.ps1 -RootPid <pid of the app's first process> -Label <name>
param(
    [Parameter(Mandatory = $true)][int]$RootPid,
    [Parameter(Mandatory = $true)][string]$Label,
    [int]$CpuSeconds = 10
)

$all = Get-CimInstance Win32_Process
$root = $all | Where-Object ProcessId -eq $RootPid
if (-not $root) { "process $RootPid not found"; exit 1 }

# Everything started by the app, directly or not.
$tree = @($root)
$queue = @($RootPid)
while ($queue.Count) {
    $next = @()
    foreach ($child in $all | Where-Object { $queue -contains $_.ParentProcessId -and $_.ProcessId -ne $RootPid }) {
        if ($tree.ProcessId -notcontains $child.ProcessId) { $tree += $child; $next += $child.ProcessId }
    }
    $queue = $next
}

# The app itself: processes running the same executable as the root.
$own = $tree | Where-Object { $_.Name -eq $root.Name }
$privateWs = @{}
Get-CimInstance Win32_PerfRawData_PerfProc_Process | ForEach-Object { $privateWs[[int]$_.IDProcess] = $_.WorkingSetPrivate }

function Sum-Memory($processes) {
    $private = 0; $working = 0
    foreach ($p in $processes) {
        $private += $p.PrivatePageCount
        if ($privateWs.ContainsKey([int]$p.ProcessId)) { $working += $privateWs[[int]$p.ProcessId] }
    }
    [pscustomobject]@{ Private = $private / 1MB; PrivateWorkingSet = $working / 1MB }
}
function Cpu-Time($processes) {
    $total = 0.0
    foreach ($p in $processes) {
        $proc = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
        if ($proc) { $total += $proc.TotalProcessorTime.TotalMilliseconds }
    }
    $total
}

$before = Cpu-Time $own
Start-Sleep $CpuSeconds
$after = Cpu-Time $own
$ownMemory = Sum-Memory $own
$treeMemory = Sum-Memory $tree
$groups = $tree | Group-Object Name | ForEach-Object { "$($_.Count)x $($_.Name)" }

"== $Label"
"own processes:            {0}" -f $own.Count
"own committed memory:     {0:N0} MB" -f $ownMemory.Private
"own private working set:  {0:N0} MB" -f $ownMemory.PrivateWorkingSet
"own idle CPU:             {0:N2} % of one core over {1} s" -f (($after - $before) / ($CpuSeconds * 10)), $CpuSeconds
"whole tree:               {0:N0} MB committed, {1:N0} MB private working set ({2})" -f $treeMemory.Private, $treeMemory.PrivateWorkingSet, ($groups -join ', ')
