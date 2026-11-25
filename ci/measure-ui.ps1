# What a UI costs on a Windows machine with no graphics card (a GitHub runner's adapter is WARP): the same method as ci/measure-ui.sh.
# Start it, place its window at the top left, settle, read the process's CPU time over an idle stretch, then over the pointer moving
# round an ellipse over the plan, the wheel (ten notches in, ten out) and a drag that pans (the middle button, which egui and, since stage 3's selection slice, Rune pan with; -PanButton 1 is the left),
# and keep a screenshot. The pointer is the real one (SetCursorPos, mouse_event), so a window must be on the desktop, as it is on a runner.
#
# usage: pwsh ci/measure-ui.ps1 -Name NAME -Out DIR -Exe PATH [-AppArgs a,b] [-PanButton 1|2]
param(
    [Parameter(Mandatory)][string]$Name,
    [Parameter(Mandatory)][string]$Out,
    [Parameter(Mandatory)][string]$Exe,
    [string[]]$AppArgs = @(),
    [int]$Idle = 10,
    [int]$Moves = 1500,
    [int]$Notches = 200,
    [int]$Drags = 1000,
    [int]$Settle = 15,
    [int]$PanButton = 2
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Win {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
}
"@
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

if ($AppArgs.Count -gt 0) { $p = Start-Process -FilePath $Exe -ArgumentList $AppArgs -PassThru } else { $p = Start-Process -FilePath $Exe -PassThru }
try {
    $window = [IntPtr]::Zero
    for ($i = 0; $i -lt 240; $i++) {
        $p.Refresh()
        if ($p.HasExited) { throw "$Name exited before it showed a window (code $($p.ExitCode))" }
        $window = $p.MainWindowHandle
        if ($window -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($window -eq [IntPtr]::Zero) { throw "$Name showed no window in two minutes" }
    [void][Win]::MoveWindow($window, 0, 0, 1280, 800, $true)
    Start-Sleep -Seconds $Settle
    $p.Refresh()
    if ($p.HasExited) { throw "$Name went away while it was being placed" }

    function Cpu { $p.Refresh(); return $p.TotalProcessorTime.TotalSeconds }
    function Stretch([scriptblock]$Work) {
        $c0 = Cpu; $t0 = [DateTime]::UtcNow
        & $Work
        $seconds = ([DateTime]::UtcNow - $t0).TotalSeconds
        return @{ cpu = (Cpu) - $c0; seconds = $seconds }
    }

    $sIdle = Stretch { Start-Sleep -Seconds $Idle }
    $sMove = Stretch {
        for ($i = 0; $i -lt $Moves; $i++) {
            $a = $i * 0.15
            [void][Win]::SetCursorPos([int](560 + 260 * [Math]::Cos($a)), [int](430 + 200 * [Math]::Sin($a)))
            Start-Sleep -Milliseconds 16
        }
    }
    $sZoom = Stretch {
        [void][Win]::SetCursorPos(560, 430)
        Start-Sleep -Milliseconds 200
        for ($i = 0; $i -lt $Notches; $i++) {
            $delta = if ([Math]::Floor($i / 10) % 2 -eq 0) { 120 } else { -120 }
            [Win]::mouse_event(0x0800, 0, 0, $delta, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 30
        }
    }
    $down, $up = if ($PanButton -eq 2) { 0x0020, 0x0040 } else { 0x0002, 0x0004 }
    $sPan = Stretch {
        [void][Win]::SetCursorPos(560, 430)
        Start-Sleep -Milliseconds 200
        [Win]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
        for ($i = 0; $i -le $Drags; $i++) {
            $a = $i * 6.2832 / $Drags
            [void][Win]::SetCursorPos([int](460 + 100 * [Math]::Cos($a)), [int](430 + 100 * [Math]::Sin($a)))
            Start-Sleep -Milliseconds 16
        }
        [Win]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
    }

    $p.Refresh()
    $rss = [int]($p.WorkingSet64 / 1MB)
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $shot = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    [System.Drawing.Graphics]::FromImage($shot).CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $shot.Save((Join-Path $Out "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)

    $r = [ordered]@{
        name = $Name
        idle_cores = [Math]::Round($sIdle.cpu / $sIdle.seconds, 3)
        move_cores = [Math]::Round($sMove.cpu / $sMove.seconds, 3)
        ms_cpu_per_move = [Math]::Round($sMove.cpu * 1000 / $Moves, 2)
        zoom_cores = [Math]::Round($sZoom.cpu / $sZoom.seconds, 3)
        ms_cpu_per_zoom = [Math]::Round($sZoom.cpu * 1000 / $Notches, 2)
        pan_cores = [Math]::Round($sPan.cpu / $sPan.seconds, 3)
        ms_cpu_per_pan = [Math]::Round($sPan.cpu * 1000 / $Drags, 2)
        rss_mb = $rss
        screen = "$($bounds.Width)x$($bounds.Height)"
    }
    $json = $r | ConvertTo-Json -Compress
    Set-Content -Path (Join-Path $Out "$Name.json") -Value $json
    Write-Host $json
} finally {
    if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
